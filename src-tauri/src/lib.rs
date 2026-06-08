use chrono::{DateTime, SecondsFormat, Utc};
use fs2::FileExt;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child as StdChild, Command as StdCommand, Output, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

const DEFAULT_PROFILE_ID: &str = "default";
const SHARED_AUTH_RELATIVE_PATH: &[&str] = &["pioc-auth", "auth.json"];
const AUTH_LOCK_FILE: &str = "auth.lock";
const PROFILES_DIR_NAME: &str = "pi-profiles";
const SHARED_ADDONS_DIR_NAME: &str = "pi-addons";
const PIOC_CONTROL_DIR_NAME: &str = "pioc-control";
const PIOC_CONTROL_EXTENSION_FILE: &str = "pioc-control.ts";
const PIOC_CONTROL_EXTENSION_SOURCE: &str = include_str!("../../pi-extensions/pioc-control.ts");
const PROFILE_META_FILE: &str = "profile-meta.json";
const PROFILE_SETTINGS_FILE: &str = "settings.json";
const PROFILE_AUTH_FILE: &str = "auth.json";
const PROFILE_SESSIONS_DIR: &str = "sessions";
const GLOBAL_PACKAGES_SETTINGS_KEY: &str = "globalPackages";
const PROFILE_GENERATED_SETTINGS_KEYS: &[&str] = &[
    "defaultProvider",
    "defaultModel",
    "defaultThinkingLevel",
    "hideThinkingBlock",
    "theme",
    "quietStartup",
    "collapseChangelog",
    "enableInstallTelemetry",
    "doubleEscapeAction",
    "treeFilterMode",
    "steeringMode",
    "followUpMode",
    "transport",
    "showHardwareCursor",
    "enableSkillCommands",
    "packages",
    "skills",
    "extensions",
    "prompts",
    "themes",
];
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const PI_PACKAGE_COMMAND_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PI_PACKAGE_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiProfile {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_thinking_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hide_thinking_block: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quiet_startup: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    collapse_changelog: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enable_install_telemetry: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    double_escape_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tree_filter_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    steering_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    follow_up_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    show_hardware_cursor: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enable_skill_commands: Option<bool>,
    #[serde(default)]
    packages: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    prompts: Vec<String>,
    #[serde(default)]
    themes: Vec<String>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    extra_settings: serde_json::Map<String, serde_json::Value>,
    created_at: String,
    updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_used_at: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiCommandOutput {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiAddonPackage {
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed_path: Option<String>,
    #[serde(default)]
    resource_types: Vec<String>,
    #[serde(default)]
    global: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiAddonsMigrationResult {
    migrated_profiles: usize,
    migrated_packages: usize,
    copied_packages: usize,
}

#[derive(Clone)]
struct PiocControlPaths {
    telemetry_path: PathBuf,
    command_path: PathBuf,
}
struct PiLaunchProfile {
    id: String,
    profile_dir: PathBuf,
    session_dir: PathBuf,
}

enum PtySession {
    Starting,
    Running {
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        child: Box<dyn Child + Send + Sync>,
        pi_profile_id: Option<String>,
    },
}
#[derive(Clone, Copy)]
enum PtyKind {
    Pi,
    Shell,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PtyStartRequest {
    id: u64,
    cols: u16,
    rows: u16,
    mode: Option<String>,
    working_directory: Option<String>,
    pi_profile_id: Option<String>,
}

#[derive(Default)]
struct PtyState {
    sessions: Mutex<HashMap<u64, PtySession>>,
    running_pi_profiles: Mutex<HashMap<String, u64>>,
}

impl Drop for PtyState {
    fn drop(&mut self) {
        if let Ok(sessions) = self.sessions.get_mut() {
            for (_, session) in sessions.drain() {
                kill_running_session_sync(session);
            }
        }
    }
}

#[derive(Clone, Serialize)]
struct PtyDataPayload {
    id: u64,
    data: String,
}

#[derive(Clone, Serialize)]
struct PtyExitPayload {
    id: u64,
}

#[derive(Clone, Serialize)]
struct PtyReadyPayload {
    id: u64,
}

#[derive(Clone, Serialize)]
struct PtyErrorPayload {
    id: u64,
    error: String,
}

fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn now_millis_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn unix_seconds_to_iso(value: &str) -> Option<String> {
    let seconds = value.parse::<i64>().ok()?;
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn normalize_timestamp(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(unix_seconds_to_iso(value).unwrap_or_else(|| value.to_string()))
    }
}

fn is_valid_profile_id(profile_id: &str) -> bool {
    !profile_id.is_empty()
        && profile_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn validate_profile_id(profile_id: &str) -> Result<(), String> {
    if is_valid_profile_id(profile_id) {
        Ok(())
    } else {
        Err("profile IDs may only contain letters, numbers, underscores, and dashes".to_string())
    }
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("unable to resolve app data directory: {error}"))?;

    fs::create_dir_all(&app_data_dir)
        .map_err(|error| format!("unable to create app data directory: {error}"))?;
    Ok(app_data_dir)
}

fn shared_auth_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut path = app_data_dir(app)?;
    for segment in SHARED_AUTH_RELATIVE_PATH {
        path.push(segment);
    }
    Ok(path)
}

fn profiles_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join(PROFILES_DIR_NAME))
}

fn shared_addons_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join(SHARED_ADDONS_DIR_NAME))
}

fn pioc_control_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join(PIOC_CONTROL_DIR_NAME);
    fs::create_dir_all(&dir)
        .map_err(|error| format!("unable to create PIOC control directory: {error}"))?;
    Ok(dir)
}

fn pioc_control_paths(app: &AppHandle, id: u64) -> Result<PiocControlPaths, String> {
    let dir = pioc_control_dir(app)?;
    Ok(PiocControlPaths {
        telemetry_path: dir.join(format!("terminal-{id}.json")),
        command_path: dir.join(format!("terminal-{id}.commands.jsonl")),
    })
}

fn ensure_pioc_control_extension(app: &AppHandle) -> Result<PathBuf, String> {
    let path = pioc_control_dir(app)?.join(PIOC_CONTROL_EXTENSION_FILE);
    fs::write(&path, PIOC_CONTROL_EXTENSION_SOURCE).map_err(|error| {
        format!(
            "unable to write PIOC control extension {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}

fn profile_dir(app: &AppHandle, profile_id: &str) -> Result<PathBuf, String> {
    validate_profile_id(profile_id)?;
    Ok(profiles_root(app)?.join(profile_id))
}

fn ensure_shared_auth(app: &AppHandle) -> Result<PathBuf, String> {
    let auth_path = shared_auth_path(app)?;
    if let Some(parent) = auth_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("unable to create shared auth directory: {error}"))?;
    }

    if !auth_path.exists() {
        fs::write(&auth_path, b"{}\n")
            .map_err(|error| format!("unable to initialize shared auth file: {error}"))?;
    }

    Ok(auth_path)
}

fn default_profile() -> PiProfile {
    let now = now_string();

    PiProfile {
        id: DEFAULT_PROFILE_ID.to_string(),
        name: "Default Pi".to_string(),
        description: Some("Default reusable Pi profile".to_string()),
        default_provider: None,
        default_model: None,
        default_thinking_level: None,
        hide_thinking_block: None,
        theme: None,
        quiet_startup: None,
        collapse_changelog: None,
        enable_install_telemetry: None,
        double_escape_action: None,
        tree_filter_mode: None,
        steering_mode: None,
        follow_up_mode: None,
        transport: None,
        show_hardware_cursor: None,
        enable_skill_commands: None,
        packages: Vec::new(),
        skills: vec!["skills".to_string()],
        extensions: vec!["extensions".to_string()],
        prompts: vec!["prompts".to_string()],
        themes: vec!["themes".to_string()],
        extra_settings: serde_json::Map::new(),
        created_at: now.clone(),
        updated_at: now,
        last_used_at: None,
    }
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn remove_generated_profile_settings(settings: &mut serde_json::Map<String, serde_json::Value>) {
    for key in PROFILE_GENERATED_SETTINGS_KEYS {
        settings.remove(*key);
    }
}
fn normalize_profile(mut profile: PiProfile) -> Result<PiProfile, String> {
    profile.id = profile.id.trim().to_string();
    validate_profile_id(&profile.id)?;

    profile.name = profile.name.trim().to_string();
    if profile.name.is_empty() {
        return Err("profile name is required".to_string());
    }

    profile.description = trim_optional_string(profile.description);
    profile.default_provider = trim_optional_string(profile.default_provider);
    profile.default_model = trim_optional_string(profile.default_model);
    profile.default_thinking_level = trim_optional_string(profile.default_thinking_level);
    profile.theme = trim_optional_string(profile.theme);
    profile.double_escape_action = trim_optional_string(profile.double_escape_action);
    profile.tree_filter_mode = trim_optional_string(profile.tree_filter_mode);
    profile.steering_mode = trim_optional_string(profile.steering_mode);
    profile.follow_up_mode = trim_optional_string(profile.follow_up_mode);
    profile.transport = trim_optional_string(profile.transport);
    profile.packages = normalize_list(profile.packages);
    profile.skills = normalize_list(profile.skills);
    profile.extensions = normalize_list(profile.extensions);
    profile.prompts = normalize_list(profile.prompts);
    profile.themes = normalize_list(profile.themes);
    remove_generated_profile_settings(&mut profile.extra_settings);

    let now = now_string();
    profile.created_at = normalize_timestamp(&profile.created_at).unwrap_or_else(|| now.clone());
    profile.updated_at = normalize_timestamp(&profile.updated_at).unwrap_or_else(|| now.clone());
    profile.last_used_at = profile
        .last_used_at
        .as_deref()
        .and_then(normalize_timestamp);

    Ok(profile)
}

fn ensure_profile_subdirectories(path: &Path) -> Result<(), String> {
    for directory in [
        "skills",
        "extensions",
        "prompts",
        "themes",
        PROFILE_SESSIONS_DIR,
    ] {
        fs::create_dir_all(path.join(directory))
            .map_err(|error| format!("unable to create profile directory {directory}: {error}"))?;
    }

    Ok(())
}

fn parse_npm_package_name(source: &str) -> Option<String> {
    let spec = source.trim().strip_prefix("npm:")?.trim();
    if spec.is_empty() {
        return None;
    }

    if let Some(rest) = spec.strip_prefix('@') {
        let slash = rest.find('/')?;
        let after_name = &rest[slash + 1..];
        let version_index = after_name.find('@').map(|index| slash + 1 + index);
        let end = version_index.unwrap_or(rest.len());
        return Some(format!("@{}", &rest[..end]));
    }

    Some(
        spec.split_once('@')
            .map(|(name, _)| name)
            .unwrap_or(spec)
            .to_string(),
    )
}

fn strip_git_ref_from_path(path: &str) -> &str {
    let last_slash = path.rfind('/');
    let last_at = path.rfind('@');

    if let Some(last_at) = last_at {
        if last_slash
            .map(|last_slash| last_at > last_slash)
            .unwrap_or(true)
        {
            return &path[..last_at];
        }
    }

    path
}

fn parse_git_package_path(source: &str) -> Option<(String, String)> {
    let mut value = source.trim();
    if let Some(rest) = value.strip_prefix("git:") {
        value = rest;
    } else if !(value.starts_with("https://")
        || value.starts_with("http://")
        || value.starts_with("ssh://")
        || value.starts_with("git://"))
    {
        return None;
    }

    if let Some(rest) = value.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let path = strip_git_ref_from_path(path).trim_end_matches(".git");
        return Some((host.to_string(), path.trim_matches('/').to_string()));
    }

    let without_protocol = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .or_else(|| value.strip_prefix("ssh://"))
        .or_else(|| value.strip_prefix("git://"))
        .unwrap_or(value);
    let without_user = without_protocol
        .strip_prefix("git@")
        .unwrap_or(without_protocol);
    let (host, path) = without_user.split_once('/')?;
    let path = strip_git_ref_from_path(path).trim_end_matches(".git");

    Some((host.to_string(), path.trim_matches('/').to_string()))
}

fn push_relative_segments(path: &mut PathBuf, relative: &str) {
    for segment in relative.split('/') {
        let segment = segment.trim();
        if !segment.is_empty() {
            path.push(segment);
        }
    }
}

fn expected_addon_package_path(agent_dir: &Path, source: &str) -> Option<PathBuf> {
    let source = strip_enclosing_quotes(strip_optional_package_flag(
        strip_pi_package_command_prefix(source),
    ));
    let source = source.trim();
    if source.is_empty() {
        return None;
    }

    if let Some(package_name) = parse_npm_package_name(source) {
        let mut path = agent_dir.join("npm").join("node_modules");
        push_relative_segments(&mut path, &package_name);
        return Some(path);
    }

    if let Some((host, package_path)) = parse_git_package_path(source) {
        let mut path = agent_dir.join("git");
        path.push(host);
        push_relative_segments(&mut path, &package_path);
        return Some(path);
    }

    let path = PathBuf::from(source);
    Some(if path.is_absolute() {
        path
    } else {
        agent_dir.join(path)
    })
}

fn expected_managed_addon_package_path(agent_dir: &Path, source: &str) -> Option<PathBuf> {
    let source = strip_enclosing_quotes(strip_optional_package_flag(
        strip_pi_package_command_prefix(source),
    ));
    let source = source.trim();
    if source.is_empty() {
        return None;
    }

    if let Some(package_name) = parse_npm_package_name(source) {
        let mut path = agent_dir.join("npm").join("node_modules");
        push_relative_segments(&mut path, &package_name);
        return Some(path);
    }

    if let Some((host, package_path)) = parse_git_package_path(source) {
        let mut path = agent_dir.join("git");
        path.push(host);
        push_relative_segments(&mut path, &package_path);
        return Some(path);
    }

    None
}

fn installed_addon_package_path(agent_dir: &Path, source: &str) -> Option<PathBuf> {
    let path = expected_addon_package_path(agent_dir, source)?;
    path.exists().then_some(path)
}

fn shared_installed_addon_path_from_profile_dir(
    profile_dir: &Path,
    source: &str,
) -> Option<PathBuf> {
    let app_data_dir = profile_dir.parent()?.parent()?;
    let shared_addons_dir = app_data_dir.join(SHARED_ADDONS_DIR_NAME);
    installed_addon_package_path(&shared_addons_dir, source)
}

fn profile_package_setting_entry(profile_dir: &Path, package: &str) -> serde_json::Value {
    let package = package.trim();

    if package.starts_with('{') {
        if let Ok(serde_json::Value::Object(mut object)) =
            serde_json::from_str::<serde_json::Value>(package)
        {
            if let Some(source) = object.get("source").and_then(|value| value.as_str()) {
                if let Some(path) =
                    shared_installed_addon_path_from_profile_dir(profile_dir, source)
                {
                    object.insert(
                        "source".to_string(),
                        path.to_string_lossy().to_string().into(),
                    );
                }
            }

            return serde_json::Value::Object(object);
        }
    }

    if let Some(path) = shared_installed_addon_path_from_profile_dir(profile_dir, package) {
        return serde_json::Value::String(path.to_string_lossy().to_string());
    }

    serde_json::Value::String(package.to_string())
}

fn profile_package_settings_value(profile_dir: &Path, packages: &[String]) -> serde_json::Value {
    serde_json::Value::Array(
        packages
            .iter()
            .map(|package| profile_package_setting_entry(profile_dir, package))
            .collect(),
    )
}

fn active_profile_packages(profile_dir: &Path, profile_packages: &[String]) -> Vec<String> {
    let mut active_packages = Vec::new();
    let mut active_package_set = HashSet::new();

    for source in shared_global_package_sources_from_profile_dir(profile_dir) {
        if active_package_set.insert(source.clone()) {
            active_packages.push(source);
        }
    }

    for package in profile_packages {
        let Some(source) = package_entry_source(package) else {
            continue;
        };

        if active_package_set.insert(source) {
            active_packages.push(package.clone());
        }
    }

    active_packages
}
fn write_profile_settings(profile_dir: &Path, profile: &PiProfile) -> Result<(), String> {
    let mut settings = profile.extra_settings.clone();
    remove_generated_profile_settings(&mut settings);

    if let Some(default_provider) = profile.default_provider.as_ref() {
        settings.insert(
            "defaultProvider".to_string(),
            default_provider.clone().into(),
        );
    }
    if let Some(default_model) = profile.default_model.as_ref() {
        settings.insert("defaultModel".to_string(), default_model.clone().into());
    }
    if let Some(default_thinking_level) = profile.default_thinking_level.as_ref() {
        settings.insert(
            "defaultThinkingLevel".to_string(),
            default_thinking_level.clone().into(),
        );
    }
    if let Some(hide_thinking_block) = profile.hide_thinking_block {
        settings.insert("hideThinkingBlock".to_string(), hide_thinking_block.into());
    }
    if let Some(theme) = profile.theme.as_ref() {
        settings.insert("theme".to_string(), theme.clone().into());
    }
    if let Some(quiet_startup) = profile.quiet_startup {
        settings.insert("quietStartup".to_string(), quiet_startup.into());
    }
    if let Some(collapse_changelog) = profile.collapse_changelog {
        settings.insert("collapseChangelog".to_string(), collapse_changelog.into());
    }
    if let Some(enable_install_telemetry) = profile.enable_install_telemetry {
        settings.insert(
            "enableInstallTelemetry".to_string(),
            enable_install_telemetry.into(),
        );
    }
    if let Some(double_escape_action) = profile.double_escape_action.as_ref() {
        settings.insert(
            "doubleEscapeAction".to_string(),
            double_escape_action.clone().into(),
        );
    }
    if let Some(tree_filter_mode) = profile.tree_filter_mode.as_ref() {
        settings.insert(
            "treeFilterMode".to_string(),
            tree_filter_mode.clone().into(),
        );
    }
    if let Some(steering_mode) = profile.steering_mode.as_ref() {
        settings.insert("steeringMode".to_string(), steering_mode.clone().into());
    }
    if let Some(follow_up_mode) = profile.follow_up_mode.as_ref() {
        settings.insert("followUpMode".to_string(), follow_up_mode.clone().into());
    }
    if let Some(transport) = profile.transport.as_ref() {
        settings.insert("transport".to_string(), transport.clone().into());
    }
    if let Some(show_hardware_cursor) = profile.show_hardware_cursor {
        settings.insert(
            "showHardwareCursor".to_string(),
            show_hardware_cursor.into(),
        );
    }
    if let Some(enable_skill_commands) = profile.enable_skill_commands {
        settings.insert(
            "enableSkillCommands".to_string(),
            enable_skill_commands.into(),
        );
    }
    settings.insert(
        "packages".to_string(),
        profile_package_settings_value(
            profile_dir,
            &active_profile_packages(profile_dir, &profile.packages),
        ),
    );
    settings.insert(
        "skills".to_string(),
        serde_json::to_value(&profile.skills)
            .map_err(|error| format!("unable to serialize profile skills: {error}"))?,
    );
    settings.insert(
        "extensions".to_string(),
        serde_json::to_value(&profile.extensions)
            .map_err(|error| format!("unable to serialize profile extensions: {error}"))?,
    );
    settings.insert(
        "prompts".to_string(),
        serde_json::to_value(&profile.prompts)
            .map_err(|error| format!("unable to serialize profile prompts: {error}"))?,
    );
    settings.insert(
        "themes".to_string(),
        serde_json::to_value(&profile.themes)
            .map_err(|error| format!("unable to serialize profile themes: {error}"))?,
    );

    let settings_json = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("unable to serialize profile settings: {error}"))?;
    fs::write(
        profile_dir.join(PROFILE_SETTINGS_FILE),
        format!("{settings_json}\n"),
    )
    .map_err(|error| format!("unable to write profile settings: {error}"))
}

fn write_profile_meta(profile_dir: &Path, profile: &PiProfile) -> Result<(), String> {
    let profile_json = serde_json::to_string_pretty(profile)
        .map_err(|error| format!("unable to serialize profile metadata: {error}"))?;
    fs::write(
        profile_dir.join(PROFILE_META_FILE),
        format!("{profile_json}\n"),
    )
    .map_err(|error| format!("unable to write profile metadata: {error}"))
}

fn copy_if_changed(from: &Path, to: &Path) -> Result<bool, String> {
    let source =
        fs::read(from).map_err(|error| format!("unable to read {}: {error}", from.display()))?;
    let changed = match fs::read(to) {
        Ok(existing) => existing != source,
        Err(_) => true,
    };

    if changed {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("unable to create {}: {error}", parent.display()))?;
        }
        fs::write(to, source)
            .map_err(|error| format!("unable to write {}: {error}", to.display()))?;
    }

    Ok(changed)
}

fn write_bytes_if_changed_atomic(path: &Path, contents: &[u8]) -> Result<bool, String> {
    let changed = match fs::read(path) {
        Ok(existing) => existing != contents,
        Err(_) => true,
    };

    if !changed {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("unable to create {}: {error}", parent.display()))?;
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("auth.json");
    let temp_path = path.with_file_name(format!("{file_name}.tmp-{}", now_millis_string()));
    fs::write(&temp_path, contents)
        .map_err(|error| format!("unable to write {}: {error}", temp_path.display()))?;

    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("unable to replace {}: {error}", path.display()))?;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!(
            "unable to replace {} with {}: {error}",
            path.display(),
            temp_path.display()
        )
    })?;

    Ok(true)
}

fn write_auth_object_if_changed(
    path: &Path,
    auth: &serde_json::Map<String, serde_json::Value>,
) -> Result<bool, String> {
    let auth_json = serde_json::to_string_pretty(&serde_json::Value::Object(auth.clone()))
        .map_err(|error| format!("unable to serialize auth file: {error}"))?;
    write_bytes_if_changed_atomic(path, format!("{auth_json}\n").as_bytes())
}

fn read_auth_object(
    path: &Path,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    match serde_json::from_str::<serde_json::Value>(trimmed)
        .map_err(|error| format!("unable to parse auth file {}: {error}", path.display()))?
    {
        serde_json::Value::Object(object) if object.is_empty() => Ok(None),
        serde_json::Value::Object(object) => Ok(Some(object)),
        _ => Err(format!(
            "auth file {} must contain a JSON object",
            path.display()
        )),
    }
}

fn auth_file_modified(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn merge_json_objects(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Map<String, serde_json::Value>,
) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (
                Some(serde_json::Value::Object(target_object)),
                serde_json::Value::Object(source_object),
            ) => {
                merge_json_objects(target_object, source_object);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn merged_auth_from_candidates(
    candidates: &[(PathBuf, SystemTime)],
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut ordered = candidates.to_vec();
    ordered.sort_by(|(left_path, left_time), (right_path, right_time)| {
        left_time
            .cmp(right_time)
            .then_with(|| left_path.cmp(right_path))
    });

    let mut merged = serde_json::Map::new();
    for (path, _) in ordered {
        if let Some(auth) = read_auth_object(&path)? {
            merge_json_objects(&mut merged, &auth);
        }
    }

    Ok(merged)
}

fn auth_sync_lock_path(shared_auth: &Path) -> PathBuf {
    shared_auth
        .parent()
        .map(|parent| parent.join(AUTH_LOCK_FILE))
        .unwrap_or_else(|| PathBuf::from(AUTH_LOCK_FILE))
}

fn with_auth_sync_lock<T>(
    shared_auth: &Path,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock_path = auth_sync_lock_path(shared_auth);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("unable to create auth lock directory: {error}"))?;
    }

    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("unable to open auth lock {}: {error}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .map_err(|error| format!("unable to lock auth sync {}: {error}", lock_path.display()))?;

    let result = action();
    if let Err(error) = lock_file.unlock() {
        if result.is_ok() {
            return Err(format!(
                "unable to unlock auth sync {}: {error}",
                lock_path.display()
            ));
        }
    }

    result
}

fn idle_profile_dirs(app: &AppHandle, state: &PtyState) -> Result<Vec<PathBuf>, String> {
    let root = profiles_root(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut profile_dirs = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|error| format!("unable to list profiles for auth sync: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("unable to read profile directory entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(profile_id) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_valid_profile_id(profile_id) || is_profile_running(state, profile_id) {
            continue;
        }

        profile_dirs.push(path);
    }

    Ok(profile_dirs)
}

fn sync_auth_to_profile_dirs(
    shared_auth: &Path,
    profile_dirs: &[PathBuf],
    auth: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    write_auth_object_if_changed(shared_auth, auth)?;
    for profile_dir in profile_dirs {
        write_auth_object_if_changed(&profile_dir.join(PROFILE_AUTH_FILE), auth)?;
    }

    Ok(())
}

fn sync_auth_from_newest_available(app: &AppHandle, state: &PtyState) -> Result<(), String> {
    let shared_auth = ensure_shared_auth(app)?;
    with_auth_sync_lock(&shared_auth, || {
        let profile_dirs = idle_profile_dirs(app, state)?;
        let mut candidates = vec![(shared_auth.clone(), auth_file_modified(&shared_auth))];

        for profile_dir in &profile_dirs {
            let auth_path = profile_dir.join(PROFILE_AUTH_FILE);
            if auth_path.exists() {
                candidates.push((auth_path.clone(), auth_file_modified(&auth_path)));
            }
        }

        let merged_auth = merged_auth_from_candidates(&candidates)?;
        sync_auth_to_profile_dirs(&shared_auth, &profile_dirs, &merged_auth)
    })
}

fn save_profile_files(app: &AppHandle, profile: PiProfile) -> Result<PiProfile, String> {
    let profile = normalize_profile(profile)?;
    let shared_auth = ensure_shared_auth(app)?;
    let dir = profile_dir(app, &profile.id)?;

    fs::create_dir_all(&dir)
        .map_err(|error| format!("unable to create profile directory: {error}"))?;
    ensure_profile_subdirectories(&dir)?;

    let profile_auth = dir.join(PROFILE_AUTH_FILE);
    if !profile_auth.exists() {
        copy_if_changed(&shared_auth, &profile_auth)?;
    }

    write_profile_settings(&dir, &profile)?;
    write_profile_meta(&dir, &profile)?;

    Ok(profile)
}

fn read_profile_meta(path: &Path) -> Result<PiProfile, String> {
    let profile_json = fs::read_to_string(path).map_err(|error| {
        format!(
            "unable to read profile metadata {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_str(&profile_json).map_err(|error| {
        format!(
            "unable to parse profile metadata {}: {error}",
            path.display()
        )
    })
}

fn read_profile_settings(
    profile_dir: &Path,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let settings_path = profile_dir.join(PROFILE_SETTINGS_FILE);
    if !settings_path.exists() {
        return Ok(None);
    }

    let settings_json = fs::read_to_string(&settings_path).map_err(|error| {
        format!(
            "unable to read profile settings {}: {error}",
            settings_path.display()
        )
    })?;
    let trimmed = settings_json.trim();
    if trimmed.is_empty() {
        return Ok(Some(serde_json::Map::new()));
    }

    match serde_json::from_str::<serde_json::Value>(trimmed).map_err(|error| {
        format!(
            "unable to parse profile settings {}: {error}",
            settings_path.display()
        )
    })? {
        serde_json::Value::Object(settings) => Ok(Some(settings)),
        _ => Err(format!(
            "profile settings {} must be a JSON object",
            settings_path.display()
        )),
    }
}

fn settings_package_sources(settings: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    settings
        .get("packages")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| match value {
                    serde_json::Value::String(value) => Some(value.trim().to_string()),
                    serde_json::Value::Object(object) => object
                        .get("source")
                        .and_then(|source| source.as_str())
                        .map(|source| source.trim().to_string()),
                    _ => None,
                })
                .filter(|value| !value.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn settings_global_package_sources(
    settings: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    settings
        .get(GLOBAL_PACKAGES_SETTINGS_KEY)
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .filter_map(|value| package_entry_source(&value))
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_package_source_list(values: Vec<String>) -> Vec<String> {
    let mut normalized_sources = Vec::new();
    let mut source_set = HashSet::new();

    for value in values {
        let Some(source) = package_entry_source(&value) else {
            continue;
        };

        if source_set.insert(source.clone()) {
            normalized_sources.push(source);
        }
    }

    normalized_sources
}

fn shared_addons_dir_from_profile_dir(profile_dir: &Path) -> Option<PathBuf> {
    Some(profile_dir.parent()?.parent()?.join(SHARED_ADDONS_DIR_NAME))
}

fn shared_global_package_sources_from_profile_dir(profile_dir: &Path) -> Vec<String> {
    shared_addons_dir_from_profile_dir(profile_dir)
        .and_then(|shared_addons_dir| read_profile_settings(&shared_addons_dir).ok().flatten())
        .map(|settings| {
            let shared_source_set =
                normalize_package_source_list(settings_package_sources(&settings))
                    .into_iter()
                    .collect::<HashSet<_>>();

            normalize_package_source_list(settings_global_package_sources(&settings))
                .into_iter()
                .filter(|source| shared_source_set.contains(source))
                .collect()
        })
        .unwrap_or_default()
}

fn is_global_package_entry_for_profile(
    profile_dir: &Path,
    package_entry: &str,
    global_source: &str,
) -> bool {
    let Some(source) = package_entry_source(package_entry) else {
        return false;
    };

    if source == global_source {
        return true;
    }

    let Some(shared_path) =
        shared_installed_addon_path_from_profile_dir(profile_dir, global_source)
    else {
        return false;
    };

    let source_path = PathBuf::from(&source);
    source_path == shared_path || source == shared_path.to_string_lossy()
}

fn profile_package_sources_from_settings(
    profile_dir: &Path,
    settings: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    let global_sources = shared_global_package_sources_from_profile_dir(profile_dir);
    settings_string_list(settings, "packages")
        .unwrap_or_default()
        .into_iter()
        .filter(|package_entry| {
            !global_sources.iter().any(|global_source| {
                is_global_package_entry_for_profile(profile_dir, package_entry, global_source)
            })
        })
        .collect()
}
fn write_settings_map(
    settings_dir: &Path,
    settings: &serde_json::Map<String, serde_json::Value>,
    context: &str,
) -> Result<(), String> {
    fs::create_dir_all(settings_dir)
        .map_err(|error| format!("unable to create {context} settings directory: {error}"))?;
    let settings_json = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("unable to serialize {context} settings: {error}"))?;
    fs::write(
        settings_dir.join(PROFILE_SETTINGS_FILE),
        format!("{settings_json}\n"),
    )
    .map_err(|error| format!("unable to write {context} settings: {error}"))
}

fn package_entry_source(package_entry: &str) -> Option<String> {
    let package_entry = package_entry.trim();
    if package_entry.is_empty() {
        return None;
    }

    if package_entry.starts_with('{') {
        if let Ok(serde_json::Value::Object(object)) =
            serde_json::from_str::<serde_json::Value>(package_entry)
        {
            return object
                .get("source")
                .and_then(|source| source.as_str())
                .and_then(|source| normalize_package_source_input(Some(source.to_string())));
        }
    }

    normalize_package_source_input(Some(package_entry.to_string()))
}

fn package_json_name(package_path: &Path) -> Option<String> {
    fs::read_to_string(package_path.join("package.json"))
        .ok()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
        .and_then(|package_json| package_json.get("name").cloned())
        .and_then(|name| name.as_str().map(str::to_string))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn path_components_to_posix(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    (!parts.is_empty()).then(|| parts.join("/"))
}

fn git_origin_url(package_path: &Path) -> Option<String> {
    let config = fs::read_to_string(package_path.join(".git").join("config")).ok()?;
    let mut in_origin = false;

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed == "[remote \"origin\"]";
            continue;
        }

        if in_origin {
            if let Some(url) = trimmed.strip_prefix("url =") {
                let url = url.trim();
                if !url.is_empty() {
                    return Some(if url.starts_with("git@") {
                        format!("git:{url}")
                    } else {
                        url.to_string()
                    });
                }
            }
        }
    }

    None
}

fn source_from_profile_installed_path(profile_dir: &Path, source: &str) -> Option<String> {
    let source_path = PathBuf::from(source);
    if !source_path.is_absolute() {
        return None;
    }

    let npm_root = profile_dir.join("npm").join("node_modules");
    if source_path.starts_with(&npm_root) {
        return package_json_name(&source_path).map(|name| format!("npm:{name}"));
    }

    let git_root = profile_dir.join("git");
    if source_path.starts_with(&git_root) {
        if let Some(origin_url) = git_origin_url(&source_path) {
            return Some(origin_url);
        }

        let relative_path = source_path.strip_prefix(&git_root).ok()?;
        let mut components = relative_path.components();
        let host = components.next()?.as_os_str().to_str()?;
        let package_path = path_components_to_posix(components.as_path())?;
        return Some(format!("git:https://{host}/{package_path}"));
    }

    None
}

fn safe_shared_source_segment(value: &str) -> String {
    let segment = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if segment.is_empty() {
        "package".to_string()
    } else {
        segment
    }
}

fn copy_path_recursive(from: &Path, to: &Path) -> Result<bool, String> {
    if !from.exists() {
        return Ok(false);
    }

    if fs::canonicalize(from).ok() == fs::canonicalize(to).ok() {
        return Ok(false);
    }

    if from.is_file() {
        return copy_if_changed(from, to);
    }

    if !from.is_dir() {
        return Ok(false);
    }

    fs::create_dir_all(to)
        .map_err(|error| format!("unable to create {}: {error}", to.display()))?;
    let mut changed = false;
    for entry in
        fs::read_dir(from).map_err(|error| format!("unable to list {}: {error}", from.display()))?
    {
        let entry =
            entry.map_err(|error| format!("unable to read {} entry: {error}", from.display()))?;
        let child_from = entry.path();
        let child_to = to.join(entry.file_name());
        changed |= copy_path_recursive(&child_from, &child_to)?;
    }

    Ok(changed)
}

fn migrate_local_profile_package(
    profile_id: &str,
    profile_dir: &Path,
    shared_addons_dir: &Path,
    source: &str,
) -> Result<Option<(String, bool)>, String> {
    if parse_npm_package_name(source).is_some() || parse_git_package_path(source).is_some() {
        return Ok(None);
    }

    let source_path = PathBuf::from(source);
    let resolved_source = if source_path.is_absolute() {
        source_path
    } else {
        profile_dir.join(&source_path)
    };

    if !resolved_source.exists() {
        return Ok(None);
    }

    if resolved_source.starts_with(profile_dir) {
        let source_segment = safe_shared_source_segment(source);
        let shared_source = format!("local/{profile_id}/{source_segment}");
        let target_path = shared_addons_dir
            .join("local")
            .join(profile_id)
            .join(&source_segment);
        let copied = copy_path_recursive(&resolved_source, &target_path)?;
        return Ok(Some((shared_source, copied)));
    }

    Ok(Some((source.to_string(), false)))
}

fn migrate_profile_package_entry(
    profile_id: &str,
    profile_dir: &Path,
    shared_addons_dir: &Path,
    package_entry: &str,
) -> Result<Option<(String, bool)>, String> {
    let Some(source) = package_entry_source(package_entry) else {
        return Ok(None);
    };
    let source = source_from_profile_installed_path(profile_dir, &source)
        .or_else(|| source_from_profile_installed_path(shared_addons_dir, &source))
        .unwrap_or(source);

    if let Some((shared_source, copied)) =
        migrate_local_profile_package(profile_id, profile_dir, shared_addons_dir, &source)?
    {
        return Ok(Some((shared_source, copied)));
    }

    let source_path = installed_addon_package_path(profile_dir, &source);
    let target_path = expected_managed_addon_package_path(shared_addons_dir, &source);
    let copied = match (source_path, target_path) {
        (Some(_source_path), Some(target_path)) if parse_npm_package_name(&source).is_some() => {
            let source_root = profile_dir.join("npm");
            let target_root = shared_addons_dir.join("npm");
            if !source_root.exists() {
                false
            } else if !target_root.join("package.json").exists() {
                copy_path_recursive(&source_root, &target_root)?
            } else if !target_path.exists() {
                copy_path_recursive(
                    &source_root.join("node_modules"),
                    &target_root.join("node_modules"),
                )?
            } else {
                false
            }
        }
        (Some(source_path), Some(target_path)) if !target_path.exists() => {
            copy_path_recursive(&source_path, &target_path)?
        }
        _ => false,
    };

    Ok(Some((source, copied)))
}

fn migrate_profile_packages_to_shared_addons(
    app: &AppHandle,
) -> Result<PiAddonsMigrationResult, String> {
    ensure_default_profile(app)?;

    let profiles_root = profiles_root(app)?;
    fs::create_dir_all(&profiles_root)
        .map_err(|error| format!("unable to create profiles directory: {error}"))?;

    let shared_addons_dir = shared_addons_dir(app)?;
    fs::create_dir_all(&shared_addons_dir)
        .map_err(|error| format!("unable to create shared Pi add-ons directory: {error}"))?;
    fs::create_dir_all(shared_addons_dir.join(PROFILE_SESSIONS_DIR)).map_err(|error| {
        format!("unable to create shared Pi add-ons session directory: {error}")
    })?;

    let mut shared_settings = read_profile_settings(&shared_addons_dir)?.unwrap_or_default();
    let raw_shared_sources = normalize_list(settings_package_sources(&shared_settings));
    let mut shared_sources = normalize_list(
        raw_shared_sources
            .iter()
            .filter_map(|source| package_entry_source(source))
            .collect(),
    );
    let mut shared_source_set = shared_sources.iter().cloned().collect::<HashSet<_>>();
    let mut shared_settings_changed = shared_sources != raw_shared_sources;
    let mut migrated_profile_ids = HashSet::new();
    let mut migrated_packages = 0;
    let mut copied_packages = 0;

    for entry in fs::read_dir(&profiles_root)
        .map_err(|error| format!("unable to list profiles for add-on migration: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("unable to read profile directory entry: {error}"))?;
        let profile_dir = entry.path();
        if !profile_dir.is_dir() {
            continue;
        }

        let Some(profile_id) = profile_dir.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_valid_profile_id(profile_id) || !profile_dir.join(PROFILE_META_FILE).exists() {
            continue;
        }

        let mut profile = read_profile_files(&profile_dir)?;
        if profile.packages.is_empty() {
            continue;
        }

        let original_packages = profile.packages.clone();
        let mut next_packages = Vec::new();
        let mut next_package_set = HashSet::new();
        let mut profile_changed = false;
        let mut profile_settings_needs_write = false;
        let mut profile_migrated = false;

        for package_entry in original_packages.iter() {
            match migrate_profile_package_entry(
                profile_id,
                &profile_dir,
                &shared_addons_dir,
                package_entry,
            )? {
                Some((shared_source, copied)) => {
                    profile_migrated = true;
                    copied_packages += usize::from(copied);
                    let source_changed = package_entry_source(package_entry)
                        .map(|source| source != shared_source)
                        .unwrap_or(true);

                    if next_package_set.insert(shared_source.clone()) {
                        next_packages.push(shared_source.clone());
                    }

                    let shared_source_added = if shared_source_set.insert(shared_source.clone()) {
                        shared_sources.push(shared_source.clone());
                        shared_settings_changed = true;
                        migrated_packages += 1;
                        true
                    } else {
                        false
                    };

                    profile_settings_needs_write |= copied || source_changed || shared_source_added;
                }
                None => {
                    let package_entry = package_entry.trim().to_string();
                    if !package_entry.is_empty() && next_package_set.insert(package_entry.clone()) {
                        next_packages.push(package_entry);
                    }
                }
            }
        }

        if profile.packages != next_packages {
            profile.packages = next_packages;
            profile.updated_at = now_string();
            write_profile_meta(&profile_dir, &profile)?;
            write_profile_settings(&profile_dir, &profile)?;
            profile_changed = true;
        } else if profile_settings_needs_write {
            write_profile_settings(&profile_dir, &profile)?;
        }

        if profile_migrated && (profile_changed || profile_settings_needs_write) {
            migrated_profile_ids.insert(profile_id.to_string());
        }
    }

    if shared_settings_changed {
        shared_sources.sort_by_key(|source| source.to_lowercase());
        shared_settings.insert(
            "packages".to_string(),
            serde_json::to_value(&shared_sources)
                .map_err(|error| format!("unable to serialize shared add-on packages: {error}"))?,
        );
        write_settings_map(&shared_addons_dir, &shared_settings, "shared Pi add-ons")?;
    }

    Ok(PiAddonsMigrationResult {
        migrated_profiles: migrated_profile_ids.len(),
        migrated_packages,
        copied_packages,
    })
}

fn package_resource_types(package_path: &Path) -> Vec<String> {
    let mut resource_types = Vec::new();
    let package_json_path = package_path.join("package.json");
    let pi_manifest = fs::read_to_string(&package_json_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
        .and_then(|package_json| package_json.get("pi").cloned());

    for resource_type in ["extensions", "skills", "prompts", "themes"] {
        let manifest_has_type = pi_manifest
            .as_ref()
            .and_then(|manifest| manifest.get(resource_type))
            .and_then(|value| value.as_array())
            .map(|entries| !entries.is_empty())
            .unwrap_or(false);
        let conventional_dir_exists = package_path.join(resource_type).exists();

        if manifest_has_type || conventional_dir_exists {
            resource_types.push(resource_type.to_string());
        }
    }

    resource_types
}

fn list_shared_addon_packages(app: &AppHandle) -> Result<Vec<PiAddonPackage>, String> {
    let shared_addons_dir = shared_addons_dir(app)?;
    fs::create_dir_all(&shared_addons_dir)
        .map_err(|error| format!("unable to create shared Pi add-ons directory: {error}"))?;

    let settings = read_profile_settings(&shared_addons_dir)?.unwrap_or_default();
    let global_source_set =
        normalize_package_source_list(settings_global_package_sources(&settings))
            .into_iter()
            .collect::<HashSet<_>>();
    let mut packages = normalize_package_source_list(settings_package_sources(&settings))
        .into_iter()
        .map(|source| {
            let installed_path = installed_addon_package_path(&shared_addons_dir, &source);
            let resource_types = installed_path
                .as_deref()
                .map(package_resource_types)
                .unwrap_or_default();
            let global = global_source_set.contains(&source);
            PiAddonPackage {
                source,
                installed_path: installed_path.map(|path| path.to_string_lossy().to_string()),
                resource_types,
                global,
            }
        })
        .collect::<Vec<_>>();

    packages.sort_by_key(|package| package.source.to_lowercase());
    Ok(packages)
}

fn rewrite_profile_settings_for_all_profiles(app: &AppHandle) -> Result<(), String> {
    for profile in list_profiles_from_disk(app)? {
        let dir = profile_dir(app, &profile.id)?;
        write_profile_settings(&dir, &profile)?;
    }

    Ok(())
}
fn settings_string(
    settings: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    settings
        .get(key)?
        .as_str()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn settings_bool(settings: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<bool> {
    settings.get(key)?.as_bool()
}

fn extract_profile_extra_settings(
    settings: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut extra_settings = settings.clone();
    remove_generated_profile_settings(&mut extra_settings);
    extra_settings
}

fn settings_string_list(
    settings: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<Vec<String>> {
    let values = settings.get(key)?.as_array()?;
    Some(
        values
            .iter()
            .filter_map(|value| match value {
                serde_json::Value::String(value) => Some(value.trim().to_string()),
                serde_json::Value::Object(_) if key == "packages" => Some(value.to_string()),
                _ => None,
            })
            .filter(|value| !value.is_empty())
            .collect(),
    )
}

fn apply_settings_to_profile(
    mut profile: PiProfile,
    settings: &serde_json::Map<String, serde_json::Value>,
    profile_dir: &Path,
) -> PiProfile {
    profile.extra_settings = extract_profile_extra_settings(settings);

    if let Some(default_provider) = settings_string(settings, "defaultProvider") {
        profile.default_provider = Some(default_provider);
    }
    if let Some(default_model) = settings_string(settings, "defaultModel") {
        profile.default_model = Some(default_model);
    }
    if let Some(default_thinking_level) = settings_string(settings, "defaultThinkingLevel") {
        profile.default_thinking_level = Some(default_thinking_level);
    }
    if let Some(hide_thinking_block) = settings_bool(settings, "hideThinkingBlock") {
        profile.hide_thinking_block = Some(hide_thinking_block);
    }
    if let Some(theme) = settings_string(settings, "theme") {
        profile.theme = Some(theme);
    }
    if let Some(quiet_startup) = settings_bool(settings, "quietStartup") {
        profile.quiet_startup = Some(quiet_startup);
    }
    if let Some(collapse_changelog) = settings_bool(settings, "collapseChangelog") {
        profile.collapse_changelog = Some(collapse_changelog);
    }
    if let Some(enable_install_telemetry) = settings_bool(settings, "enableInstallTelemetry") {
        profile.enable_install_telemetry = Some(enable_install_telemetry);
    }
    if let Some(double_escape_action) = settings_string(settings, "doubleEscapeAction") {
        profile.double_escape_action = Some(double_escape_action);
    }
    if let Some(tree_filter_mode) = settings_string(settings, "treeFilterMode") {
        profile.tree_filter_mode = Some(tree_filter_mode);
    }
    if let Some(steering_mode) = settings_string(settings, "steeringMode") {
        profile.steering_mode = Some(steering_mode);
    }
    if let Some(follow_up_mode) = settings_string(settings, "followUpMode") {
        profile.follow_up_mode = Some(follow_up_mode);
    }
    if let Some(transport) = settings_string(settings, "transport") {
        profile.transport = Some(transport);
    }
    if let Some(show_hardware_cursor) = settings_bool(settings, "showHardwareCursor") {
        profile.show_hardware_cursor = Some(show_hardware_cursor);
    }
    if let Some(enable_skill_commands) = settings_bool(settings, "enableSkillCommands") {
        profile.enable_skill_commands = Some(enable_skill_commands);
    }
    if profile.packages.is_empty() {
        let packages = profile_package_sources_from_settings(profile_dir, settings);
        if !packages.is_empty() {
            profile.packages = packages;
        }
    }
    if let Some(skills) = settings_string_list(settings, "skills") {
        profile.skills = skills;
    }
    if let Some(extensions) = settings_string_list(settings, "extensions") {
        profile.extensions = extensions;
    }
    if let Some(prompts) = settings_string_list(settings, "prompts") {
        profile.prompts = prompts;
    }
    if let Some(themes) = settings_string_list(settings, "themes") {
        profile.themes = themes;
    }

    profile
}

fn read_profile_files(profile_dir: &Path) -> Result<PiProfile, String> {
    let mut profile = normalize_profile(read_profile_meta(&profile_dir.join(PROFILE_META_FILE))?)?;

    if let Some(settings) = read_profile_settings(profile_dir)? {
        profile = apply_settings_to_profile(profile, &settings, profile_dir);
    }

    normalize_profile(profile)
}

fn ensure_default_profile(app: &AppHandle) -> Result<(), String> {
    let default_dir = profile_dir(app, DEFAULT_PROFILE_ID)?;
    if !default_dir.join(PROFILE_META_FILE).exists() {
        save_profile_files(app, default_profile())?;
    }

    Ok(())
}

fn list_profiles_from_disk(app: &AppHandle) -> Result<Vec<PiProfile>, String> {
    ensure_default_profile(app)?;
    let root = profiles_root(app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("unable to create profiles directory: {error}"))?;

    let mut profiles = Vec::new();
    let entries =
        fs::read_dir(&root).map_err(|error| format!("unable to list profiles: {error}"))?;

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("unable to read profile directory entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(profile_id) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_valid_profile_id(profile_id) {
            continue;
        }

        let meta_path = path.join(PROFILE_META_FILE);
        if !meta_path.exists() {
            continue;
        }

        let profile = read_profile_files(&path)?;
        profiles.push(profile);
    }

    profiles.sort_by_key(|profile| profile.name.to_lowercase());
    Ok(profiles)
}

fn mark_profile_running(state: &PtyState, profile_id: &str) -> Result<(), String> {
    let mut running = state
        .running_pi_profiles
        .lock()
        .map_err(|_| "profile runtime state is unavailable".to_string())?;
    *running.entry(profile_id.to_string()).or_insert(0) += 1;
    Ok(())
}

fn mark_profile_exited(state: &PtyState, profile_id: &str) {
    if let Ok(mut running) = state.running_pi_profiles.lock() {
        if let Some(count) = running.get_mut(profile_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                running.remove(profile_id);
            }
        }
    }
}

fn is_profile_running(state: &PtyState, profile_id: &str) -> bool {
    state
        .running_pi_profiles
        .lock()
        .map(|running| running.get(profile_id).copied().unwrap_or(0) > 0)
        .unwrap_or(false)
}

fn prepare_pi_profile_for_launch(
    app: &AppHandle,
    state: &PtyState,
    requested_profile_id: Option<&str>,
) -> Result<PiLaunchProfile, String> {
    ensure_default_profile(app)?;

    let profile_id = requested_profile_id
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROFILE_ID);
    validate_profile_id(profile_id)?;

    let dir = profile_dir(app, profile_id)?;
    let meta_path = dir.join(PROFILE_META_FILE);
    if !meta_path.exists() {
        return Err(format!("Pi profile '{profile_id}' does not exist"));
    }

    let shared_auth = ensure_shared_auth(app)?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("unable to create profile directory: {error}"))?;
    ensure_profile_subdirectories(&dir)?;
    let profile_auth = dir.join(PROFILE_AUTH_FILE);
    if !profile_auth.exists() {
        copy_if_changed(&shared_auth, &profile_auth)?;
    }

    sync_auth_from_newest_available(app, state)?;
    let profile = read_profile_files(&dir)?;
    write_profile_settings(&dir, &profile)?;

    Ok(PiLaunchProfile {
        id: profile_id.to_string(),
        profile_dir: dir.clone(),
        session_dir: dir.join(PROFILE_SESSIONS_DIR),
    })
}

fn sync_profile_auth_after_exit(
    app: &AppHandle,
    state: &PtyState,
    profile_id: &str,
) -> Result<(), String> {
    mark_profile_exited(state, profile_id);

    let shared_auth = ensure_shared_auth(app)?;
    let launched_profile_auth = profile_dir(app, profile_id)?.join(PROFILE_AUTH_FILE);

    with_auth_sync_lock(&shared_auth, || {
        let mut candidates = vec![(shared_auth.clone(), auth_file_modified(&shared_auth))];
        if launched_profile_auth.exists() {
            candidates.push((
                launched_profile_auth.clone(),
                auth_file_modified(&launched_profile_auth),
            ));
        }

        let merged_auth = merged_auth_from_candidates(&candidates)?;
        let profile_dirs = idle_profile_dirs(app, state)?;
        sync_auth_to_profile_dirs(&shared_auth, &profile_dirs, &merged_auth)
    })
}

fn configure_terminal_command(command: &mut CommandBuilder, working_directory: Option<&str>) {
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");

    if let Some(cwd) = working_directory {
        command.cwd(PathBuf::from(cwd));
    } else if let Ok(cwd) = std::env::current_dir() {
        command.cwd(cwd);
    }
}

#[cfg(windows)]
fn find_pi_cmd_shim() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;

    for directory in std::env::split_paths(&path) {
        for file_name in ["pi.CMD", "pi.cmd", "pi.BAT", "pi.bat"] {
            let candidate = directory.join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(windows)]
fn parse_pi_cmd_shim(shim_path: &Path) -> Option<(PathBuf, PathBuf, Option<String>)> {
    let shim = fs::read_to_string(shim_path).ok()?;
    let shim_dir = shim_path.parent()?;
    let node_path_value = shim.lines().find_map(|line| {
        line.trim()
            .strip_prefix("@SET \"NODE_PATH=")
            .and_then(|value| value.strip_suffix('"'))
            .map(str::to_string)
    });

    let (node_path, cli_path) = shim.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("@\"")?;
        let node_end = rest.find('"')?;
        let node_path = PathBuf::from(&rest[..node_end]);
        let rest = rest[node_end + 1..].trim_start();
        let cli = rest.strip_prefix('"')?;
        let cli_end = cli.find('"')?;
        let cli = &cli[..cli_end];
        let cli_path = if let Some(relative_cli) = cli.strip_prefix("%~dp0") {
            shim_dir.join(relative_cli.trim_start_matches(['\\', '/']))
        } else {
            PathBuf::from(cli)
        };

        Some((node_path, cli_path))
    })?;

    if !node_path.is_file() || !cli_path.is_file() {
        return None;
    }

    Some((node_path, cli_path, node_path_value))
}

#[cfg(windows)]
fn pi_command_from_cmd_shim(shim_path: &Path) -> Option<CommandBuilder> {
    let (node_path, cli_path, node_path_value) = parse_pi_cmd_shim(shim_path)?;
    let mut command = CommandBuilder::new(node_path.as_os_str());
    command.arg(cli_path.as_os_str());

    if let Some(node_path_value) = node_path_value {
        command.env("NODE_PATH", node_path_value);
    }

    Some(command)
}

#[cfg(windows)]
fn std_pi_command_from_cmd_shim(shim_path: &Path) -> Option<StdCommand> {
    let (node_path, cli_path, node_path_value) = parse_pi_cmd_shim(shim_path)?;
    let mut command = StdCommand::new(node_path);
    command.arg(cli_path);

    if let Some(node_path_value) = node_path_value {
        command.env("NODE_PATH", node_path_value);
    }

    Some(command)
}

#[cfg(windows)]
fn direct_pi_command() -> Option<CommandBuilder> {
    find_pi_cmd_shim().and_then(|shim_path| pi_command_from_cmd_shim(&shim_path))
}

fn pi_process_command() -> StdCommand {
    #[cfg(windows)]
    {
        let mut command = find_pi_cmd_shim()
            .and_then(|shim_path| std_pi_command_from_cmd_shim(&shim_path))
            .unwrap_or_else(|| {
                let mut command = StdCommand::new("cmd.exe");
                command.args(["/D", "/C", "pi"]);
                command
            });
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    #[cfg(not(windows))]
    {
        StdCommand::new("pi")
    }
}

fn default_pi_command(working_directory: Option<&str>) -> CommandBuilder {
    #[cfg(windows)]
    let mut command = direct_pi_command().unwrap_or_else(|| {
        let mut command = CommandBuilder::new("cmd.exe");
        command.args(["/D", "/C", "pi"]);
        command
    });

    #[cfg(not(windows))]
    let mut command = {
        let mut command = CommandBuilder::new("sh");
        command.args(["-lc", "exec pi \"$@\"", "pi"]);
        command
    };

    configure_terminal_command(&mut command, working_directory);
    command
}

fn default_shell_command(working_directory: Option<&str>) -> CommandBuilder {
    #[cfg(windows)]
    let mut command = {
        let mut command = CommandBuilder::new("powershell.exe");
        command.args(["-NoLogo"]);
        command
    };

    #[cfg(not(windows))]
    let mut command = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        CommandBuilder::new(shell)
    };

    configure_terminal_command(&mut command, working_directory);
    command
}
fn emit_start_error(app: &AppHandle, id: u64, error: String) {
    if let Ok(mut sessions) = app.state::<PtyState>().sessions.lock() {
        sessions.remove(&id);
    }

    let _ = app.emit("pty:error", PtyErrorPayload { id, error });
    let _ = app.emit("pty:exit", PtyExitPayload { id });
}

#[cfg(windows)]
fn kill_process_tree(process_id: Option<u32>) {
    let Some(pid) = process_id else {
        return;
    };

    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn child_process_ids(pid: u32) -> Vec<u32> {
    let output = StdCommand::new("pgrep")
        .args(["-P", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();

    output
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn collect_process_tree(pid: u32, process_ids: &mut Vec<u32>) {
    if process_ids.contains(&pid) {
        return;
    }

    for child_pid in child_process_ids(pid) {
        collect_process_tree(child_pid, process_ids);
    }
    process_ids.push(pid);
}

#[cfg(not(windows))]
fn signal_process(pid: u32, signal: &str) {
    let _ = StdCommand::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(windows))]
fn kill_process_tree(process_id: Option<u32>) {
    let Some(pid) = process_id else {
        return;
    };

    let mut process_ids = Vec::new();
    collect_process_tree(pid, &mut process_ids);

    for pid in &process_ids {
        signal_process(*pid, "TERM");
    }
    thread::sleep(Duration::from_millis(250));
    for pid in &process_ids {
        signal_process(*pid, "KILL");
    }
}

fn kill_running_session_sync(session: PtySession) {
    if let PtySession::Running {
        master,
        writer,
        mut child,
        ..
    } = session
    {
        let process_id = child.process_id();

        drop(writer);
        drop(master);

        kill_process_tree(process_id);
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn session_pi_profile_id(session: &PtySession) -> Option<String> {
    match session {
        PtySession::Running { pi_profile_id, .. } => pi_profile_id.clone(),
        PtySession::Starting => None,
    }
}

fn kill_running_session_background(app: AppHandle, session: PtySession) {
    let pi_profile_id = session_pi_profile_id(&session);
    thread::spawn(move || {
        kill_running_session_sync(session);
        if let Some(profile_id) = pi_profile_id {
            let _ = sync_profile_auth_after_exit(&app, &app.state::<PtyState>(), &profile_id);
        }
    });
}

fn prepare_shared_addons_for_package_command(app: &AppHandle) -> Result<PiLaunchProfile, String> {
    let dir = shared_addons_dir(app)?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("unable to create shared Pi add-ons directory: {error}"))?;
    fs::create_dir_all(dir.join(PROFILE_SESSIONS_DIR)).map_err(|error| {
        format!("unable to create shared Pi add-ons session directory: {error}")
    })?;

    Ok(PiLaunchProfile {
        id: "shared-addons".to_string(),
        profile_dir: dir.clone(),
        session_dir: dir.join(PROFILE_SESSIONS_DIR),
    })
}
fn pi_command_output_text(output: &PiCommandOutput) -> String {
    [output.stdout.trim(), output.stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_pi_command_failure(args: &[String], output: &PiCommandOutput) -> String {
    let code = output
        .code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mut message = format!("pi {} failed with exit code {code}", args.join(" "));
    let output_text = pi_command_output_text(output);

    if !output_text.is_empty() {
        message.push_str("\n\n");
        message.push_str(&output_text);
    }

    message
}

fn read_pipe_to_end<R>(mut reader: R) -> Vec<u8>
where
    R: Read + Send + 'static,
{
    let mut output = Vec::new();
    let _ = reader.read_to_end(&mut output);
    output
}

fn wait_for_child_output(mut child: StdChild, timeout: Duration) -> Result<(Output, bool), String> {
    let stdout_handle = child
        .stdout
        .take()
        .map(|stdout| thread::spawn(move || read_pipe_to_end(stdout)));
    let stderr_handle = child
        .stderr
        .take()
        .map(|stderr| thread::spawn(move || read_pipe_to_end(stderr)));
    let started_at = Instant::now();

    let (status, timed_out) = loop {
        match child
            .try_wait()
            .map_err(|error| format!("unable to poll Pi package command: {error}"))?
        {
            Some(status) => break (status, false),
            None if started_at.elapsed() >= timeout => {
                kill_process_tree(Some(child.id()));
                let _ = child.kill();
                let status = child.wait().map_err(|error| {
                    format!("unable to wait for timed-out Pi package command: {error}")
                })?;
                break (status, true);
            }
            None => thread::sleep(PI_PACKAGE_COMMAND_POLL_INTERVAL),
        }
    };

    let stdout = stdout_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    Ok((
        Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
    ))
}

fn run_pi_package_command(
    profile: &PiLaunchProfile,
    args: &[String],
) -> Result<PiCommandOutput, String> {
    let mut command = pi_process_command();
    command
        .args(args)
        .current_dir(&profile.profile_dir)
        .env(
            "PI_CODING_AGENT_DIR",
            profile.profile_dir.to_string_lossy().to_string(),
        )
        .env(
            "PI_CODING_AGENT_SESSION_DIR",
            profile.session_dir.to_string_lossy().to_string(),
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command
        .spawn()
        .map_err(|error| format!("unable to run pi {}: {error}", args.join(" ")))?;
    let (output, timed_out) = wait_for_child_output(child, PI_PACKAGE_COMMAND_TIMEOUT)?;
    let result = PiCommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    };

    if timed_out {
        let mut message = format!(
            "pi {} timed out after {} seconds",
            args.join(" "),
            PI_PACKAGE_COMMAND_TIMEOUT.as_secs()
        );
        let output_text = pi_command_output_text(&result);
        if !output_text.is_empty() {
            message.push_str("\n\n");
            message.push_str(&output_text);
        }
        return Err(message);
    }

    if output.status.success() {
        Ok(result)
    } else {
        Err(format_pi_command_failure(args, &result))
    }
}

fn strip_pi_package_command_prefix(value: &str) -> &str {
    let mut rest = value.trim();
    let lower = rest.to_ascii_lowercase();

    if lower == "pi" {
        return "";
    }

    if lower.starts_with("pi ") {
        rest = rest[3..].trim_start();
    }

    let lower = rest.to_ascii_lowercase();
    for command in ["install", "remove", "uninstall", "update", "list"] {
        if lower == command {
            return "";
        }

        let prefix = format!("{command} ");
        if lower.starts_with(&prefix) {
            return rest[prefix.len()..].trim();
        }
    }

    rest
}

fn strip_optional_package_flag(value: &str) -> &str {
    let rest = value.trim();
    let lower = rest.to_ascii_lowercase();

    if lower == "--extensions" {
        return "";
    }

    for flag in ["--extension ", "--extension=", "-l ", "--local "] {
        if lower.starts_with(flag) {
            return rest[flag.len()..].trim();
        }
    }

    rest
}

fn strip_enclosing_quotes(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\"' && bytes[value.len() - 1] == b'\"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }

    value
}

fn normalize_package_source_input(source: Option<String>) -> Option<String> {
    let source = source?;
    let source = strip_pi_package_command_prefix(&source);
    let source = strip_optional_package_flag(source);
    let source = strip_enclosing_quotes(source).trim().to_string();

    if source.is_empty() {
        None
    } else {
        Some(source)
    }
}

#[tauri::command]
fn pi_addons_list(app: AppHandle) -> Result<Vec<PiAddonPackage>, String> {
    migrate_profile_packages_to_shared_addons(&app)?;
    list_shared_addon_packages(&app)
}

#[tauri::command]
async fn pi_addons_set_global_packages(
    app: AppHandle,
    sources: Vec<String>,
) -> Result<Vec<PiAddonPackage>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        migrate_profile_packages_to_shared_addons(&app)?;

        let shared_addons_dir = shared_addons_dir(&app)?;
        fs::create_dir_all(&shared_addons_dir)
            .map_err(|error| format!("unable to create shared Pi add-ons directory: {error}"))?;

        let mut settings = read_profile_settings(&shared_addons_dir)?.unwrap_or_default();
        let shared_sources = normalize_package_source_list(settings_package_sources(&settings));
        let shared_source_set = shared_sources.iter().cloned().collect::<HashSet<_>>();
        let mut unknown_sources = Vec::new();
        let mut global_sources = Vec::new();
        let mut global_source_set = HashSet::new();

        for source in normalize_package_source_list(sources) {
            if !shared_source_set.contains(&source) {
                unknown_sources.push(source);
                continue;
            }

            if global_source_set.insert(source.clone()) {
                global_sources.push(source);
            }
        }

        if !unknown_sources.is_empty() {
            return Err(format!(
                "cannot mark uninstalled Pi add-on(s) as global: {}",
                unknown_sources.join(", ")
            ));
        }

        settings.insert(
            GLOBAL_PACKAGES_SETTINGS_KEY.to_string(),
            serde_json::to_value(&global_sources)
                .map_err(|error| format!("unable to serialize global Pi add-ons: {error}"))?,
        );
        write_settings_map(&shared_addons_dir, &settings, "shared Pi add-ons")?;
        rewrite_profile_settings_for_all_profiles(&app)?;
        list_shared_addon_packages(&app)
    })
    .await
    .map_err(|error| format!("Global Pi add-on settings task failed: {error}"))?
}

#[tauri::command]
async fn pi_addons_package_command(
    app: AppHandle,
    command: String,
    source: Option<String>,
) -> Result<PiCommandOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let command_name = command.trim().to_ascii_lowercase();
        let source = normalize_package_source_input(source);
        let mut args = vec![command_name.clone()];

        match command_name.as_str() {
            "install" => {
                let source = source.ok_or_else(|| "package source is required".to_string())?;
                args.push(source);
            }
            "remove" | "uninstall" => {
                let source = source.ok_or_else(|| "package source is required".to_string())?;
                args[0] = "remove".to_string();
                args.push(source);
            }
            "update" => {
                if let Some(source) = source {
                    args.push("--extension".to_string());
                    args.push(source);
                } else {
                    args.push("--extensions".to_string());
                }
            }
            "list" => {
                if source.is_some() {
                    return Err("pi list does not accept a package source".to_string());
                }
            }
            _ => {
                return Err(
                    "unsupported Pi package command; use install, remove, uninstall, update, or list"
                        .to_string(),
                );
            }
        }

        let shared_addons = prepare_shared_addons_for_package_command(&app)?;
        let output = run_pi_package_command(&shared_addons, &args)?;
        if command_name != "list" {
            rewrite_profile_settings_for_all_profiles(&app)?;
        }

        Ok(output)
    })
    .await
    .map_err(|error| format!("Pi package command task failed: {error}"))?
}

#[tauri::command]
fn pi_profiles_list(app: AppHandle) -> Result<Vec<PiProfile>, String> {
    migrate_profile_packages_to_shared_addons(&app)?;
    list_profiles_from_disk(&app)
}

#[tauri::command]
fn pi_profile_save(app: AppHandle, profile: PiProfile) -> Result<(), String> {
    save_profile_files(&app, profile).map(|_| ())
}

#[tauri::command]
fn pi_profile_delete(
    app: AppHandle,
    state: State<'_, PtyState>,
    profile_id: String,
) -> Result<(), String> {
    let profile_id = profile_id.trim().to_string();
    validate_profile_id(&profile_id)?;

    if profile_id == DEFAULT_PROFILE_ID {
        return Err("the default Pi profile cannot be deleted".to_string());
    }

    if is_profile_running(&state, &profile_id) {
        return Err("cannot delete a Pi profile while it has running terminals".to_string());
    }

    let dir = profile_dir(&app, &profile_id)?;
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .map_err(|error| format!("unable to delete profile directory: {error}"))?;
    }

    Ok(())
}

#[tauri::command]
fn pi_profile_reveal_dir(app: AppHandle, profile_id: String) -> Result<String, String> {
    let profile_id = profile_id.trim().to_string();
    validate_profile_id(&profile_id)?;
    let dir = profile_dir(&app, &profile_id)?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("unable to create profile directory: {error}"))?;
    let path = dir.to_string_lossy().to_string();
    app.opener()
        .open_path(path.clone(), None::<String>)
        .map_err(|error| format!("unable to open profile directory: {error}"))?;
    Ok(path)
}

fn json_string_field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(str::trim)
        .filter(|field| !field.is_empty())
}

#[tauri::command]
fn pioc_control_read(app: AppHandle, id: u64) -> Result<Option<serde_json::Value>, String> {
    let paths = pioc_control_paths(&app, id)?;
    if !paths.telemetry_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&paths.telemetry_path).map_err(|error| {
        format!(
            "unable to read PIOC telemetry {}: {error}",
            paths.telemetry_path.display()
        )
    })?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    serde_json::from_str(trimmed).map(Some).map_err(|error| {
        format!(
            "unable to parse PIOC telemetry {}: {error}",
            paths.telemetry_path.display()
        )
    })
}

#[tauri::command]
fn pioc_control_command(app: AppHandle, id: u64, command: serde_json::Value) -> Result<(), String> {
    let command_type = json_string_field(&command, "type")
        .ok_or_else(|| "PIOC control command requires a type".to_string())?;

    match command_type {
        "set_model" => {
            json_string_field(&command, "provider")
                .ok_or_else(|| "set_model requires provider".to_string())?;
            json_string_field(&command, "modelId")
                .ok_or_else(|| "set_model requires modelId".to_string())?;
        }
        "set_thinking_level" => {
            let level = json_string_field(&command, "level")
                .ok_or_else(|| "set_thinking_level requires level".to_string())?;
            if !matches!(
                level,
                "off" | "minimal" | "low" | "medium" | "high" | "xhigh"
            ) {
                return Err(format!("unsupported thinking level: {level}"));
            }
        }
        "refresh" => {}
        other => return Err(format!("unsupported PIOC control command: {other}")),
    }

    let mut object = command
        .as_object()
        .cloned()
        .ok_or_else(|| "PIOC control command must be a JSON object".to_string())?;
    object.insert(
        "id".to_string(),
        format!("{id}-{}", now_millis_string()).into(),
    );
    object.insert("requestedAt".to_string(), now_millis_string().into());

    let paths = pioc_control_paths(&app, id)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.command_path)
        .map_err(|error| {
            format!(
                "unable to open PIOC command file {}: {error}",
                paths.command_path.display()
            )
        })?;
    let line = serde_json::to_string(&serde_json::Value::Object(object))
        .map_err(|error| format!("unable to serialize PIOC control command: {error}"))?;
    writeln!(file, "{line}").map_err(|error| {
        format!(
            "unable to write PIOC command file {}: {error}",
            paths.command_path.display()
        )
    })
}

#[tauri::command]
fn pty_start(
    app: AppHandle,
    state: State<'_, PtyState>,
    request: PtyStartRequest,
) -> Result<(), String> {
    let PtyStartRequest {
        id,
        cols,
        rows,
        mode,
        working_directory,
        pi_profile_id,
    } = request;
    let kind = match mode.as_deref().unwrap_or("pi") {
        "pi" => PtyKind::Pi,
        "shell" => PtyKind::Shell,
        other => return Err(format!("unsupported terminal mode: {other}")),
    };
    let working_directory = working_directory
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let pi_profile_id = pi_profile_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let Some(profile_id) = pi_profile_id.as_deref() {
        validate_profile_id(profile_id)?;
    }

    if let Some(cwd) = working_directory.as_deref() {
        if !PathBuf::from(cwd).is_dir() {
            return Err(format!("working directory does not exist: {cwd}"));
        }
    }

    {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| "terminal session state is unavailable".to_string())?;

        if sessions.contains_key(&id) {
            return Err(format!("terminal session {id} already exists"));
        }

        sessions.insert(id, PtySession::Starting);
    }

    thread::spawn(move || {
        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(pair) => pair,
            Err(error) => {
                emit_start_error(&app, id, error.to_string());
                return;
            }
        };

        let launch_profile = if matches!(kind, PtyKind::Pi) {
            match prepare_pi_profile_for_launch(
                &app,
                &app.state::<PtyState>(),
                pi_profile_id.as_deref(),
            ) {
                Ok(profile) => {
                    if let Err(error) = mark_profile_running(&app.state::<PtyState>(), &profile.id)
                    {
                        emit_start_error(&app, id, error);
                        return;
                    }
                    Some(profile)
                }
                Err(error) => {
                    emit_start_error(&app, id, error);
                    return;
                }
            }
        } else {
            None
        };

        let mut command = match kind {
            PtyKind::Pi => default_pi_command(working_directory.as_deref()),
            PtyKind::Shell => default_shell_command(working_directory.as_deref()),
        };

        if let Some(profile) = launch_profile.as_ref() {
            command.env(
                "PI_CODING_AGENT_DIR",
                profile.profile_dir.to_string_lossy().to_string(),
            );
            command.env(
                "PI_CODING_AGENT_SESSION_DIR",
                profile.session_dir.to_string_lossy().to_string(),
            );

            let control_paths = match pioc_control_paths(&app, id) {
                Ok(paths) => paths,
                Err(error) => {
                    mark_profile_exited(&app.state::<PtyState>(), &profile.id);
                    emit_start_error(&app, id, error);
                    return;
                }
            };
            let control_extension_path = match ensure_pioc_control_extension(&app) {
                Ok(path) => path,
                Err(error) => {
                    mark_profile_exited(&app.state::<PtyState>(), &profile.id);
                    emit_start_error(&app, id, error);
                    return;
                }
            };

            command.arg("--extension");
            command.arg(control_extension_path.as_os_str());

            let _ = fs::remove_file(&control_paths.telemetry_path);
            let _ = fs::write(&control_paths.command_path, b"");

            command.env("PIOC_TERMINAL_ID", id.to_string());
            command.env("PIOC_PROFILE_ID", profile.id.clone());
            command.env(
                "PIOC_TELEMETRY_PATH",
                control_paths.telemetry_path.to_string_lossy().to_string(),
            );
            command.env(
                "PIOC_COMMAND_PATH",
                control_paths.command_path.to_string_lossy().to_string(),
            );
        }

        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                if let Some(profile) = launch_profile.as_ref() {
                    mark_profile_exited(&app.state::<PtyState>(), &profile.id);
                }
                emit_start_error(&app, id, error.to_string());
                return;
            }
        };
        let mut reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                if let Some(profile) = launch_profile.as_ref() {
                    mark_profile_exited(&app.state::<PtyState>(), &profile.id);
                }
                emit_start_error(&app, id, error.to_string());
                return;
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                if let Some(profile) = launch_profile.as_ref() {
                    mark_profile_exited(&app.state::<PtyState>(), &profile.id);
                }
                emit_start_error(&app, id, error.to_string());
                return;
            }
        };

        let mut session = Some(PtySession::Running {
            master: pair.master,
            writer,
            child,
            pi_profile_id: launch_profile.as_ref().map(|profile| profile.id.clone()),
        });

        let should_continue = match app.state::<PtyState>().sessions.lock() {
            Ok(mut sessions) if sessions.contains_key(&id) => {
                sessions.insert(id, session.take().expect("running session missing"));
                true
            }
            _ => false,
        };

        if !should_continue {
            if let Some(session) = session {
                kill_running_session_sync(session);
            }
            if let Some(profile) = launch_profile.as_ref() {
                let _ = sync_profile_auth_after_exit(&app, &app.state::<PtyState>(), &profile.id);
            }
            let _ = app.emit("pty:exit", PtyExitPayload { id });
            return;
        }

        let _ = app.emit("pty:ready", PtyReadyPayload { id });

        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let data = String::from_utf8_lossy(&buffer[..count]).to_string();
                    let _ = app.emit("pty:data", PtyDataPayload { id, data });
                }
                Err(_) => break,
            }
        }

        let removed_session = app
            .state::<PtyState>()
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(&id));

        if let Some(session) = removed_session {
            kill_running_session_sync(session);
        }

        if let Some(profile) = launch_profile.as_ref() {
            if let Err(error) =
                sync_profile_auth_after_exit(&app, &app.state::<PtyState>(), &profile.id)
            {
                let _ = app.emit("pty:error", PtyErrorPayload { id, error });
            }
        }

        let _ = app.emit("pty:exit", PtyExitPayload { id });
    });

    Ok(())
}

#[tauri::command]
fn pty_write(state: State<'_, PtyState>, id: u64, data: String) -> Result<(), String> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "terminal session state is unavailable".to_string())?;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| format!("terminal session {id} does not exist"))?;

    match session {
        PtySession::Running { writer, .. } => {
            writer
                .write_all(data.as_bytes())
                .map_err(|error| error.to_string())?;
            writer.flush().map_err(|error| error.to_string())
        }
        PtySession::Starting => Err(format!("terminal session {id} is still starting")),
    }
}

#[tauri::command]
fn pty_resize(state: State<'_, PtyState>, id: u64, cols: u16, rows: u16) -> Result<(), String> {
    let sessions = state
        .sessions
        .lock()
        .map_err(|_| "terminal session state is unavailable".to_string())?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| format!("terminal session {id} does not exist"))?;

    match session {
        PtySession::Running { master, .. } => master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string()),
        PtySession::Starting => Ok(()),
    }
}

#[tauri::command]
fn pty_kill(app: AppHandle, state: State<'_, PtyState>, id: u64) -> Result<(), String> {
    let session = {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| "terminal session state is unavailable".to_string())?;

        sessions.remove(&id)
    };

    if let Some(session) = session {
        kill_running_session_background(app, session);
    }

    Ok(())
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(PtyState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            pi_profiles_list,
            pi_profile_save,
            pi_profile_delete,
            pi_profile_reveal_dir,
            pi_addons_list,
            pi_addons_package_command,
            pi_addons_set_global_packages,
            pioc_control_read,
            pioc_control_command,
            pty_start,
            pty_write,
            pty_resize,
            pty_kill
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn write_json(path: &Path, value: serde_json::Value) {
        let json = serde_json::to_string_pretty(&value).expect("test JSON should serialize");
        fs::write(path, format!("{json}\n")).expect("test JSON should be written");
    }

    #[test]
    fn now_string_uses_iso_utc_timestamps() {
        let timestamp = now_string();

        assert!(timestamp.contains('T'));
        assert!(timestamp.ends_with('Z'));
    }

    #[test]
    fn normalize_profile_trims_lists_and_removes_generated_extra_settings() {
        let mut profile = default_profile();
        profile.id = " custom_1 ".to_string();
        profile.name = " Custom profile ".to_string();
        profile.created_at = "1700000000".to_string();
        profile.updated_at = "".to_string();
        profile.last_used_at = Some("1700000001".to_string());
        profile.packages = vec![
            " beta ".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
            "".to_string(),
        ];
        profile
            .extra_settings
            .insert("theme".to_string(), json!("dark"));
        profile
            .extra_settings
            .insert("custom".to_string(), json!(true));

        let normalized = normalize_profile(profile).expect("profile should normalize");

        assert_eq!(normalized.id, "custom_1");
        assert_eq!(normalized.name, "Custom profile");
        assert_eq!(normalized.packages, vec!["beta", "alpha"]);
        assert!(!normalized.extra_settings.contains_key("theme"));
        assert_eq!(normalized.extra_settings.get("custom"), Some(&json!(true)));
        assert_eq!(normalized.created_at, "2023-11-14T22:13:20.000Z");
        assert!(normalized.updated_at.contains('T'));
        assert_eq!(
            normalized.last_used_at.as_deref(),
            Some("2023-11-14T22:13:21.000Z")
        );
    }

    #[test]
    fn auth_merge_preserves_disjoint_nested_credentials() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let openai_auth = temp_dir.path().join("openai.json");
        let anthropic_auth = temp_dir.path().join("anthropic.json");
        write_json(
            &openai_auth,
            json!({"providers": {"openai": {"apiKey": "openai-key"}}}),
        );
        write_json(
            &anthropic_auth,
            json!({"providers": {"anthropic": {"apiKey": "anthropic-key"}}}),
        );

        let merged = serde_json::Value::Object(
            merged_auth_from_candidates(&[
                (openai_auth, UNIX_EPOCH + Duration::from_secs(1)),
                (anthropic_auth, UNIX_EPOCH + Duration::from_secs(2)),
            ])
            .expect("auth files should merge"),
        );

        assert_eq!(
            merged["providers"]["openai"]["apiKey"].as_str(),
            Some("openai-key")
        );
        assert_eq!(
            merged["providers"]["anthropic"]["apiKey"].as_str(),
            Some("anthropic-key")
        );
    }

    #[test]
    fn newer_auth_candidate_wins_conflicts_without_dropping_other_keys() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let old_auth = temp_dir.path().join("old.json");
        let new_auth = temp_dir.path().join("new.json");
        write_json(
            &old_auth,
            json!({
                "providers": {"openai": {"apiKey": "old-key", "org": "team"}},
                "tokens": {"github": "gh-token"}
            }),
        );
        write_json(
            &new_auth,
            json!({"providers": {"openai": {"apiKey": "new-key"}}}),
        );

        let merged = serde_json::Value::Object(
            merged_auth_from_candidates(&[
                (old_auth, UNIX_EPOCH + Duration::from_secs(1)),
                (new_auth, UNIX_EPOCH + Duration::from_secs(2)),
            ])
            .expect("auth files should merge"),
        );

        assert_eq!(
            merged["providers"]["openai"]["apiKey"].as_str(),
            Some("new-key")
        );
        assert_eq!(merged["providers"]["openai"]["org"].as_str(), Some("team"));
        assert_eq!(merged["tokens"]["github"].as_str(), Some("gh-token"));
    }

    #[test]
    fn auth_writes_are_idempotent_and_detect_empty_credentials() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let auth_path = temp_dir.path().join("auth.json");
        let auth = json!({"providers": {"openai": {"apiKey": "key"}}})
            .as_object()
            .expect("auth fixture should be an object")
            .clone();

        assert!(write_auth_object_if_changed(&auth_path, &auth).expect("auth should write"));
        assert!(
            !write_auth_object_if_changed(&auth_path, &auth).expect("same auth should not rewrite")
        );
        assert!(read_auth_object(&auth_path)
            .expect("auth should be readable")
            .is_some());

        assert!(
            write_auth_object_if_changed(&auth_path, &serde_json::Map::new())
                .expect("empty auth should write")
        );
        assert!(read_auth_object(&auth_path)
            .expect("empty auth should be readable")
            .is_none());
    }

    #[test]
    fn package_source_input_accepts_full_pi_commands() {
        assert_eq!(
            normalize_package_source_input(Some("pi install --extension '@scope/pkg'".to_string())),
            Some("@scope/pkg".to_string())
        );
        assert_eq!(
            normalize_package_source_input(Some("update --extensions".to_string())),
            None
        );
    }
}

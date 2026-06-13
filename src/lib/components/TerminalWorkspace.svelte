<script lang="ts">
  import { onMount, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { invoke } from "@tauri-apps/api/core";
  import { Store } from "@tauri-apps/plugin-store";
  import { open } from "@tauri-apps/plugin-dialog";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check } from "@tauri-apps/plugin-updater";
  import { dragHandle, dragHandleZone, SHADOW_ITEM_MARKER_PROPERTY_NAME, type DndEvent } from "svelte-dnd-action";
  import RiDeleteBinLine from "remixicon-svelte/icons/delete-bin-line";
  import RiDragMove2Line from "remixicon-svelte/icons/drag-move-2-line";
  import RiFolderOpenLine from "remixicon-svelte/icons/folder-open-line";
  import RiStarFill from "remixicon-svelte/icons/star-fill";
  import RiStarLine from "remixicon-svelte/icons/star-line";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Separator } from "$lib/components/ui/separator";
  import WTermTerminal from "$lib/components/WTermTerminal.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import ASCIIText from "$lib/components/svelte-bits/ASCIIText.svelte";
  import type { CommandPaletteCommand } from "$lib/command-palette";
  import { cn } from "$lib/utils";

  type TerminalKind = "pi" | "shell";

  type PiProfile = {
    id: string;
    name: string;
    description?: string;
    defaultProvider?: string;
    defaultModel?: string;
    defaultThinkingLevel?: string;
    hideThinkingBlock?: boolean;
    theme?: string;
    quietStartup?: boolean;
    collapseChangelog?: boolean;
    enableInstallTelemetry?: boolean;
    doubleEscapeAction?: string;
    treeFilterMode?: string;
    steeringMode?: string;
    followUpMode?: string;
    transport?: string;
    showHardwareCursor?: boolean;
    enableSkillCommands?: boolean;
    packages: string[];
    skills: string[];
    extensions: string[];
    prompts: string[];
    themes: string[];
    extraSettings?: Record<string, unknown>;
    createdAt: string;
    updatedAt: string;
    lastUsedAt?: string;
  };


  type PiAddonPackage = {
    source: string;
    installedPath?: string;
    resourceTypes: string[];
    global: boolean;
  };

  type PiVanillaMigrationResult = {
    sourceDir: string;
    targetProfileId: string;
    copiedResourceFiles: number;
    importedPackages: number;
    activatedPackages: number;
    copiedPackages: number;
    globalPackages: number;
    updatedProfileSettings: boolean;
  };

  type PiProfileResourceType = "skills" | "extensions" | "prompts" | "themes";

  type TerminalSession = {
    id: number;
    workspaceInstanceId: string;
    cached: boolean;
    kind: TerminalKind;
    piProfileId?: string;
    workingDirectory?: string;
    closing?: boolean;
    hasUserInput?: boolean;
  };

  type TerminalDndItem = {
    id: string;
    terminalId: number;
    kind: TerminalKind;
    piProfileId?: string;
    [SHADOW_ITEM_MARKER_PROPERTY_NAME]?: boolean;
  };

  type SavedWorkspaceTerminal = {
    kind: TerminalKind;
    piProfileId?: string;
  };
  type SavedWorkspace = {
    id: string;
    name: string;
    workingDirectory?: string;
    terminals: SavedWorkspaceTerminal[];
    savedAt: string;
    lastUsedAt: string;
  };

  type WorkspaceRuntime = {
    instanceId: string;
    savedWorkspaceId?: string;
    name: string;
    workingDirectory?: string;
    selectedTerminalId: number | null;
    fullscreenTerminalId: number | null;
    cachedTerminalReady: boolean;
    dirty?: boolean;
  };
  const WORKSPACES_STORE_PATH = "workspaces.json";
  const WORKSPACES_KEY = "workspaces";
  const FALLBACK_STORAGE_KEY = "pioc.workspaces";
  const PROFILE_SETTINGS_STORE_PATH = "profile-settings.json";
  const FAVORITE_PI_PROFILE_KEY = "favoritePiProfileId";
  const FALLBACK_FAVORITE_PI_PROFILE_KEY = "pioc.favoritePiProfileId";
  const DEFAULT_PI_PROFILE_ID = "default";
  const APP_UPDATE_CHANNEL = import.meta.env.VITE_PIOC_UPDATE_CHANNEL ?? "dev";
  const APP_UPDATER_ENABLED = import.meta.env.VITE_PIOC_UPDATER_ENABLED === "true";
  // Keep prewarming wired up, but disabled by default to avoid an idle Pi Node runtime.
  // This can become a persisted user setting later.
  const PREWARM_FAVORITE_PI_PROFILE = false;
  const TERMINAL_CLOSE_CONFIRMATION_MESSAGE =
    "This terminal has received input. Closing it will stop the process and lose any in-progress work. Close it anyway?";
  const TERMINAL_DND_TYPE = "terminal-layout";
  const TERMINAL_DND_FLIP_DURATION_MS = 140;
  const TERMINAL_DND_DROP_TARGET_STYLE = {
    outline: "1px solid color-mix(in oklab, var(--ring) 65%, transparent)",
    outlineOffset: "-1px",
  };
  const PROFILE_SETTINGS_CONTROL_CLASS =
    "dark:bg-input/30 border-input focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 disabled:bg-input/50 dark:disabled:bg-input/80 h-8 w-full min-w-0 rounded-none border bg-transparent px-2.5 py-1 text-xs transition-colors focus-visible:ring-1 aria-invalid:ring-1 outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50";
  const PROFILE_SETTINGS_TEXTAREA_CLASS =
    "dark:bg-input/30 border-input focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 disabled:bg-input/50 dark:disabled:bg-input/80 min-h-28 w-full min-w-0 resize-y rounded-none border bg-transparent px-2.5 py-2 font-mono text-xs transition-colors focus-visible:ring-1 aria-invalid:ring-1 outline-none placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50";
  const PROFILE_EXTRA_SETTINGS_PLACEHOLDER =
    '{\n  "terminal": { "showImages": true },\n  "compaction": { "enabled": true }\n}';

  type HomeAsciiPalette = {
    gradient: [string, string, string];
  };

  const HOME_ASCII_PALETTE_STORAGE_KEY = "pioc-home-ascii-palette";
  const DEFAULT_HOME_ASCII_PALETTE: HomeAsciiPalette = {
    gradient: ["#67e8f9", "#38bdf8", "#2563eb"],
  };
  const HOME_ASCII_PALETTES: HomeAsciiPalette[] = [
    DEFAULT_HOME_ASCII_PALETTE,
    { gradient: ["#a7f3d0", "#34d399", "#14b8a6"] },
    { gradient: ["#bef264", "#22c55e", "#10b981"] },
    { gradient: ["#c4b5fd", "#8b5cf6", "#6366f1"] },
    { gradient: ["#f0abfc", "#c084fc", "#818cf8"] },
    { gradient: ["#e0f2fe", "#7dd3fc", "#22d3ee"] },
    { gradient: ["#f9a8d4", "#ec4899", "#8b5cf6"] },
    { gradient: ["#fb7185", "#f97316", "#facc15"] },
    { gradient: ["#fde68a", "#f59e0b", "#ef4444"] },
  ];

  function readHomeAsciiPaletteIndex(): number | null {
    if (typeof localStorage === "undefined") return null;

    try {
      const index = Number(localStorage.getItem(HOME_ASCII_PALETTE_STORAGE_KEY));
      return Number.isInteger(index) && index >= 0 && index < HOME_ASCII_PALETTES.length ? index : null;
    } catch {
      return null;
    }
  }

  function writeHomeAsciiPaletteIndex(index: number) {
    if (typeof localStorage === "undefined") return;

    try {
      localStorage.setItem(HOME_ASCII_PALETTE_STORAGE_KEY, String(index));
    } catch {
      // Ignore storage failures; this launch still gets a fresh color.
    }
  }

  function nextHomeAsciiPaletteIndex() {
    const previousIndex = readHomeAsciiPaletteIndex();
    const availableIndexes = HOME_ASCII_PALETTES
      .map((_, index) => index)
      .filter((index) => index !== previousIndex);
    const index = availableIndexes[Math.floor(Math.random() * availableIndexes.length)] ?? 0;

    writeHomeAsciiPaletteIndex(index);
    return index;
  }

  function createHomeAsciiPalette(): HomeAsciiPalette {
    return HOME_ASCII_PALETTES[nextHomeAsciiPaletteIndex()] ?? DEFAULT_HOME_ASCII_PALETTE;
  }

  let homeAsciiPalette = $state(createHomeAsciiPalette());

  function cycleHomeAsciiPalette() {
    homeAsciiPalette = createHomeAsciiPalette();
  }

  function createWorkspaceInstanceId() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return `runtime-${crypto.randomUUID()}`;
    }

    return `runtime-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }

  function createWorkspaceRuntime(options: Partial<WorkspaceRuntime> = {}): WorkspaceRuntime {
    return {
      instanceId: options.instanceId ?? createWorkspaceInstanceId(),
      savedWorkspaceId: options.savedWorkspaceId,
      name: options.name?.trim() || "Untitled",
      workingDirectory: options.workingDirectory,
      selectedTerminalId: options.selectedTerminalId ?? null,
      fullscreenTerminalId: options.fullscreenTerminalId ?? null,
      cachedTerminalReady: options.cachedTerminalReady ?? false,
      dirty: options.dirty ?? false,
    };
  }
  let {
    onHotkeysChange,
    onAppUpdateActionChange,
  }: {
    onHotkeysChange?: (hotkeys: string[]) => void;
    onAppUpdateActionChange?: (action: { disabled: boolean; title: string; run: () => void }) => void;
  } = $props();
  const initialWorkspaceInstanceId = createWorkspaceInstanceId();
  let terminalSessions = $state<TerminalSession[]>(
    PREWARM_FAVORITE_PI_PROFILE
      ? [
          {
            id: 1,
            workspaceInstanceId: initialWorkspaceInstanceId,
            cached: true,
            kind: "pi",
            piProfileId: DEFAULT_PI_PROFILE_ID,
          },
        ]
      : [],
  );
  let nextTerminalId = PREWARM_FAVORITE_PI_PROFILE ? 1 : 0;
  let cachedTerminalReady = $state(false);
  let selectedTerminalId = $state<number | null>(null);
  let fullscreenTerminalId = $state<number | null>(null);
  let savedWorkspaces = $state<SavedWorkspace[]>([]);
  let runningWorkspaces = $state<WorkspaceRuntime[]>([
    createWorkspaceRuntime({ instanceId: initialWorkspaceInstanceId }),
  ]);
  let activeWorkspaceInstanceId = $state(initialWorkspaceInstanceId);

  let piProfiles = $state<PiProfile[]>([]);
  let piAddonPackages = $state<PiAddonPackage[]>([]);
  let piAddonsReady = $state(false);
  let piAddonGlobalSaving = $state(false);
  let favoritePiProfileId = $state(DEFAULT_PI_PROFILE_ID);
  let launchProfileDialogOpen = $state(false);
  let launchProfileId = $state(DEFAULT_PI_PROFILE_ID);
  let profilesReady = $state(false);
  let profileDialogOpen = $state(false);
  let selectedProfileId = $state(DEFAULT_PI_PROFILE_ID);
  let profileError = $state("");
  let profileNameDraft = $state("");
  let profileDescriptionDraft = $state("");
  let profileProviderDraft = $state("");
  let profileModelDraft = $state("");
  let profileThinkingDraft = $state("");
  let profileShowThinkingBlocksDraft = $state(true);
  let profileThemeDraft = $state("");
  let profileQuietStartupDraft = $state(false);
  let profileCollapseChangelogDraft = $state(false);
  let profileEnableInstallTelemetryDraft = $state(true);
  let profileDoubleEscapeActionDraft = $state("");
  let profileTreeFilterModeDraft = $state("");
  let profileSteeringModeDraft = $state("");
  let profileFollowUpModeDraft = $state("");
  let profileTransportDraft = $state("");
  let profileShowHardwareCursorDraft = $state(false);
  let profileEnableSkillCommandsDraft = $state(true);
  let profileExtraSettingsDraft = $state("");
  let profileSelectedPackageSources = $state<string[]>([]);
  let profileSkillsDraft = $state("");
  let profileExtensionsDraft = $state("");
  let profilePromptsDraft = $state("");
  let profileThemesDraft = $state("");
  let profileResourceFolderOpening = $state<PiProfileResourceType | null>(null);
  let profilePackageCommandRunning = $state(false);
  let piVanillaMigrationRunning = $state(false);
  let piAddonBusy = $derived(profilePackageCommandRunning || piAddonGlobalSaving || piVanillaMigrationRunning);
  let skillAddonPackages = $derived(piAddonPackages.filter((addonPackage) => addonPackage.resourceTypes.includes("skills")));
  let extensionAddonPackages = $derived(piAddonPackages.filter((addonPackage) => addonPackage.resourceTypes.includes("extensions")));
  let promptAddonPackages = $derived(piAddonPackages.filter((addonPackage) => addonPackage.resourceTypes.includes("prompts")));
  let themeAddonPackages = $derived(piAddonPackages.filter((addonPackage) => addonPackage.resourceTypes.includes("themes")));
  let unclassifiedAddonPackages = $derived(piAddonPackages.filter((addonPackage) => addonPackage.resourceTypes.length === 0));

  let appUpdateRunning = $state(false);
  let appUpdateMessage = $state("");

  let saveWorkspaceDialogOpen = $state(false);
  let openWorkspaceDialogOpen = $state(false);
  let closeConfirmationOpen = $state(false);
  let pendingCloseTerminalId = $state<number | null>(null);
  let commandPaletteOpen = $state(false);
  let workspaceNameDraft = $state("");
  let workspaceDirectoryDraft = $state("");
  let workspacePersistenceReady = $state(false);
  let workspaceStore: Store | null = null;
  let profileSettingsStore: Store | null = null;
  let terminalDndActive = $state(false);
  let terminalDndWorkspaceInstanceId = $state<string | null>(null);
  let terminalDndInitialTerminalIds = $state<number[]>([]);
  let terminalDndItems = $state<TerminalDndItem[]>([]);
  let activeTerminalCount = $derived(
    terminalSessions.filter(
      (terminal) =>
        terminal.workspaceInstanceId === activeWorkspaceInstanceId && !terminal.cached && !terminal.closing,
    ).length,
  );
  let totalRunningTerminalCount = $derived(
    terminalSessions.filter((terminal) => !terminal.cached && !terminal.closing).length,
  );
  let terminalLayoutKey = $derived(
    [
      `workspace:${activeWorkspaceInstanceId}`,
      fullscreenTerminalId === null ? "layout:all" : `layout:fullscreen:${fullscreenTerminalId}`,
      ...terminalSessions
        .filter(
          (terminal) =>
            terminal.workspaceInstanceId === activeWorkspaceInstanceId &&
            !terminal.cached &&
            (fullscreenTerminalId === null || terminal.id === fullscreenTerminalId),
        )
        .map((terminal) => `${terminal.id}:${terminal.closing ? "closing" : "open"}`),
    ].join("|"),
  );
  let activeLayoutTerminals = $derived(terminalSessions.filter(terminalIsLayoutVisible));
  let terminalDndEnabled = $derived(activeLayoutTerminals.length > 1 && fullscreenTerminalId === null);
  let terminalDndOptions = $derived({
    items: terminalDndItems,
    type: TERMINAL_DND_TYPE,
    flipDurationMs: TERMINAL_DND_FLIP_DURATION_MS,
    dragDisabled: !terminalDndEnabled,
    dropFromOthersDisabled: true,
    dropTargetStyle: TERMINAL_DND_DROP_TARGET_STYLE,
    delayTouchStart: true,
    useCursorForDetection: true,
  });
  let favoritePiLaunchReady = $derived(!PREWARM_FAVORITE_PI_PROFILE || cachedTerminalReady);
  let commandPaletteCommands = $derived([
    {
      id: "terminal.new-favorite-pi",
      title: newFavoritePiHotkeyText(),
      description: favoritePiLaunchReady
        ? `Launch ${profileDisplayName(favoritePiProfileId)} in the active workspace.`
        : `Preparing ${hotkeyProfileDisplayName(favoritePiProfileId)} PI instance…`,
      group: "Functions",
      keywords: ["new", "launch", "pi", "agent", "favorite", profileDisplayName(favoritePiProfileId)],
      shortcut: "CTRL+N",
      disabled: !favoritePiLaunchReady,
      run: () => spawnTerminal(favoritePiProfileId),
    },
    {
      id: "terminal.launch-pi-profile",
      title: "Launch Pi profile…",
      description: profilesReady ? "Choose a Pi profile to launch." : "Loading Pi profiles…",
      group: "Functions",
      keywords: ["new", "launch", "pi", "profile", "agent", "select"],
      shortcut: "CTRL+SHIFT+N",
      disabled: !profilesReady,
      run: openLaunchProfileDialog,
    },
    {
      id: "terminal.new-shell",
      title: "New terminal",
      description: "Open a local shell terminal in the active workspace.",
      group: "Functions",
      keywords: ["new", "terminal", "shell", "pty", "command"],
      shortcut: "CTRL+T",
      run: spawnEmptyTerminal,
    },
    {
      id: "terminal.toggle-fullscreen",
      title: fullscreenTerminalId === null ? "Fullscreen selected terminal" : "Exit fullscreen",
      description: activeTerminalCount > 1 && selectedTerminalId !== null
        ? "Toggle focus mode for the selected terminal."
        : "Select one of multiple terminals first.",
      group: "Functions",
      keywords: ["fullscreen", "focus", "terminal", "layout"],
      shortcut: "CTRL+F",
      disabled: activeTerminalCount <= 1 || selectedTerminalId === null,
      run: toggleFullscreenSelectedTerminal,
    },
    {
      id: "terminal.close-selected",
      title: "Close selected terminal",
      description: activeTerminalCount > 0 ? "Close the selected or newest terminal." : "No terminal is open.",
      group: "Functions",
      keywords: ["close", "terminal", "quit", "stop"],
      shortcut: "CTRL+Q",
      disabled: activeTerminalCount === 0,
      run: () => void closeSelectedOrNewestTerminal(),
    },
    {
      id: "workspace.new",
      title: "New workspace",
      description: "Create an empty workspace without stopping the current one.",
      group: "Workspaces",
      keywords: ["new", "workspace", "tab"],
      run: createNewWorkspace,
    },
    {
      id: "workspace.close",
      title: "Close workspace",
      description: activeTerminalCount > 0
        ? `Close ${activeWorkspaceName()} and stop its running terminals.`
        : `Close ${activeWorkspaceName()}.`,
      group: "Workspaces",
      keywords: ["close", "workspace", "tab", activeWorkspaceName()],
      disabled: runningWorkspaces.length === 0,
      run: () => requestCloseWorkspace(activeWorkspaceInstanceId),
    },
    {
      id: "workspace.save",
      title: "Save workspace…",
      description: workspacePersistenceReady
        ? activeTerminalCount > 0
          ? "Persist the current terminal layout and working directory."
          : "Start a terminal before saving a workspace."
        : "Loading workspace storage…",
      group: "Workspaces",
      keywords: ["save", "workspace", "layout", activeWorkspaceName()],
      shortcut: "CTRL+S",
      disabled: !workspacePersistenceReady || activeTerminalCount === 0,
      run: openSaveWorkspaceDialog,
    },
    {
      id: "workspace.open",
      title: "Open workspace…",
      description: savedWorkspaces.length > 0 ? "Restore a saved workspace." : "No saved workspaces yet.",
      group: "Workspaces",
      keywords: ["open", "workspace", "restore", ...savedWorkspaces.map((workspace) => workspace.name)],
      shortcut: "CTRL+O",
      disabled: !workspacePersistenceReady || savedWorkspaces.length === 0,
      run: openWorkspaceDialog,
    },
    {
      id: "profile.manage",
      title: "Manage Pi profiles…",
      description: profilesReady ? "Edit Pi defaults, packages, skills, extensions, prompts, and themes." : "Loading Pi profiles…",
      group: "Profiles",
      keywords: ["profile", "settings", "packages", "skills", "extensions", "prompts", "themes", "pi"],
      disabled: !profilesReady,
      run: openProfileDialog,
    },
    {
      id: "app.check-updates",
      title: appUpdateRunning ? "Checking for app updates…" : "Check for app updates",
      description: appUpdateCommandDescription(),
      group: "App",
      keywords: ["update", "upgrade", "release", "restart", "prod", APP_UPDATE_CHANNEL],
      disabled: appUpdateDisabled(),
      run: () => void checkForAppUpdate(),
    },
    {
      id: "app.open-diagnostics-log",
      title: "Open diagnostics log",
      description: "Open the verbose PIOC log for bug reports.",
      group: "App",
      keywords: ["diagnostics", "logs", "bug", "report", "pi", "update"],
      run: () => void openDiagnosticLog(),
    },
  ] satisfies CommandPaletteCommand[]);
  $effect(() => {
    const hotkeys: string[] = [];

    if (activeTerminalCount > 0) {
      hotkeys.push("CTRL+K: Command Palette");
      hotkeys.push("CTRL+S: Save Workspace");

      if (savedWorkspaces.length > 0) {
        hotkeys.push("CTRL+O: Open Workspace");
      }

      if (profilesReady) {
        hotkeys.push("CTRL+SHIFT+N: Select PI");
      }

      if (favoritePiLaunchReady) {
        hotkeys.push(`CTRL+N: ${newFavoritePiHotkeyText()}`);
      }
      hotkeys.push("CTRL+T: Terminal");

      if (activeTerminalCount > 1 && selectedTerminalId !== null) {
        hotkeys.push(fullscreenTerminalId === null ? "CTRL+F: Fullscreen" : "CTRL+F: Exit Fullscreen");
      }

      hotkeys.push("CTRL+Q: Close");
    }

    onHotkeysChange?.(hotkeys);
  });

  $effect(() => {
    onAppUpdateActionChange?.({
      disabled: appUpdateDisabled(),
      title: appUpdateCommandDescription(),
      run: () => void checkForAppUpdate(),
    });
  });

  $effect(() => {
    if (!terminalDndActive) {
      terminalDndItems = terminalDndItemsForSessions(activeLayoutTerminals);
    }
  });

  onMount(() => {
    void loadSavedWorkspaces();
    void (async () => {
      await initializePiProfiles();
      await loadPiAddons();
    })();
  });

  onMount(() => {
    function handleHotkey(event: KeyboardEvent) {
      if (event.altKey || event.repeat) return;

      const key = event.key.toLowerCase();
      const isCommandPaletteShortcut = key === "k" && !event.shiftKey && (event.ctrlKey || event.metaKey);

      if (!isCommandPaletteShortcut && (!event.ctrlKey || event.metaKey || commandPaletteOpen)) return;

      function consumeHotkey() {
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation();
      }

      if (isCommandPaletteShortcut) {
        consumeHotkey();
        commandPaletteOpen = !commandPaletteOpen;
        return;
      }

      if (key === "s") {
        consumeHotkey();
        openSaveWorkspaceDialog();
        return;
      }

      if (key === "o") {
        consumeHotkey();
        openWorkspaceDialog();
        return;
      }
      if (key === "n") {
        consumeHotkey();
        if (event.shiftKey) {
          openLaunchProfileDialog();
        } else {
          spawnTerminal(favoritePiProfileId);
        }
        return;
      }
      if (key === "t") {
        consumeHotkey();
        spawnEmptyTerminal();
        return;
      }

      if (key === "f") {
        consumeHotkey();
        toggleFullscreenSelectedTerminal();
        return;
      }

      if (key === "q") {
        consumeHotkey();
        void closeSelectedOrNewestTerminal();
      }
    }

    window.addEventListener("keydown", handleHotkey, { capture: true });

    return () => {
      window.removeEventListener("keydown", handleHotkey, { capture: true });
    };
  });

  function isTerminalKind(value: unknown): value is TerminalKind {
    return value === "pi" || value === "shell";
  }
  function normalizeWorkingDirectory(value: unknown) {
    return typeof value === "string" ? value.trim() : "";
  }

  function normalizeProfileId(value: unknown) {
    const profileId = typeof value === "string" ? value.trim() : "";
    return profileId || DEFAULT_PI_PROFILE_ID;
  }

  function splitProfileList(value: string) {
    return value
      .split(/[\r\n,]+/)
      .map((item) => item.trim())
      .filter(Boolean);
  }

  function joinProfileList(values: string[]) {
    return values.join("\n");
  }

  function formatProfileExtraSettings(settings?: Record<string, unknown>) {
    if (!settings || Object.keys(settings).length === 0) return "";

    return JSON.stringify(settings, null, 2);
  }

  function appUpdateDisabled() {
    return !APP_UPDATER_ENABLED || appUpdateRunning || totalRunningTerminalCount > 0;
  }

  function appUpdateCommandDescription() {
    if (!APP_UPDATER_ENABLED) return `Updates are disabled for ${APP_UPDATE_CHANNEL} builds.`;
    if (appUpdateRunning) return appUpdateMessage || "Checking for signed app updates…";
    if (totalRunningTerminalCount > 0) return "Close all running terminals before installing an app update.";
    return "Check the production release channel for a signed PIOC update.";
  }

  function formatUpdateBytes(bytes: number) {
    if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  function formatUpdateProgress(downloaded: number, total: number) {
    return total > 0
      ? `${formatUpdateBytes(downloaded)} / ${formatUpdateBytes(total)}`
      : formatUpdateBytes(downloaded);
  }

  async function checkForAppUpdate() {
    if (appUpdateRunning) return;

    if (!APP_UPDATER_ENABLED) {
      appUpdateMessage = `Updates are disabled for ${APP_UPDATE_CHANNEL} builds.`;
      return;
    }

    if (totalRunningTerminalCount > 0) {
      appUpdateMessage = "Close all running terminals before checking for an app update.";
      return;
    }

    appUpdateRunning = true;
    appUpdateMessage = "Checking for signed app updates…";

    try {
      const update = await check();

      if (!update) {
        appUpdateMessage = "PIOC is up to date.";
        return;
      }

      const notes = update.body?.trim();
      const shouldInstall = window.confirm(
        [
          `PIOC ${update.version} is available.`,
          notes ? `\nRelease notes:\n${notes}` : "",
          "\nDownload, install, and relaunch now?",
        ].join("\n"),
      );

      if (!shouldInstall) {
        appUpdateMessage = `Update ${update.version} is available. Run Check for app updates when you are ready to install it.`;
        return;
      }

      let downloaded = 0;
      let contentLength = 0;
      appUpdateMessage = `Downloading PIOC ${update.version}…`;

      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            contentLength = event.data.contentLength ?? 0;
            downloaded = 0;
            appUpdateMessage = `Downloading PIOC ${update.version} (${formatUpdateProgress(downloaded, contentLength)})…`;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            appUpdateMessage = `Downloading PIOC ${update.version} (${formatUpdateProgress(downloaded, contentLength)})…`;
            break;
          case "Finished":
            appUpdateMessage = "Update installed. Relaunching PIOC…";
            break;
        }
      });

      await relaunch();
    } catch (error) {
      appUpdateMessage = error instanceof Error ? error.message : String(error);
    } finally {
      appUpdateRunning = false;
    }
  }

  async function openDiagnosticLog() {
    try {
      const path = await invoke<string>("pioc_diagnostic_log_open");
      appUpdateMessage = `Diagnostics log opened: ${path}`;
    } catch (error) {
      appUpdateMessage = error instanceof Error ? error.message : String(error);
    }
  }
  function resetProfileSettingsDrafts(profile?: PiProfile | null) {
    profileShowThinkingBlocksDraft = !(profile?.hideThinkingBlock ?? false);
    profileThemeDraft = profile?.theme ?? "";
    profileQuietStartupDraft = profile?.quietStartup ?? false;
    profileCollapseChangelogDraft = profile?.collapseChangelog ?? false;
    profileEnableInstallTelemetryDraft = profile?.enableInstallTelemetry ?? true;
    profileDoubleEscapeActionDraft = profile?.doubleEscapeAction ?? "";
    profileTreeFilterModeDraft = profile?.treeFilterMode ?? "";
    profileSteeringModeDraft = profile?.steeringMode ?? "";
    profileFollowUpModeDraft = profile?.followUpMode ?? "";
    profileTransportDraft = profile?.transport ?? "";
    profileShowHardwareCursorDraft = profile?.showHardwareCursor ?? false;
    profileEnableSkillCommandsDraft = profile?.enableSkillCommands ?? true;
    profileExtraSettingsDraft = formatProfileExtraSettings(profile?.extraSettings);
  }

  function parseProfileExtraSettingsDraft() {
    const draft = profileExtraSettingsDraft.trim();
    if (!draft) return {};

    const parsed: unknown = JSON.parse(draft);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error("Additional settings JSON must be an object.");
    }

    return parsed as Record<string, unknown>;
  }
  function uniqueProfileList(values: string[]) {
    return [...new Set(values.map((value) => value.trim()).filter(Boolean))];
  }

  function normalizePiPackageSourceInput(value: string) {
    let source = value.trim();
    const lower = source.toLowerCase();

    if (lower === "pi") return "";
    if (lower.startsWith("pi ")) {
      source = source.slice(3).trimStart();
    }

    const commandLower = source.toLowerCase();
    for (const command of ["install", "remove", "uninstall", "update", "list"]) {
      if (commandLower === command) return "";
      if (commandLower.startsWith(`${command} `)) {
        source = source.slice(command.length + 1).trimStart();
        break;
      }
    }

    const flagLower = source.toLowerCase();
    if (flagLower === "--extensions") return "";
    for (const flag of ["--extension ", "--extension=", "-l ", "--local "]) {
      if (flagLower.startsWith(flag)) {
        source = source.slice(flag.length).trimStart();
        break;
      }
    }

    if (
      (source.startsWith('"') && source.endsWith('"')) ||
      (source.startsWith("'") && source.endsWith("'"))
    ) {
      source = source.slice(1, -1).trim();
    }

    return source;
  }

  function setProfilePackageDrafts(packages: string[]) {
    profileSelectedPackageSources = uniqueProfileList(
      packages.map((packageEntry) => normalizePiPackageSourceInput(packageEntry) || packageEntry.trim()),
    );
  }

  function selectedProfilePackagesForSave() {
    return uniqueProfileList(profileSelectedPackageSources);
  }

  function profilePackageSelected(source: string) {
    return profileSelectedPackageSources.includes(source);
  }

  function toggleProfilePackageSelection(source: string) {
    if (piAddonBusy) return;

    profileSelectedPackageSources = profilePackageSelected(source)
      ? profileSelectedPackageSources.filter((selectedSource) => selectedSource !== source)
      : [...profileSelectedPackageSources, source];
  }

  function selectedAddonCount(addonPackages: PiAddonPackage[]) {
    return addonPackages.filter((addonPackage) => addonPackage.global || profilePackageSelected(addonPackage.source)).length;
  }

  function globalAddonCount(addonPackages: PiAddonPackage[]) {
    return addonPackages.filter((addonPackage) => addonPackage.global).length;
  }

  async function toggleGlobalPackageSelection(source: string) {
    if (piAddonBusy) return;

    const previousPackages = piAddonPackages;
    const nextPackages = piAddonPackages.map((addonPackage) =>
      addonPackage.source === source ? { ...addonPackage, global: !addonPackage.global } : addonPackage,
    );
    const globalSources = nextPackages.filter((addonPackage) => addonPackage.global).map((addonPackage) => addonPackage.source);

    piAddonPackages = nextPackages;
    piAddonGlobalSaving = true;
    profileError = "";

    try {
      piAddonPackages = await invoke<PiAddonPackage[]>("pi_addons_set_global_packages", { sources: globalSources });
    } catch (error) {
      piAddonPackages = previousPackages;
      profileError = error instanceof Error ? error.message : String(error);
    } finally {
      piAddonGlobalSaving = false;
    }
  }

  function profileById(profileId?: string) {
    const normalizedProfileId = normalizeProfileId(profileId);
    return piProfiles.find((profile) => profile.id === normalizedProfileId) ?? null;
  }

  function profileDisplayName(profileId?: string) {
    const normalizedProfileId = normalizeProfileId(profileId);
    return profileById(normalizedProfileId)?.name ?? (normalizedProfileId === DEFAULT_PI_PROFILE_ID ? "Default Pi" : normalizedProfileId);
  }

  function hotkeyProfileDisplayName(profileId?: string) {
    const profileName = profileDisplayName(profileId).replace(/\s+pi$/i, "").trim() || "Default";

    return profileName
      .split(/\s+/)
      .map((word) => (word === word.toLowerCase() ? `${word.charAt(0).toUpperCase()}${word.slice(1)}` : word))
      .join(" ");
  }

  function newFavoritePiHotkeyText() {
    return `New ${hotkeyProfileDisplayName(favoritePiProfileId)} PI`;
  }


  function samePiProfile(left?: string, right?: string) {
    return normalizeProfileId(left) === normalizeProfileId(right);
  }

  function createProfileId(name: string) {
    const base =
      name
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, "-")
        .replace(/^-+|-+$/g, "") || "profile";
    let candidate = base;
    let suffix = 2;

    while (piProfiles.some((profile) => profile.id === candidate)) {
      candidate = `${base}-${suffix++}`;
    }

    return candidate;
  }

  function resetCachedPiTerminal(workingDirectory = activeWorkspaceWorkingDirectory()) {
    if (!PREWARM_FAVORITE_PI_PROFILE) return;

    const workspace = ensureActiveWorkspaceRuntime();
    const normalizedWorkingDirectory = normalizeWorkingDirectory(workingDirectory);
    const nextCachedTerminal: TerminalSession = {
      id: ++nextTerminalId,
      workspaceInstanceId: workspace.instanceId,
      cached: true,
      kind: "pi",
      piProfileId: favoritePiProfileId || DEFAULT_PI_PROFILE_ID,
      workingDirectory: normalizedWorkingDirectory || undefined,
    };

    terminalSessions = [
      ...terminalSessions.filter(
        (terminal) => !(terminal.workspaceInstanceId === workspace.instanceId && terminal.cached),
      ),
      nextCachedTerminal,
    ];
    cachedTerminalReady = false;
    updateActiveWorkspaceRuntime((currentWorkspace) => ({
      ...currentWorkspace,
      cachedTerminalReady: false,
    }));
  }

  async function initializePiProfiles() {
    await loadFavoritePiProfileId();
    await loadPiProfiles();
  }

  async function loadFavoritePiProfileId() {
    try {
      profileSettingsStore = await Store.load(PROFILE_SETTINGS_STORE_PATH);
      const storedProfileId = await profileSettingsStore.get<unknown>(FAVORITE_PI_PROFILE_KEY);
      if (typeof storedProfileId === "string") {
        favoritePiProfileId = normalizeProfileId(storedProfileId);
        launchProfileId = favoritePiProfileId;
      }
      return;
    } catch {
      profileSettingsStore = null;
    }

    try {
      const storedProfileId = localStorage.getItem(FALLBACK_FAVORITE_PI_PROFILE_KEY);
      if (storedProfileId) {
        favoritePiProfileId = normalizeProfileId(storedProfileId);
        launchProfileId = favoritePiProfileId;
      }
    } catch {
      // Ignore fallback storage failures.
    }
  }

  async function persistFavoritePiProfileId(profileId: string) {
    if (profileSettingsStore) {
      try {
        await profileSettingsStore.set(FAVORITE_PI_PROFILE_KEY, profileId);
        await profileSettingsStore.save();
        return;
      } catch {
        profileSettingsStore = null;
      }
    }

    try {
      localStorage.setItem(FALLBACK_FAVORITE_PI_PROFILE_KEY, profileId);
    } catch {
      // Ignore fallback storage failures.
    }
  }

  function setFavoritePiProfile(profileId: string) {
    const normalizedProfileId = normalizeProfileId(profileId);
    const nextFavoriteProfile =
      piProfiles.find((profile) => samePiProfile(profile.id, normalizedProfileId)) ?? piProfiles[0];
    const nextFavoriteProfileId = nextFavoriteProfile?.id ?? DEFAULT_PI_PROFILE_ID;
    const changed = !samePiProfile(favoritePiProfileId, nextFavoriteProfileId);

    favoritePiProfileId = nextFavoriteProfileId;

    if (changed) {
      resetCachedPiTerminal();
    }

    void persistFavoritePiProfileId(nextFavoriteProfileId);
  }

  function openLaunchProfileDialog() {
    if (!profilesReady) return;

    profileError = "";
    launchProfileId = profileById(launchProfileId)?.id ?? profileById(favoritePiProfileId)?.id ?? piProfiles[0]?.id ?? DEFAULT_PI_PROFILE_ID;
    launchProfileDialogOpen = true;
  }

  function launchSelectedPiProfile(profileId = launchProfileId) {
    if (!profilesReady) return;

    launchProfileId = normalizeProfileId(profileId);
    spawnTerminal(launchProfileId, true);
    launchProfileDialogOpen = false;
  }

  function populateProfileDraft(profile?: PiProfile | null) {
    const profileToEdit = profile ?? profileById(selectedProfileId) ?? piProfiles[0];

    if (!profileToEdit) {
      selectedProfileId = "";
      profileNameDraft = "";
      profileDescriptionDraft = "";
      profileProviderDraft = "";
      profileModelDraft = "";
      profileThinkingDraft = "";
      resetProfileSettingsDrafts(null);
      profileSelectedPackageSources = [];
      profileSkillsDraft = "skills";
      profileExtensionsDraft = "extensions";
      profilePromptsDraft = "prompts";
      profileThemesDraft = "themes";
      return;
    }

    selectedProfileId = profileToEdit.id;
    profileNameDraft = profileToEdit.name;
    profileDescriptionDraft = profileToEdit.description ?? "";
    profileProviderDraft = profileToEdit.defaultProvider ?? "";
    profileModelDraft = profileToEdit.defaultModel ?? "";
    profileThinkingDraft = profileToEdit.defaultThinkingLevel ?? "";
    resetProfileSettingsDrafts(profileToEdit);
    setProfilePackageDrafts(profileToEdit.packages);
    profileSkillsDraft = joinProfileList(profileToEdit.skills);
    profileExtensionsDraft = joinProfileList(profileToEdit.extensions);
    profilePromptsDraft = joinProfileList(profileToEdit.prompts);
    profileThemesDraft = joinProfileList(profileToEdit.themes);
  }

  function newProfileDraft() {
    selectedProfileId = "";
    profileError = "";
    profileNameDraft = "New Pi Profile";
    profileDescriptionDraft = "";
    profileProviderDraft = "";
    profileModelDraft = "";
    profileThinkingDraft = "";
    resetProfileSettingsDrafts(null);
    profileSelectedPackageSources = [];
    profileSkillsDraft = "skills";
    profileExtensionsDraft = "extensions";
    profilePromptsDraft = "prompts";
    profileThemesDraft = "themes";
  }

  async function openProfileDialog() {
    profileError = "";
    await loadPiAddons();
    populateProfileDraft(profileById(favoritePiProfileId) ?? piProfiles[0]);
    profileDialogOpen = true;
  }

  async function loadPiAddons() {
    try {
      piAddonPackages = await invoke<PiAddonPackage[]>("pi_addons_list");
      piAddonsReady = true;
    } catch (error) {
      piAddonsReady = true;
      profileError = error instanceof Error ? error.message : String(error);
    }
  }
  async function loadPiProfiles() {
    try {
      const profiles = await invoke<PiProfile[]>("pi_profiles_list");
      piProfiles = profiles;
      const nextFavoriteProfile =
        profiles.find((profile) => samePiProfile(profile.id, favoritePiProfileId)) ??
        profiles.find((profile) => profile.id === DEFAULT_PI_PROFILE_ID) ??
        profiles[0];
      favoritePiProfileId = nextFavoriteProfile?.id ?? DEFAULT_PI_PROFILE_ID;
      launchProfileId = profileById(launchProfileId)?.id ?? favoritePiProfileId;
      profilesReady = true;
      resetCachedPiTerminal();
      void persistFavoritePiProfileId(favoritePiProfileId);
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
      profilesReady = true;
    }
  }

  async function saveProfileDraft(event?: SubmitEvent) {
    event?.preventDefault();

    const name = profileNameDraft.trim();
    if (!name) {
      profileError = "Profile name is required.";
      return;
    }

    profileError = "";
    let extraSettings: Record<string, unknown>;
    try {
      extraSettings = parseProfileExtraSettingsDraft();
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
      return;
    }

    const existingProfile = selectedProfileId ? profileById(selectedProfileId) : null;
    const timestamp = new Date().toISOString();
    const profile: PiProfile = {
      id: existingProfile?.id ?? createProfileId(name),
      name,
      description: profileDescriptionDraft.trim() || undefined,
      defaultProvider: profileProviderDraft.trim() || undefined,
      defaultModel: profileModelDraft.trim() || undefined,
      defaultThinkingLevel: profileThinkingDraft.trim() || undefined,
      hideThinkingBlock: !profileShowThinkingBlocksDraft,
      theme: profileThemeDraft.trim() || undefined,
      quietStartup: profileQuietStartupDraft,
      collapseChangelog: profileCollapseChangelogDraft,
      enableInstallTelemetry: profileEnableInstallTelemetryDraft,
      doubleEscapeAction: profileDoubleEscapeActionDraft.trim() || undefined,
      treeFilterMode: profileTreeFilterModeDraft.trim() || undefined,
      steeringMode: profileSteeringModeDraft.trim() || undefined,
      followUpMode: profileFollowUpModeDraft.trim() || undefined,
      transport: profileTransportDraft.trim() || undefined,
      showHardwareCursor: profileShowHardwareCursorDraft,
      enableSkillCommands: profileEnableSkillCommandsDraft,
      packages: selectedProfilePackagesForSave(),
      skills: splitProfileList(profileSkillsDraft),
      extensions: splitProfileList(profileExtensionsDraft),
      prompts: splitProfileList(profilePromptsDraft),
      themes: splitProfileList(profileThemesDraft),
      extraSettings,
      createdAt: existingProfile?.createdAt ?? timestamp,
      updatedAt: timestamp,
      lastUsedAt: existingProfile?.lastUsedAt,
    };

    try {
      await invoke("pi_profile_save", { profile });
      await loadPiProfiles();
      launchProfileId = profile.id;
      populateProfileDraft(profileById(profile.id));
      profileError = "";
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
    }
  }

  async function deleteSelectedProfile() {
    if (!selectedProfileId || selectedProfileId === DEFAULT_PI_PROFILE_ID) return;

    try {
      await invoke("pi_profile_delete", { profileId: selectedProfileId });
      await loadPiProfiles();
      populateProfileDraft(profileById(favoritePiProfileId) ?? piProfiles[0]);
      profileError = "";
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
    }
  }

  async function revealSelectedProfileDir() {
    if (!selectedProfileId) return;

    try {
      await invoke<string>("pi_profile_reveal_dir", { profileId: selectedProfileId });
      profileError = "";
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
    }
  }

  async function revealSelectedProfileResourceDir(resourceType: PiProfileResourceType) {
    if (!selectedProfileId || profileResourceFolderOpening) return;

    profileResourceFolderOpening = resourceType;
    profileError = "";

    try {
      await invoke<string>("pi_profile_reveal_resource_dir", {
        profileId: selectedProfileId,
        resourceType,
      });
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
    } finally {
      profileResourceFolderOpening = null;
    }
  }



  async function migrateVanillaPiAddons() {
    if (piAddonBusy) return;

    profileError = "";
    piVanillaMigrationRunning = true;

    await tick();
    try {
      const result = await invoke<PiVanillaMigrationResult>("pi_vanilla_migrate", {
        targetProfileId: selectedProfileId || DEFAULT_PI_PROFILE_ID,
      });
      await loadPiProfiles();
      await loadPiAddons();
      populateProfileDraft(profileById(result.targetProfileId) ?? profileById(favoritePiProfileId) ?? piProfiles[0]);
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
    } finally {
      piVanillaMigrationRunning = false;
    }
  }

  function openInstallAddonCommandPalette() {
    profileError = "";
    profileDialogOpen = false;
    commandPaletteOpen = true;
  }

  async function installSharedPiAddonFromCommand(packageCommand: string) {
    const packageSource = packageCommand.trim();
    if (!packageSource) return;

    profileError = "";
    await loadPiAddons();
    populateProfileDraft(profileById(favoritePiProfileId) ?? piProfiles[0]);
    profileDialogOpen = true;
    await tick();
    await runProfilePackageCommand("install", packageSource);
  }
  async function runProfilePackageCommand(command: "install" | "remove" | "update" | "list", source?: string): Promise<boolean> {
    if (piAddonBusy) return false;
    const packageSource = source?.trim() ?? "";
    if ((command === "install" || command === "remove") && !packageSource) {
      profileError = "Package source is required.";
      return false;
    }

    const normalizedPackageSource = normalizePiPackageSourceInput(packageSource);
    profileError = "";
    profilePackageCommandRunning = true;

    await tick();
    try {
      await invoke("pi_addons_package_command", {
        command,
        source: packageSource || null,
      });

      await loadPiAddons();

      if (command === "install" && normalizedPackageSource) {
        const installedPackage = piAddonPackages.find((addonPackage) => addonPackage.source === normalizedPackageSource);
        if (installedPackage && !profilePackageSelected(installedPackage.source)) {
          profileSelectedPackageSources = [...profileSelectedPackageSources, installedPackage.source];
        }
      } else if (command === "remove" && normalizedPackageSource) {
        profileSelectedPackageSources = profileSelectedPackageSources.filter((selectedSource) => selectedSource !== normalizedPackageSource);
      }
      return true;
    } catch (error) {
      profileError = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      profilePackageCommandRunning = false;
    }
  }

  function createWorkspaceId() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return `workspace-${crypto.randomUUID()}`;
    }

    return `workspace-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }

  function normalizeWorkspaceTerminals(value: unknown): SavedWorkspaceTerminal[] {
    if (!Array.isArray(value)) return [];

    return value.flatMap((terminal) => {
      if (!terminal || typeof terminal !== "object") return [];

      const record = terminal as Record<string, unknown>;
      const kind = record.kind;
      if (!isTerminalKind(kind)) return [];

      const piProfileId = kind === "pi" ? normalizeProfileId(record.piProfileId) : undefined;
      return [{ kind, piProfileId }];
    });
  }

  function normalizeSavedWorkspaces(value: unknown): SavedWorkspace[] {
    if (!Array.isArray(value)) return [];

    return value.flatMap((workspace) => {
      if (!workspace || typeof workspace !== "object") return [];

      const record = workspace as Record<string, unknown>;
      const name = typeof record.name === "string" ? record.name.trim() : "";
      if (!name) return [];

      const savedAt = typeof record.savedAt === "string" ? record.savedAt : new Date().toISOString();
      const lastUsedAt = typeof record.lastUsedAt === "string" ? record.lastUsedAt : savedAt;
      const workingDirectory = normalizeWorkingDirectory(record.workingDirectory);
      return [
        {
          id: typeof record.id === "string" && record.id ? record.id : createWorkspaceId(),
          name,
          workingDirectory: workingDirectory || undefined,
          terminals: normalizeWorkspaceTerminals(record.terminals),
          savedAt,
          lastUsedAt,
        },
      ];
    });
  }

  async function loadSavedWorkspaces() {
    try {
      workspaceStore = await Store.load(WORKSPACES_STORE_PATH);
      savedWorkspaces = normalizeSavedWorkspaces(await workspaceStore.get<unknown>(WORKSPACES_KEY));
      workspacePersistenceReady = true;
      return;
    } catch {
      workspaceStore = null;
    }

    try {
      const rawWorkspaces = localStorage.getItem(FALLBACK_STORAGE_KEY);
      savedWorkspaces = normalizeSavedWorkspaces(rawWorkspaces ? JSON.parse(rawWorkspaces) : []);
    } catch {
      savedWorkspaces = [];
    } finally {
      workspacePersistenceReady = true;
    }
  }

  async function persistSavedWorkspaces(workspaces: SavedWorkspace[]) {
    if (workspaceStore) {
      try {
        await workspaceStore.set(WORKSPACES_KEY, workspaces);
        await workspaceStore.save();
        return;
      } catch {
        workspaceStore = null;
      }
    }

    localStorage.setItem(FALLBACK_STORAGE_KEY, JSON.stringify(workspaces));
  }

  function workspaceRuntimeById(instanceId: string | null | undefined) {
    return instanceId ? runningWorkspaces.find((workspace) => workspace.instanceId === instanceId) : undefined;
  }

  function activeWorkspaceRuntime() {
    return workspaceRuntimeById(activeWorkspaceInstanceId) ?? runningWorkspaces[0] ?? null;
  }

  function ensureActiveWorkspaceRuntime() {
    const existingWorkspace = activeWorkspaceRuntime();
    if (existingWorkspace) return existingWorkspace;

    const workspace = createWorkspaceRuntime();
    runningWorkspaces = [workspace];
    activeWorkspaceInstanceId = workspace.instanceId;
    selectedTerminalId = workspace.selectedTerminalId;
    fullscreenTerminalId = workspace.fullscreenTerminalId;
    cachedTerminalReady = workspace.cachedTerminalReady;
    return workspace;
  }

  function visibleTerminalsForWorkspace(instanceId: string) {
    return terminalSessions.filter(
      (terminal) => terminal.workspaceInstanceId === instanceId && !terminal.cached && !terminal.closing,
    );
  }

  function workspaceTerminalCount(instanceId: string) {
    return visibleTerminalsForWorkspace(instanceId).length;
  }

  function terminalDndItemId(terminalId: number) {
    return `terminal-${terminalId}`;
  }

  function terminalDndItemsForSessions(sessions: TerminalSession[]): TerminalDndItem[] {
    return sessions.map((terminal) => ({
      id: terminalDndItemId(terminal.id),
      terminalId: terminal.id,
      kind: terminal.kind,
      piProfileId: terminal.kind === "pi" ? normalizeProfileId(terminal.piProfileId) : undefined,
    }));
  }

  function terminalDndItemIsShadow(item: TerminalDndItem) {
    return Boolean(item[SHADOW_ITEM_MARKER_PROPERTY_NAME]);
  }

  function terminalDndItemLabel(item: TerminalDndItem) {
    const terminal = terminalSessions.find((candidate) => candidate.id === item.terminalId);
    if (!terminal) return "terminal";

    return terminal.kind === "pi"
      ? `${profileDisplayName(terminal.piProfileId)} PI instance`
      : `Terminal ${terminal.id}`;
  }

  function normalizeTerminalDndItems(items: TerminalDndItem[], instanceId: string) {
    const terminalById = new Map(
      visibleTerminalsForWorkspace(instanceId).map((terminal) => [terminal.id, terminal]),
    );

    return items.flatMap((item) => {
      const terminal = terminalById.get(item.terminalId);
      if (!terminal) return [];

      const [normalizedItem] = terminalDndItemsForSessions([terminal]);
      return [
        {
          ...normalizedItem,
          id: item.id || normalizedItem.id,
          [SHADOW_ITEM_MARKER_PROPERTY_NAME]: terminalDndItemIsShadow(item) || undefined,
        },
      ];
    });
  }

  function sameTerminalIdOrder(left: number[], right: number[]) {
    return left.length === right.length && left.every((terminalId, index) => terminalId === right[index]);
  }

  function terminalDndOrderedTerminalIds(items: TerminalDndItem[], instanceId: string) {
    const availableTerminals = visibleTerminalsForWorkspace(instanceId);
    const availableIds = new Set(availableTerminals.map((terminal) => terminal.id));
    const orderedIds: number[] = [];
    const orderedIdSet = new Set<number>();

    for (const item of items) {
      if (!availableIds.has(item.terminalId) || orderedIdSet.has(item.terminalId)) continue;

      orderedIds.push(item.terminalId);
      orderedIdSet.add(item.terminalId);
    }

    for (const terminal of availableTerminals) {
      if (!orderedIdSet.has(terminal.id)) {
        orderedIds.push(terminal.id);
      }
    }

    return orderedIds;
  }

  function applyTerminalOrderForWorkspace(instanceId: string, orderedTerminalIds: number[], markDirty = false) {
    const visibleTerminals = visibleTerminalsForWorkspace(instanceId);
    if (visibleTerminals.length === 0) return;

    const terminalById = new Map(visibleTerminals.map((terminal) => [terminal.id, terminal]));
    const orderedTerminals = orderedTerminalIds.flatMap((terminalId) => {
      const terminal = terminalById.get(terminalId);
      return terminal ? [terminal] : [];
    });
    const orderedIdSet = new Set(orderedTerminals.map((terminal) => terminal.id));
    const nextVisibleTerminals = [
      ...orderedTerminals,
      ...visibleTerminals.filter((terminal) => !orderedIdSet.has(terminal.id)),
    ];
    const currentOrder = visibleTerminals.map((terminal) => terminal.id);
    const nextOrder = nextVisibleTerminals.map((terminal) => terminal.id);

    if (!sameTerminalIdOrder(currentOrder, nextOrder)) {
      const nextVisibleQueue = [...nextVisibleTerminals];
      terminalSessions = terminalSessions.map((terminal) =>
        terminal.workspaceInstanceId === instanceId && !terminal.cached && !terminal.closing
          ? (nextVisibleQueue.shift() ?? terminal)
          : terminal,
      );
    }

    if (markDirty) {
      updateWorkspaceRuntime(instanceId, (workspace) => ({
        ...workspace,
        dirty: true,
      }));
    }
  }

  function beginTerminalDndIfNeeded(instanceId: string) {
    if (terminalDndActive) return;

    terminalDndActive = true;
    terminalDndWorkspaceInstanceId = instanceId;
    terminalDndInitialTerminalIds = visibleTerminalsForWorkspace(instanceId).map((terminal) => terminal.id);
  }

  function handleTerminalDndConsider(event: CustomEvent<DndEvent<TerminalDndItem>>) {
    const instanceId = terminalDndWorkspaceInstanceId ?? activeWorkspaceInstanceId;
    beginTerminalDndIfNeeded(instanceId);

    const expectedTerminalCount = visibleTerminalsForWorkspace(instanceId).length;
    const nextItems = normalizeTerminalDndItems(event.detail.items, instanceId);
    if (nextItems.length !== expectedTerminalCount) return;

    terminalDndItems = nextItems;
    applyTerminalOrderForWorkspace(instanceId, terminalDndOrderedTerminalIds(nextItems, instanceId));
  }

  function handleTerminalDndFinalize(event: CustomEvent<DndEvent<TerminalDndItem>>) {
    const instanceId = terminalDndWorkspaceInstanceId ?? activeWorkspaceInstanceId;
    beginTerminalDndIfNeeded(instanceId);

    const expectedTerminalCount = visibleTerminalsForWorkspace(instanceId).length;
    const eventItems = normalizeTerminalDndItems(event.detail.items, instanceId);
    const nextItems = eventItems.length === expectedTerminalCount ? eventItems : terminalDndItems;
    const finalTerminalIds = terminalDndOrderedTerminalIds(nextItems, instanceId);
    const orderChanged = !sameTerminalIdOrder(terminalDndInitialTerminalIds, finalTerminalIds);
    applyTerminalOrderForWorkspace(instanceId, finalTerminalIds, orderChanged);

    terminalDndActive = false;
    terminalDndWorkspaceInstanceId = null;
    terminalDndInitialTerminalIds = [];
    terminalDndItems = terminalDndItemsForSessions(terminalSessions.filter(terminalIsLayoutVisible));
  }

  function updateWorkspaceRuntime(
    instanceId: string,
    updater: (workspace: WorkspaceRuntime) => WorkspaceRuntime,
  ) {
    const nextRunningWorkspaces = runningWorkspaces.map((workspace) =>
      workspace.instanceId === instanceId ? updater(workspace) : workspace,
    );
    const updatedActiveWorkspace = nextRunningWorkspaces.find(
      (workspace) => workspace.instanceId === activeWorkspaceInstanceId,
    );

    runningWorkspaces = nextRunningWorkspaces;

    if (instanceId === activeWorkspaceInstanceId && updatedActiveWorkspace) {
      selectedTerminalId = updatedActiveWorkspace.selectedTerminalId;
      fullscreenTerminalId = updatedActiveWorkspace.fullscreenTerminalId;
      cachedTerminalReady = updatedActiveWorkspace.cachedTerminalReady;
    }
  }

  function updateActiveWorkspaceRuntime(updater: (workspace: WorkspaceRuntime) => WorkspaceRuntime) {
    const workspace = ensureActiveWorkspaceRuntime();
    updateWorkspaceRuntime(workspace.instanceId, updater);
  }

  function syncActiveWorkspaceRuntime() {
    updateActiveWorkspaceRuntime((workspace) => ({
      ...workspace,
      selectedTerminalId,
      fullscreenTerminalId,
      cachedTerminalReady,
    }));
  }

  function activateWorkspace(instanceId: string) {
    const workspace = workspaceRuntimeById(instanceId);
    if (!workspace) return;

    const visibleTerminals = visibleTerminalsForWorkspace(instanceId);
    const selectedId = visibleTerminals.some((terminal) => terminal.id === workspace.selectedTerminalId)
      ? workspace.selectedTerminalId
      : (visibleTerminals[0]?.id ?? null);
    const fullscreenId = visibleTerminals.some((terminal) => terminal.id === workspace.fullscreenTerminalId)
      ? workspace.fullscreenTerminalId
      : null;

    activeWorkspaceInstanceId = workspace.instanceId;
    selectedTerminalId = selectedId;
    fullscreenTerminalId = fullscreenId;
    cachedTerminalReady = workspace.cachedTerminalReady;
    updateWorkspaceRuntime(workspace.instanceId, (currentWorkspace) => ({
      ...currentWorkspace,
      selectedTerminalId: selectedId,
      fullscreenTerminalId: fullscreenId,
    }));
  }

  function nextUntitledWorkspaceName() {
    const names = new Set(runningWorkspaces.map((workspace) => workspace.name));
    if (!names.has("Untitled")) return "Untitled";

    let suffix = 2;
    while (names.has(`Untitled ${suffix}`)) {
      suffix += 1;
    }

    return `Untitled ${suffix}`;
  }

  function createNewWorkspace() {
    const workspace = createWorkspaceRuntime({ name: nextUntitledWorkspaceName() });
    runningWorkspaces = [...runningWorkspaces, workspace];
    activateWorkspace(workspace.instanceId);
  }

  function requestCloseWorkspace(instanceId: string) {
    const workspace = workspaceRuntimeById(instanceId);
    if (!workspace) return;

    const openTerminals = visibleTerminalsForWorkspace(instanceId);
    const terminalCount = openTerminals.length;
    if (openTerminals.some((terminal) => terminal.hasUserInput)) {
      const shouldClose = window.confirm(
        `Closing ${workspace.name} will stop ${pluralize(terminalCount, "terminal")} and lose any in-progress work. Close it anyway?`,
      );
      if (!shouldClose) return;
    }

    const workspaceIndex = runningWorkspaces.findIndex((candidate) => candidate.instanceId === instanceId);
    const wasActive = activeWorkspaceInstanceId === instanceId;
    let nextRunningWorkspaces = runningWorkspaces.filter(
      (candidate) => candidate.instanceId !== instanceId,
    );
    if (nextRunningWorkspaces.length === 0) {
      nextRunningWorkspaces = [createWorkspaceRuntime()];
    }

    terminalSessions = terminalSessions.filter((terminal) => terminal.workspaceInstanceId !== instanceId);
    runningWorkspaces = nextRunningWorkspaces;

    if (wasActive || !workspaceRuntimeById(activeWorkspaceInstanceId)) {
      const nextWorkspace = nextRunningWorkspaces[Math.min(workspaceIndex, nextRunningWorkspaces.length - 1)]
        ?? nextRunningWorkspaces[0];
      activateWorkspace(nextWorkspace.instanceId);
    }
  }

  function currentWorkspaceTerminals(): SavedWorkspaceTerminal[] {
    return terminalSessions
      .filter(
        (terminal) =>
          terminal.workspaceInstanceId === activeWorkspaceInstanceId && !terminal.cached && !terminal.closing,
      )
      .map((terminal) => ({
        kind: terminal.kind,
        piProfileId: terminal.kind === "pi" ? normalizeProfileId(terminal.piProfileId) : undefined,
      }));
  }

  function sameWorkspaceName(workspace: SavedWorkspace, name: string) {
    return workspace.name.localeCompare(name, undefined, { sensitivity: "accent" }) === 0;
  }

  function activeWorkspaceName() {
    return activeWorkspaceRuntime()?.name ?? "Untitled";
  }

  function activeWorkspaceWorkingDirectory() {
    return activeWorkspaceRuntime()?.workingDirectory ?? "";
  }

  function displayWorkingDirectory(workingDirectory?: string) {
    return normalizeWorkingDirectory(workingDirectory) || "Default directory";
  }

  function sameWorkingDirectory(left?: string, right?: string) {
    return normalizeWorkingDirectory(left) === normalizeWorkingDirectory(right);
  }

  function terminalIsLayoutVisible(terminal: TerminalSession) {
    return (
      !terminal.cached &&
      !terminal.closing &&
      terminal.workspaceInstanceId === activeWorkspaceInstanceId &&
      (fullscreenTerminalId === null || terminal.id === fullscreenTerminalId)
    );
  }

  function terminalIsSelected(terminal: TerminalSession) {
    return terminal.workspaceInstanceId === activeWorkspaceInstanceId && selectedTerminalId === terminal.id;
  }

  function migrateOpenTerminalsToWorkingDirectory(workingDirectory?: string) {
    const workspace = ensureActiveWorkspaceRuntime();
    const normalizedWorkingDirectory = normalizeWorkingDirectory(workingDirectory);
    const visibleTerminals = visibleTerminalsForWorkspace(workspace.instanceId);
    if (visibleTerminals.length === 0) {
      updateActiveWorkspaceRuntime((currentWorkspace) => ({
        ...currentWorkspace,
        workingDirectory: normalizedWorkingDirectory || undefined,
      }));
      return;
    }

    const hasWorkingDirectoryChange = terminalSessions.some(
      (terminal) =>
        terminal.workspaceInstanceId === workspace.instanceId &&
        !terminal.closing &&
        !sameWorkingDirectory(terminal.workingDirectory, normalizedWorkingDirectory),
    );
    if (!hasWorkingDirectoryChange) return;

    const terminalIdMap = new Map<number, number>();
    const migratedTerminals: TerminalSession[] = visibleTerminals.map((terminal) => {
      const nextId = ++nextTerminalId;
      terminalIdMap.set(terminal.id, nextId);

      return {
        id: nextId,
        workspaceInstanceId: workspace.instanceId,
        cached: false,
        kind: terminal.kind,
        piProfileId: terminal.kind === "pi" ? normalizeProfileId(terminal.piProfileId) : undefined,
        workingDirectory: normalizedWorkingDirectory || undefined,
      };
    });
    const nextWorkspaceTerminals = [...migratedTerminals];
    if (PREWARM_FAVORITE_PI_PROFILE) {
      nextWorkspaceTerminals.push({
        id: ++nextTerminalId,
        workspaceInstanceId: workspace.instanceId,
        cached: true,
        kind: "pi",
        piProfileId: favoritePiProfileId || DEFAULT_PI_PROFILE_ID,
        workingDirectory: normalizedWorkingDirectory || undefined,
      });
    }

    terminalSessions = [
      ...terminalSessions.filter((terminal) => terminal.workspaceInstanceId !== workspace.instanceId),
      ...nextWorkspaceTerminals,
    ];
    selectedTerminalId = selectedTerminalId === null ? null : (terminalIdMap.get(selectedTerminalId) ?? null);
    fullscreenTerminalId = fullscreenTerminalId === null ? null : (terminalIdMap.get(fullscreenTerminalId) ?? null);
    cachedTerminalReady = false;
    updateActiveWorkspaceRuntime((currentWorkspace) => ({
      ...currentWorkspace,
      workingDirectory: normalizedWorkingDirectory || undefined,
      selectedTerminalId,
      fullscreenTerminalId,
      cachedTerminalReady: false,
    }));
  }

  function openSaveWorkspaceDialog() {
    if (!workspacePersistenceReady || activeTerminalCount === 0) return;

    openWorkspaceDialogOpen = false;
    workspaceNameDraft = activeWorkspaceName();
    workspaceDirectoryDraft = activeWorkspaceWorkingDirectory();
    saveWorkspaceDialogOpen = true;
  }

  function openWorkspaceDialog() {
    if (!workspacePersistenceReady) return;

    saveWorkspaceDialogOpen = false;
    openWorkspaceDialogOpen = true;
  }
  async function selectWorkspaceDirectory() {
    const selectedDirectory = await open({ directory: true, multiple: false });

    if (typeof selectedDirectory === "string") {
      workspaceDirectoryDraft = selectedDirectory;
    }
  }

  function pluralize(count: number, label: string) {
    return `${count} ${label}${count === 1 ? "" : "s"}`;
  }

  function workspaceSummary(workspace: SavedWorkspace) {
    const piTerminals = workspace.terminals.filter((terminal) => terminal.kind === "pi");
    const shellCount = workspace.terminals.filter((terminal) => terminal.kind === "shell").length;
    const parts: string[] = [];

    if (piTerminals.length > 0) {
      const profileNames = [...new Set(piTerminals.map((terminal) => profileDisplayName(terminal.piProfileId)))];
      parts.push(`${pluralize(piTerminals.length, "PI")} (${profileNames.join(", ")})`);
    }

    if (shellCount > 0) {
      parts.push(pluralize(shellCount, "terminal"));
    }

    return parts.length > 0 ? parts.join(" · ") : "No terminals";
  }


  function touchWorkspaceUsage(workspaceId: string) {
    const lastUsedAt = new Date().toISOString();
    const nextSavedWorkspaces = savedWorkspaces.map((workspace) =>
      workspace.id === workspaceId ? { ...workspace, lastUsedAt } : workspace,
    );

    savedWorkspaces = nextSavedWorkspaces;
    void persistSavedWorkspaces(nextSavedWorkspaces);
  }
  function deleteSavedWorkspace(workspaceId: string) {
    const nextSavedWorkspaces = savedWorkspaces.filter((workspace) => workspace.id !== workspaceId);

    savedWorkspaces = nextSavedWorkspaces;
    runningWorkspaces = runningWorkspaces.map((workspace) =>
      workspace.savedWorkspaceId === workspaceId
        ? { ...workspace, savedWorkspaceId: undefined, dirty: true }
        : workspace,
    );

    void persistSavedWorkspaces(nextSavedWorkspaces);
  }

  function openSavedWorkspace(workspace: SavedWorkspace) {
    const alreadyRunningWorkspace = runningWorkspaces.find(
      (runtimeWorkspace) => runtimeWorkspace.savedWorkspaceId === workspace.id,
    );
    if (alreadyRunningWorkspace) {
      activateWorkspace(alreadyRunningWorkspace.instanceId);
      touchWorkspaceUsage(workspace.id);
      openWorkspaceDialogOpen = false;
      return;
    }

    const reusableWorkspace = activeWorkspaceRuntime();
    const shouldReuseActiveWorkspace = Boolean(
      reusableWorkspace &&
        !reusableWorkspace.savedWorkspaceId &&
        !reusableWorkspace.dirty &&
        workspaceTerminalCount(reusableWorkspace.instanceId) === 0,
    );
    const instanceId = shouldReuseActiveWorkspace && reusableWorkspace
      ? reusableWorkspace.instanceId
      : createWorkspaceInstanceId();
    const workingDirectory = normalizeWorkingDirectory(workspace.workingDirectory);
    const visibleTerminals: TerminalSession[] = workspace.terminals.map((terminal) => ({
      id: ++nextTerminalId,
      workspaceInstanceId: instanceId,
      cached: false,
      kind: terminal.kind,
      piProfileId: terminal.kind === "pi" ? normalizeProfileId(terminal.piProfileId) : undefined,
      workingDirectory: workingDirectory || undefined,
    }));
    const nextWorkspaceTerminals = [...visibleTerminals];
    if (PREWARM_FAVORITE_PI_PROFILE) {
      nextWorkspaceTerminals.push({
        id: ++nextTerminalId,
        workspaceInstanceId: instanceId,
        cached: true,
        kind: "pi",
        piProfileId: favoritePiProfileId || DEFAULT_PI_PROFILE_ID,
        workingDirectory: workingDirectory || undefined,
      });
    }

    terminalSessions = [
      ...terminalSessions.filter((terminal) =>
        shouldReuseActiveWorkspace ? terminal.workspaceInstanceId !== instanceId : true,
      ),
      ...nextWorkspaceTerminals,
    ];
    const runtimeWorkspace = createWorkspaceRuntime({
      instanceId,
      savedWorkspaceId: workspace.id,
      name: workspace.name,
      workingDirectory: workingDirectory || undefined,
      selectedTerminalId: visibleTerminals[0]?.id ?? null,
      cachedTerminalReady: false,
    });
    runningWorkspaces = shouldReuseActiveWorkspace
      ? runningWorkspaces.map((candidate) =>
          candidate.instanceId === instanceId ? runtimeWorkspace : candidate,
        )
      : [...runningWorkspaces, runtimeWorkspace];
    activateWorkspace(instanceId);
    touchWorkspaceUsage(workspace.id);
    openWorkspaceDialogOpen = false;
  }

  function submitSaveWorkspace(event: SubmitEvent) {
    event.preventDefault();

    const workspaceName = workspaceNameDraft.trim();
    if (!workspaceName || activeTerminalCount === 0) return;

    void saveCurrentWorkspace(workspaceName, workspaceDirectoryDraft);
  }

  async function saveCurrentWorkspace(name: string, workingDirectoryDraft: string) {
    const terminals = currentWorkspaceTerminals();
    if (terminals.length === 0) return;

    const activeRuntime = ensureActiveWorkspaceRuntime();
    const workingDirectory = normalizeWorkingDirectory(workingDirectoryDraft);
    const timestamp = new Date().toISOString();
    const existingByName = savedWorkspaces.find((workspace) => sameWorkspaceName(workspace, name));
    const activeSavedWorkspace = activeRuntime.savedWorkspaceId
      ? savedWorkspaces.find((workspace) => workspace.id === activeRuntime.savedWorkspaceId)
      : undefined;
    const targetWorkspaceId =
      existingByName?.id ?? (activeSavedWorkspace && sameWorkspaceName(activeSavedWorkspace, name) ? activeSavedWorkspace.id : createWorkspaceId());
    const workspace: SavedWorkspace = {
      id: targetWorkspaceId,
      name,
      workingDirectory: workingDirectory || undefined,
      terminals,
      savedAt: timestamp,
      lastUsedAt: timestamp,
    };
    let updatedExistingWorkspace = false;
    let nextSavedWorkspaces = savedWorkspaces.map((savedWorkspace) => {
      if (savedWorkspace.id !== targetWorkspaceId) return savedWorkspace;

      updatedExistingWorkspace = true;
      return workspace;
    });

    if (!updatedExistingWorkspace) {
      nextSavedWorkspaces = [...nextSavedWorkspaces, workspace];
    }

    savedWorkspaces = nextSavedWorkspaces;

    try {
      await persistSavedWorkspaces(nextSavedWorkspaces);
      updateActiveWorkspaceRuntime((currentWorkspace) => ({
        ...currentWorkspace,
        savedWorkspaceId: targetWorkspaceId,
        name,
        workingDirectory: workingDirectory || undefined,
        dirty: false,
      }));
      migrateOpenTerminalsToWorkingDirectory(workingDirectory);
      saveWorkspaceDialogOpen = false;
      workspaceNameDraft = "";
      workspaceDirectoryDraft = "";
    } catch {
      saveWorkspaceDialogOpen = false;
    }
  }

  function spawnTerminal(profileId = favoritePiProfileId, allowCold = false) {
    const workspace = ensureActiveWorkspaceRuntime();
    const workingDirectory = normalizeWorkingDirectory(workspace.workingDirectory);
    const normalizedProfileId = normalizeProfileId(profileId);
    const favoriteProfileId = favoritePiProfileId || DEFAULT_PI_PROFILE_ID;

    if (
      PREWARM_FAVORITE_PI_PROFILE &&
      !allowCold &&
      samePiProfile(normalizedProfileId, favoriteProfileId) &&
      !cachedTerminalReady
    ) {
      return;
    }

    const cachedTerminal = PREWARM_FAVORITE_PI_PROFILE
      ? terminalSessions.find(
          (terminal) =>
            terminal.workspaceInstanceId === workspace.instanceId &&
            terminal.cached &&
            sameWorkingDirectory(terminal.workingDirectory, workingDirectory) &&
            samePiProfile(terminal.piProfileId, normalizedProfileId),
        )
      : undefined;

    if (!cachedTerminal) {
      const nextVisibleTerminal: TerminalSession = {
        id: ++nextTerminalId,
        workspaceInstanceId: workspace.instanceId,
        cached: false,
        kind: "pi",
        piProfileId: normalizedProfileId,
        workingDirectory: workingDirectory || undefined,
      };
      const nextTerminalSessions = [
        ...terminalSessions.filter(
          (terminal) => !(terminal.workspaceInstanceId === workspace.instanceId && terminal.cached),
        ),
        nextVisibleTerminal,
      ];

      if (PREWARM_FAVORITE_PI_PROFILE) {
        nextTerminalSessions.push({
          id: ++nextTerminalId,
          workspaceInstanceId: workspace.instanceId,
          cached: true,
          kind: "pi",
          piProfileId: favoriteProfileId,
          workingDirectory: workingDirectory || undefined,
        });
        cachedTerminalReady = false;
      }

      terminalSessions = nextTerminalSessions;
      selectedTerminalId = nextVisibleTerminal.id;
    } else {
      terminalSessions = terminalSessions
        .filter(
          (terminal) =>
            !(terminal.workspaceInstanceId === workspace.instanceId && terminal.cached && terminal.id !== cachedTerminal.id),
        )
        .map((terminal) => (terminal.id === cachedTerminal.id ? { ...terminal, cached: false } : terminal));
      if (PREWARM_FAVORITE_PI_PROFILE) {
        terminalSessions = [
          ...terminalSessions,
          {
            id: ++nextTerminalId,
            workspaceInstanceId: workspace.instanceId,
            cached: true,
            kind: "pi",
            piProfileId: favoriteProfileId,
            workingDirectory: workingDirectory || undefined,
          },
        ];
      }
      selectedTerminalId = cachedTerminal.id;
      cachedTerminalReady = false;
    }

    fullscreenTerminalId = null;
    updateActiveWorkspaceRuntime((currentWorkspace) => ({
      ...currentWorkspace,
      selectedTerminalId,
      fullscreenTerminalId: null,
      cachedTerminalReady,
      dirty: true,
    }));
  }

  function spawnEmptyTerminal() {
    const workspace = ensureActiveWorkspaceRuntime();
    const workingDirectory = normalizeWorkingDirectory(workspace.workingDirectory);
    const terminal: TerminalSession = {
      id: ++nextTerminalId,
      workspaceInstanceId: workspace.instanceId,
      cached: false,
      kind: "shell",
      workingDirectory: workingDirectory || undefined,
    };
    terminalSessions = [...terminalSessions, terminal];
    selectedTerminalId = terminal.id;
    fullscreenTerminalId = null;
    updateActiveWorkspaceRuntime((currentWorkspace) => ({
      ...currentWorkspace,
      selectedTerminalId: terminal.id,
      fullscreenTerminalId: null,
      dirty: true,
    }));
  }
  function handleTerminalReady(id: number) {
    const session = terminalSessions.find((terminal) => terminal.id === id);

    if (PREWARM_FAVORITE_PI_PROFILE && session?.cached && samePiProfile(session.piProfileId, favoritePiProfileId)) {
      updateWorkspaceRuntime(session.workspaceInstanceId, (workspace) => ({
        ...workspace,
        cachedTerminalReady: true,
      }));
    }
  }
  function handleTerminalUserInput(id: number) {
    terminalSessions = terminalSessions.map((candidate) =>
      candidate.id === id && !candidate.cached && !candidate.hasUserInput
        ? { ...candidate, hasUserInput: true }
        : candidate,
    );
  }

  function dismissCloseConfirmation() {
    closeConfirmationOpen = false;
    pendingCloseTerminalId = null;
  }

  function confirmPendingTerminalClose() {
    const id = pendingCloseTerminalId;
    dismissCloseConfirmation();

    if (id === null) return;

    const stillOpen = terminalSessions.some(
      (terminal) => terminal.id === id && !terminal.cached && !terminal.closing,
    );
    if (stillOpen) {
      closeTerminal(id);
    }
  }

  function closeTerminal(id: number) {
    const terminal = terminalSessions.find(
      (candidate) => candidate.id === id && !candidate.cached && !candidate.closing,
    );
    if (!terminal) return;

    terminalSessions = terminalSessions.map((candidate) =>
      candidate.id === id && !candidate.cached ? { ...candidate, closing: true } : candidate,
    );

    updateWorkspaceRuntime(terminal.workspaceInstanceId, (workspace) => ({
      ...workspace,
      selectedTerminalId: workspace.selectedTerminalId === id ? null : workspace.selectedTerminalId,
      fullscreenTerminalId: workspace.fullscreenTerminalId === id ? null : workspace.fullscreenTerminalId,
      dirty: true,
    }));
  }

  function requestCloseTerminal(id: number) {
    const terminal = terminalSessions.find(
      (terminal) => terminal.id === id && !terminal.cached && !terminal.closing,
    );
    if (!terminal) return;

    if (terminal.hasUserInput) {
      pendingCloseTerminalId = id;
      closeConfirmationOpen = true;
      return;
    }

    closeTerminal(id);
  }

  function closeNewestTerminal() {
    const newestTerminal = [...terminalSessions]
      .reverse()
      .find(
        (terminal) =>
          terminal.workspaceInstanceId === activeWorkspaceInstanceId && !terminal.cached && !terminal.closing,
      );

    if (newestTerminal) {
      requestCloseTerminal(newestTerminal.id);
    }
  }

  function closeSelectedOrNewestTerminal() {
    const selectedTerminal = terminalSessions.find(
      (terminal) =>
        terminal.workspaceInstanceId === activeWorkspaceInstanceId &&
        terminal.id === selectedTerminalId &&
        !terminal.cached &&
        !terminal.closing,
    );

    if (selectedTerminal) {
      requestCloseTerminal(selectedTerminal.id);
      return;
    }

    closeNewestTerminal();
  }

  function toggleFullscreenSelectedTerminal() {
    if (fullscreenTerminalId !== null) {
      fullscreenTerminalId = null;
      syncActiveWorkspaceRuntime();
      return;
    }

    if (activeTerminalCount <= 1) return;

    const selectedTerminal = terminalSessions.find(
      (terminal) =>
        terminal.workspaceInstanceId === activeWorkspaceInstanceId &&
        terminal.id === selectedTerminalId &&
        !terminal.cached &&
        !terminal.closing,
    );

    if (selectedTerminal) {
      fullscreenTerminalId = selectedTerminal.id;
      syncActiveWorkspaceRuntime();
    }
  }
  function selectTerminal(id: number) {
    const terminal = terminalSessions.find((terminal) => terminal.id === id);

    if (terminal && !terminal.cached && !terminal.closing) {
      if (terminal.workspaceInstanceId !== activeWorkspaceInstanceId) {
        activateWorkspace(terminal.workspaceInstanceId);
      }
      selectedTerminalId = id;
      syncActiveWorkspaceRuntime();
    }
  }
  function handleTerminalClosed(id: number) {
    const terminal = terminalSessions.find((candidate) => candidate.id === id);
    terminalSessions = terminalSessions.filter((candidate) => candidate.cached || candidate.id !== id);
    if (terminal) {
      updateWorkspaceRuntime(terminal.workspaceInstanceId, (workspace) => ({
        ...workspace,
        selectedTerminalId: workspace.selectedTerminalId === id ? null : workspace.selectedTerminalId,
        fullscreenTerminalId: workspace.fullscreenTerminalId === id ? null : workspace.fullscreenTerminalId,
      }));
    }

    if (pendingCloseTerminalId === id) {
      dismissCloseConfirmation();
    }
  }
</script>

<CommandPalette
  bind:open={commandPaletteOpen}
  commands={commandPaletteCommands}
  onInstallPackageCommand={installSharedPiAddonFromCommand}
/>
<Dialog.Root bind:open={closeConfirmationOpen}>
  <Dialog.Content class="gap-4 border-destructive/30 bg-popover/95">
    <Dialog.Header class="pr-8">
      <div class="flex items-start gap-3">
        <div
          class="flex size-9 shrink-0 items-center justify-center rounded-full border border-destructive/30 bg-destructive/10 text-sm font-semibold text-destructive"
          aria-hidden="true"
        >
          !
        </div>
        <div class="flex min-w-0 flex-col gap-1">
          <Dialog.Title>Close terminal?</Dialog.Title>
          <Dialog.Description>{TERMINAL_CLOSE_CONFIRMATION_MESSAGE}</Dialog.Description>
        </div>
      </div>
    </Dialog.Header>
    <div class="rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
      Press <span class="font-medium text-foreground">Keep open</span> to continue from where you left off.
    </div>
    <Dialog.Footer>
      <Button variant="outline" type="button" onclick={dismissCloseConfirmation}>Keep open</Button>
      <Button variant="destructive" type="button" onclick={confirmPendingTerminalClose}>Close terminal</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={saveWorkspaceDialogOpen}>
  <Dialog.Content class="gap-3">
    <form class="flex flex-col gap-3" onsubmit={submitSaveWorkspace}>
      <Dialog.Header class="pr-8">
        <Dialog.Title>Save workspace</Dialog.Title>
      </Dialog.Header>
      <div class="flex flex-col gap-2">
        <Input
          bind:value={workspaceNameDraft}
          autofocus
          aria-label="Workspace name"
          placeholder="Workspace name"
        />
        <div class="flex gap-2">
          <Input
            bind:value={workspaceDirectoryDraft}
            aria-label="Workspace working directory"
            placeholder="Working directory (optional)"
          />
          <Button variant="outline" type="button" onclick={selectWorkspaceDirectory}>Browse</Button>
        </div>
      </div>
      <Dialog.Footer>

        <Button variant="ghost" type="submit" disabled={!workspaceNameDraft.trim() || activeTerminalCount === 0}>Save</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={openWorkspaceDialogOpen}>
  <Dialog.Content class="gap-3">
    <Dialog.Header class="pr-8">
      <Dialog.Title>Workspaces</Dialog.Title>
    </Dialog.Header>
    {#if savedWorkspaces.length > 0}
      <div class="flex max-h-80 flex-col overflow-auto">
        {#each savedWorkspaces as workspace, index (workspace.id)}
          {#if index > 0}
            <Separator />
          {/if}

          <div class="relative">
            <Button
              variant="ghost"
              class="flex h-auto w-full min-w-0 items-start justify-start px-1 py-2 pr-9 text-left whitespace-normal"
              onclick={() => openSavedWorkspace(workspace)}
            >
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="truncate font-medium text-foreground">{workspace.name}</span>
                <span class="truncate text-muted-foreground">{displayWorkingDirectory(workspace.workingDirectory)}</span>
                <span class="truncate text-muted-foreground">{workspaceSummary(workspace)}</span>
              </span>
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              class="absolute right-1 top-1/2 -translate-y-1/2 text-muted-foreground"
              aria-label={`Delete ${workspace.name}`}
              onclick={() => deleteSavedWorkspace(workspace.id)}
            >
              <RiDeleteBinLine data-icon="inline-start" aria-hidden="true" />
            </Button>
          </div>
        {/each}
      </div>
    {:else}
      <p class="text-muted-foreground">No saved workspaces yet. Press CTRL+S to save the current workspace.</p>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={launchProfileDialogOpen}>
  <Dialog.Content class="gap-3">
    <Dialog.Header class="pr-8">
      <Dialog.Title>Launch Pi instance</Dialog.Title>
      <Dialog.Description>Choose a Pi profile to launch. CTRL+N launches your favourite.</Dialog.Description>
    </Dialog.Header>

    {#if piProfiles.length > 0}
      <div class="flex max-h-96 flex-col overflow-auto rounded-md border border-border">
        {#each piProfiles as profile, index (profile.id)}
          {#if index > 0}
            <Separator />
          {/if}

          <div class="relative">
            <Button
              variant="ghost"
              type="button"
              class={launchProfileId === profile.id
                ? "flex h-auto w-full min-w-0 items-start justify-start rounded-none bg-muted px-3 py-2 pr-12 text-left whitespace-normal"
                : "flex h-auto w-full min-w-0 items-start justify-start rounded-none px-3 py-2 pr-12 text-left whitespace-normal"}
              onclick={() => launchSelectedPiProfile(profile.id)}
            >
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="truncate font-medium text-foreground">{profile.name}</span>
                <span class="truncate text-muted-foreground">{profile.description ?? profile.id}</span>
              </span>
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              type="button"
              class="absolute right-1 top-1/2 -translate-y-1/2 text-muted-foreground"
              aria-label={samePiProfile(favoritePiProfileId, profile.id) ? `${profile.name} is favourite` : `Favourite ${profile.name}`}
              aria-pressed={samePiProfile(favoritePiProfileId, profile.id)}
              onclick={(event) => {
                event.stopPropagation();
                setFavoritePiProfile(profile.id);
              }}
            >
              {#if samePiProfile(favoritePiProfileId, profile.id)}
                <RiStarFill data-icon="inline-start" aria-hidden="true" />
              {:else}
                <RiStarLine data-icon="inline-start" aria-hidden="true" />
              {/if}
            </Button>
          </div>
        {/each}
      </div>
    {:else}
      <p class="text-muted-foreground">No Pi profiles are available yet.</p>
    {/if}

    <Dialog.Footer>
      <Button
        variant="outline"
        type="button"
        onclick={() => {
          launchProfileDialogOpen = false;
          openProfileDialog();
        }}
      >
        Manage profiles
      </Button>
      <Button variant="ghost" type="button" disabled={!launchProfileId || piProfiles.length === 0} onclick={() => launchSelectedPiProfile()}>
        Launch
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={profileDialogOpen}>
  <Dialog.Content class="h-[72vh] max-h-[44rem] w-[calc(100vw-2rem)] max-w-5xl gap-4 overflow-hidden p-5 sm:max-w-5xl">
    <Dialog.Header class="pr-8">
      <Dialog.Title>Manage Pi profiles</Dialog.Title>
      <Dialog.Description>Configure reusable Pi environments. Auth is shared automatically when Pi exits.</Dialog.Description>
    </Dialog.Header>

    <div class="grid min-h-0 gap-4 overflow-hidden lg:grid-cols-[18rem_1fr]">
      <aside class="flex min-h-0 flex-col gap-3 overflow-hidden rounded-md border border-border bg-muted/30 p-3">
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <p class="font-medium text-foreground">Profiles</p>
            <p class="truncate text-muted-foreground">{piProfiles.length} saved</p>
          </div>
          <Button variant="outline" size="sm" type="button" onclick={newProfileDraft}>New</Button>
        </div>

        <div class="flex min-h-0 flex-1 flex-col overflow-auto rounded-md border border-border bg-background/60">
          {#each piProfiles as profile, index (profile.id)}
            {#if index > 0}
              <Separator />
            {/if}
            <Button
              variant="ghost"
              type="button"
              class={selectedProfileId === profile.id
                ? "h-auto w-full justify-start rounded-none bg-muted px-3 py-2.5 text-left"
                : "h-auto w-full justify-start rounded-none px-3 py-2.5 text-left"}
              onclick={() => {
                profileError = "";
                populateProfileDraft(profile);
              }}
            >
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="truncate font-medium">{profile.name}</span>
                <span class="truncate text-muted-foreground">{profile.id}</span>
              </span>
            </Button>
          {/each}
        </div>

        <div class="flex flex-col gap-1 rounded-md border border-border bg-background/60 px-3 py-2">
          <span class="text-muted-foreground">Selected profile</span>
          <span class="truncate font-medium text-foreground">{selectedProfileId || "New profile"}</span>
        </div>
      </aside>

      <form class="grid min-h-0 grid-rows-[1fr_auto] gap-3 overflow-hidden" onsubmit={saveProfileDraft}>
        <div class="flex min-h-0 flex-col gap-3 overflow-auto pr-1">
          <div class="flex flex-col gap-3">
          <section class="flex flex-col gap-3 rounded-md border border-border p-3">
            <div class="flex flex-col gap-1">
              <h3 class="font-medium text-foreground">Profile details</h3>
              <p class="text-muted-foreground">Name the profile and describe when you use it.</p>
            </div>

            <div class="flex flex-col gap-3">
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Name
                <Input bind:value={profileNameDraft} aria-label="Profile name" placeholder="Profile name" />
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Description
                <Input bind:value={profileDescriptionDraft} aria-label="Profile description" placeholder="What this profile is for" />
              </label>
            </div>
          </section>

          <section class="flex flex-col gap-3 rounded-md border border-border p-3">
            <div class="flex flex-col gap-1">
              <h3 class="font-medium text-foreground">Default model</h3>
              <p class="text-muted-foreground">Optional defaults used when launching Pi with this profile.</p>
            </div>

            <div class="flex flex-col gap-3">
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Provider
                <Input bind:value={profileProviderDraft} aria-label="Default provider" placeholder="Default provider" />
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Model
                <Input bind:value={profileModelDraft} aria-label="Default model" placeholder="Default model" />
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Thinking level
                <Input bind:value={profileThinkingDraft} aria-label="Default thinking level" placeholder="Default thinking level" />
              </label>
            </div>
          </section>
          <section class="flex flex-col gap-3 rounded-md border border-border p-3">
            <div class="flex flex-col gap-1">
              <h3 class="font-medium text-foreground">Pi settings</h3>
              <p class="text-muted-foreground">Common Pi settings written to this profile's settings.json.</p>
            </div>

            <div class="grid gap-3 md:grid-cols-2">
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Theme
                <Input bind:value={profileThemeDraft} aria-label="Theme" placeholder="dark, light, or custom theme" />
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Double escape action
                <select class={PROFILE_SETTINGS_CONTROL_CLASS} bind:value={profileDoubleEscapeActionDraft} aria-label="Double escape action">
                  <option value="">Pi default (tree)</option>
                  <option value="tree">Tree</option>
                  <option value="fork">Fork</option>
                  <option value="none">None</option>
                </select>
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Tree filter mode
                <select class={PROFILE_SETTINGS_CONTROL_CLASS} bind:value={profileTreeFilterModeDraft} aria-label="Tree filter mode">
                  <option value="">Pi default (default)</option>
                  <option value="default">Default</option>
                  <option value="no-tools">No tools</option>
                  <option value="user-only">User only</option>
                  <option value="labeled-only">Labeled only</option>
                  <option value="all">All</option>
                </select>
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Transport
                <select class={PROFILE_SETTINGS_CONTROL_CLASS} bind:value={profileTransportDraft} aria-label="Transport">
                  <option value="">Pi default (auto)</option>
                  <option value="auto">Auto</option>
                  <option value="sse">SSE</option>
                  <option value="websocket">WebSocket</option>
                  <option value="websocket-cached">WebSocket cached</option>
                </select>
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Steering mode
                <select class={PROFILE_SETTINGS_CONTROL_CLASS} bind:value={profileSteeringModeDraft} aria-label="Steering mode">
                  <option value="">Pi default (one-at-a-time)</option>
                  <option value="one-at-a-time">One at a time</option>
                  <option value="all">All</option>
                </select>
              </label>
              <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
                Follow-up mode
                <select class={PROFILE_SETTINGS_CONTROL_CLASS} bind:value={profileFollowUpModeDraft} aria-label="Follow-up mode">
                  <option value="">Pi default (one-at-a-time)</option>
                  <option value="one-at-a-time">One at a time</option>
                  <option value="all">All</option>
                </select>
              </label>
            </div>

            <div class="grid gap-3 md:grid-cols-2">
              <label class="flex cursor-pointer items-start gap-2 text-xs font-medium text-muted-foreground">
                <input class="mt-1" type="checkbox" bind:checked={profileShowThinkingBlocksDraft} />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span class="text-foreground">Show thinking blocks</span>
                  <span>Display model thinking output instead of hiding it.</span>
                </span>
              </label>
              <label class="flex cursor-pointer items-start gap-2 text-xs font-medium text-muted-foreground">
                <input class="mt-1" type="checkbox" bind:checked={profileQuietStartupDraft} />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span class="text-foreground">Quiet startup</span>
                  <span>Hide the startup header.</span>
                </span>
              </label>
              <label class="flex cursor-pointer items-start gap-2 text-xs font-medium text-muted-foreground">
                <input class="mt-1" type="checkbox" bind:checked={profileCollapseChangelogDraft} />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span class="text-foreground">Collapse changelog</span>
                  <span>Show a condensed changelog after updates.</span>
                </span>
              </label>
              <label class="flex cursor-pointer items-start gap-2 text-xs font-medium text-muted-foreground">
                <input class="mt-1" type="checkbox" bind:checked={profileEnableInstallTelemetryDraft} />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span class="text-foreground">Install telemetry</span>
                  <span>Allow Pi's anonymous install/update version ping.</span>
                </span>
              </label>
              <label class="flex cursor-pointer items-start gap-2 text-xs font-medium text-muted-foreground">
                <input class="mt-1" type="checkbox" bind:checked={profileShowHardwareCursorDraft} />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span class="text-foreground">Show hardware cursor</span>
                  <span>Use the terminal cursor for IME positioning.</span>
                </span>
              </label>
              <label class="flex cursor-pointer items-start gap-2 text-xs font-medium text-muted-foreground">
                <input class="mt-1" type="checkbox" bind:checked={profileEnableSkillCommandsDraft} />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span class="text-foreground">Skill commands</span>
                  <span>Register skills as slash commands.</span>
                </span>
              </label>
            </div>

            <label class="flex min-w-0 flex-col gap-1 text-xs font-medium text-muted-foreground">
              Additional settings JSON
              <textarea
                class={PROFILE_SETTINGS_TEXTAREA_CLASS}
                bind:value={profileExtraSettingsDraft}
                aria-label="Additional Pi settings JSON"
                placeholder={PROFILE_EXTRA_SETTINGS_PLACEHOLDER}
                spellcheck="false"
              ></textarea>
              <span>Use this for advanced Pi settings that are not shown above. Known profile fields are managed by the form.</span>
            </label>
          </section>
          </div>

          <section class="flex flex-col gap-3 rounded-md border border-border p-3">
            <div class="flex flex-wrap gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  type="button"
                  disabled={piAddonBusy}
                  onclick={openInstallAddonCommandPalette}
                >
                  Install…
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  type="button"
                  disabled={piAddonBusy || !selectedProfileId}
                  onclick={() => void migrateVanillaPiAddons()}
                >
                  {piVanillaMigrationRunning ? "Importing…" : "Import into selected profile"}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  type="button"
                  disabled={piAddonBusy || piAddonPackages.length === 0}
                  onclick={() => void runProfilePackageCommand("update")}
                >
                  Update all
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  disabled={piAddonBusy}
                  onclick={() => void loadPiAddons()}
                >
                  Refresh
                </Button>
              </div>

            {#if piAddonBusy}
              <p class="text-muted-foreground">
                {piVanillaMigrationRunning
                  ? "Migrating vanilla Pi add-ons… first import can take a minute while PIOC copies your vanilla Pi npm add-on cache."
                  : piAddonGlobalSaving
                    ? "Saving global Pi add-ons…"
                    : "Running shared Pi add-on command…"}
              </p>
            {/if}

            <div class="flex flex-col gap-3">
              <div class="flex flex-col gap-3 rounded-md border border-border bg-background/60 p-3">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex min-w-0 flex-col gap-1">
                    <p class="font-medium text-foreground">Skills</p>
                    <p class="text-muted-foreground">{skillAddonPackages.length} skills · {selectedAddonCount(skillAddonPackages)} active · {globalAddonCount(skillAddonPackages)} always active</p>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    type="button"
                    aria-label="Open skills folder"
                    title="Open skills folder"
                    disabled={!selectedProfileId || profileResourceFolderOpening !== null}
                    onclick={() => void revealSelectedProfileResourceDir("skills")}
                  >
                    <RiFolderOpenLine aria-hidden="true" />
                  </Button>
                </div>

                {#if skillAddonPackages.length > 0}
                  <div class="flex max-h-40 flex-col overflow-auto rounded-md border border-border">
                    {#each skillAddonPackages as addonPackage, index (addonPackage.source)}
                      {#if index > 0}
                        <Separator />
                      {/if}

                      <div class="flex items-start gap-3 px-3 py-2 text-left">
                        <label class="flex min-w-0 flex-1 cursor-pointer items-start gap-2">
                          <input
                            class="mt-1"
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={profilePackageSelected(addonPackage.source)}
                            onchange={() => toggleProfilePackageSelection(addonPackage.source)}
                          />
                          <span class="flex min-w-0 flex-col gap-0.5">
                            <span class="truncate font-medium text-foreground">{addonPackage.source}</span>
                            {#if addonPackage.global}
                              <span class="truncate text-muted-foreground">Always active for every profile.</span>
                            {/if}
                            {#if !addonPackage.installedPath}
                              <span class="truncate text-muted-foreground">Installed path missing; update or reinstall this add-on.</span>
                            {/if}
                          </span>
                        </label>
                        <label class="flex shrink-0 cursor-pointer items-center gap-1 text-xs text-muted-foreground">
                          <input
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={addonPackage.global}
                            onchange={() => void toggleGlobalPackageSelection(addonPackage.source)}
                          />
                          Always active
                        </label>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="rounded-md border border-border bg-muted px-3 py-2 text-muted-foreground">
                    {piAddonsReady ? "No shared skill add-ons installed." : "Loading skill add-ons…"}
                  </p>
                {/if}

              </div>

              <div class="flex flex-col gap-3 rounded-md border border-border bg-background/60 p-3">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex min-w-0 flex-col gap-1">
                    <p class="font-medium text-foreground">Extensions</p>
                    <p class="text-muted-foreground">{extensionAddonPackages.length} extensions · {selectedAddonCount(extensionAddonPackages)} active · {globalAddonCount(extensionAddonPackages)} always active</p>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    type="button"
                    aria-label="Open extensions folder"
                    title="Open extensions folder"
                    disabled={!selectedProfileId || profileResourceFolderOpening !== null}
                    onclick={() => void revealSelectedProfileResourceDir("extensions")}
                  >
                    <RiFolderOpenLine aria-hidden="true" />
                  </Button>
                </div>

                {#if extensionAddonPackages.length > 0}
                  <div class="flex max-h-40 flex-col overflow-auto rounded-md border border-border">
                    {#each extensionAddonPackages as addonPackage, index (addonPackage.source)}
                      {#if index > 0}
                        <Separator />
                      {/if}

                      <div class="flex items-start gap-3 px-3 py-2 text-left">
                        <label class="flex min-w-0 flex-1 cursor-pointer items-start gap-2">
                          <input
                            class="mt-1"
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={profilePackageSelected(addonPackage.source)}
                            onchange={() => toggleProfilePackageSelection(addonPackage.source)}
                          />
                          <span class="flex min-w-0 flex-col gap-0.5">
                            <span class="truncate font-medium text-foreground">{addonPackage.source}</span>
                            {#if addonPackage.global}
                              <span class="truncate text-muted-foreground">Always active for every profile.</span>
                            {/if}
                            {#if !addonPackage.installedPath}
                              <span class="truncate text-muted-foreground">Installed path missing; update or reinstall this add-on.</span>
                            {/if}
                          </span>
                        </label>
                        <label class="flex shrink-0 cursor-pointer items-center gap-1 text-xs text-muted-foreground">
                          <input
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={addonPackage.global}
                            onchange={() => void toggleGlobalPackageSelection(addonPackage.source)}
                          />
                          Always active
                        </label>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="rounded-md border border-border bg-muted px-3 py-2 text-muted-foreground">
                    {piAddonsReady ? "No shared extension add-ons installed." : "Loading extension add-ons…"}
                  </p>
                {/if}

              </div>

              <div class="flex flex-col gap-3 rounded-md border border-border bg-background/60 p-3">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex min-w-0 flex-col gap-1">
                    <p class="font-medium text-foreground">Prompts</p>
                    <p class="text-muted-foreground">{promptAddonPackages.length} prompts · {selectedAddonCount(promptAddonPackages)} active · {globalAddonCount(promptAddonPackages)} always active</p>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    type="button"
                    aria-label="Open prompts folder"
                    title="Open prompts folder"
                    disabled={!selectedProfileId || profileResourceFolderOpening !== null}
                    onclick={() => void revealSelectedProfileResourceDir("prompts")}
                  >
                    <RiFolderOpenLine aria-hidden="true" />
                  </Button>
                </div>

                {#if promptAddonPackages.length > 0}
                  <div class="flex max-h-40 flex-col overflow-auto rounded-md border border-border">
                    {#each promptAddonPackages as addonPackage, index (addonPackage.source)}
                      {#if index > 0}
                        <Separator />
                      {/if}

                      <div class="flex items-start gap-3 px-3 py-2 text-left">
                        <label class="flex min-w-0 flex-1 cursor-pointer items-start gap-2">
                          <input
                            class="mt-1"
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={profilePackageSelected(addonPackage.source)}
                            onchange={() => toggleProfilePackageSelection(addonPackage.source)}
                          />
                          <span class="flex min-w-0 flex-col gap-0.5">
                            <span class="truncate font-medium text-foreground">{addonPackage.source}</span>
                            {#if addonPackage.global}
                              <span class="truncate text-muted-foreground">Always active for every profile.</span>
                            {/if}
                            {#if !addonPackage.installedPath}
                              <span class="truncate text-muted-foreground">Installed path missing; update or reinstall this add-on.</span>
                            {/if}
                          </span>
                        </label>
                        <label class="flex shrink-0 cursor-pointer items-center gap-1 text-xs text-muted-foreground">
                          <input
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={addonPackage.global}
                            onchange={() => void toggleGlobalPackageSelection(addonPackage.source)}
                          />
                          Always active
                        </label>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="rounded-md border border-border bg-muted px-3 py-2 text-muted-foreground">
                    {piAddonsReady ? "No shared prompt add-ons installed." : "Loading prompt add-ons…"}
                  </p>
                {/if}

              </div>

              <div class="flex flex-col gap-3 rounded-md border border-border bg-background/60 p-3">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex min-w-0 flex-col gap-1">
                    <p class="font-medium text-foreground">Themes</p>
                    <p class="text-muted-foreground">{themeAddonPackages.length} themes · {selectedAddonCount(themeAddonPackages)} active · {globalAddonCount(themeAddonPackages)} always active</p>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    type="button"
                    aria-label="Open themes folder"
                    title="Open themes folder"
                    disabled={!selectedProfileId || profileResourceFolderOpening !== null}
                    onclick={() => void revealSelectedProfileResourceDir("themes")}
                  >
                    <RiFolderOpenLine aria-hidden="true" />
                  </Button>
                </div>

                {#if themeAddonPackages.length > 0}
                  <div class="flex max-h-40 flex-col overflow-auto rounded-md border border-border">
                    {#each themeAddonPackages as addonPackage, index (addonPackage.source)}
                      {#if index > 0}
                        <Separator />
                      {/if}

                      <div class="flex items-start gap-3 px-3 py-2 text-left">
                        <label class="flex min-w-0 flex-1 cursor-pointer items-start gap-2">
                          <input
                            class="mt-1"
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={profilePackageSelected(addonPackage.source)}
                            onchange={() => toggleProfilePackageSelection(addonPackage.source)}
                          />
                          <span class="flex min-w-0 flex-col gap-0.5">
                            <span class="truncate font-medium text-foreground">{addonPackage.source}</span>
                            {#if addonPackage.global}
                              <span class="truncate text-muted-foreground">Always active for every profile.</span>
                            {/if}
                            {#if !addonPackage.installedPath}
                              <span class="truncate text-muted-foreground">Installed path missing; update or reinstall this add-on.</span>
                            {/if}
                          </span>
                        </label>
                        <label class="flex shrink-0 cursor-pointer items-center gap-1 text-xs text-muted-foreground">
                          <input
                            type="checkbox"
                            disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                            checked={addonPackage.global}
                            onchange={() => void toggleGlobalPackageSelection(addonPackage.source)}
                          />
                          Always active
                        </label>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="rounded-md border border-border bg-muted px-3 py-2 text-muted-foreground">
                    {piAddonsReady ? "No shared theme add-ons installed." : "Loading theme add-ons…"}
                  </p>
                {/if}

              </div>
            </div>

            {#if unclassifiedAddonPackages.length > 0}
              <div class="flex flex-col gap-2 rounded-md border border-border bg-background/60 p-3">
                <div class="flex flex-col gap-1">
                  <p class="font-medium text-foreground">Other add-ons</p>
                  <p class="text-muted-foreground">No resource type was detected for these packages · {selectedAddonCount(unclassifiedAddonPackages)} active · {globalAddonCount(unclassifiedAddonPackages)} always active</p>
                </div>

                <div class="flex max-h-40 flex-col overflow-auto rounded-md border border-border">
                  {#each unclassifiedAddonPackages as addonPackage, index (addonPackage.source)}
                    {#if index > 0}
                      <Separator />
                    {/if}

                    <div class="flex items-start gap-3 px-3 py-2 text-left">
                      <label class="flex min-w-0 flex-1 cursor-pointer items-start gap-2">
                        <input
                          class="mt-1"
                          type="checkbox"
                          disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                          checked={profilePackageSelected(addonPackage.source)}
                          onchange={() => toggleProfilePackageSelection(addonPackage.source)}
                        />
                        <span class="flex min-w-0 flex-col gap-0.5">
                          <span class="truncate font-medium text-foreground">{addonPackage.source}</span>
                          {#if addonPackage.global}
                            <span class="truncate text-muted-foreground">Always active for every profile.</span>
                          {/if}
                          {#if !addonPackage.installedPath}
                            <span class="truncate text-muted-foreground">Installed path missing; update or reinstall this add-on.</span>
                          {/if}
                        </span>
                      </label>
                      <label class="flex shrink-0 cursor-pointer items-center gap-1 text-xs text-muted-foreground">
                        <input
                          type="checkbox"
                          disabled={profilePackageCommandRunning || piAddonGlobalSaving}
                          checked={addonPackage.global}
                          onchange={() => void toggleGlobalPackageSelection(addonPackage.source)}
                        />
                        Always active
                      </label>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </section>

          {#if profileError}
            <p class="rounded-md border border-border bg-muted px-3 py-2 text-sm text-muted-foreground">{profileError}</p>
          {/if}
        </div>

        <Dialog.Footer class="border-t border-border pt-3 sm:justify-between">
          <div class="flex gap-2">
            <Button variant="outline" type="button" disabled={!selectedProfileId} onclick={revealSelectedProfileDir}>Open profile folder</Button>
            <Button
              variant="destructive"
              type="button"
              disabled={!selectedProfileId || selectedProfileId === DEFAULT_PI_PROFILE_ID}
              onclick={deleteSelectedProfile}
            >
              Delete
            </Button>
          </div>
          <Button variant="ghost" type="submit" disabled={!profileNameDraft.trim() || piAddonBusy}>Save profile</Button>
        </Dialog.Footer>
      </form>
    </div>
  </Dialog.Content>
</Dialog.Root>

{#if appUpdateMessage}
  <div class="flex w-full shrink-0 items-center justify-between gap-2 border-b border-border bg-muted/60 px-2 py-1 text-xs text-muted-foreground">
    <span class="min-w-0 truncate">{appUpdateMessage}</span>
    <Button
      variant="ghost"
      size="sm"
      type="button"
      class="shrink-0"
      onclick={() => (appUpdateMessage = "")}
    >
      Dismiss
    </Button>
  </div>
{/if}
<div class="flex w-full shrink-0 items-center gap-1 overflow-hidden border-b border-border bg-muted/30 px-2 py-1 text-xs text-muted-foreground">
  <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
    {#each runningWorkspaces as workspace, index (workspace.instanceId)}
      {#if index > 0}
        <Separator orientation="vertical" class="mx-1 h-5 shrink-0" />
      {/if}

      <div class="flex min-w-0 shrink-0 items-center">
        <Button
          variant="ghost"
          size="sm"
          type="button"
          class={workspace.instanceId === activeWorkspaceInstanceId
            ? "h-7 min-w-0 max-w-52 justify-start gap-1.5 px-2 font-medium text-foreground"
            : "h-7 min-w-0 max-w-52 justify-start gap-1.5 px-2 text-muted-foreground"}
          aria-current={workspace.instanceId === activeWorkspaceInstanceId ? "page" : undefined}
          onclick={() => activateWorkspace(workspace.instanceId)}
        >
          <span class="truncate">{workspace.name}</span>
          {#if workspace.dirty}
            <span class="text-primary" aria-label="Unsaved changes">•</span>
          {/if}
          <span class="shrink-0 text-muted-foreground">{workspaceTerminalCount(workspace.instanceId)}</span>
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          type="button"
          class="h-7 px-1.5 text-muted-foreground"
          aria-label={`Close workspace ${workspace.name}`}
          title={`Close ${workspace.name}`}
          onclick={(event) => {
            event.stopPropagation();
            requestCloseWorkspace(workspace.instanceId);
          }}
        >
          ×
        </Button>
      </div>
    {/each}
  </div>

  <Separator orientation="vertical" class="h-6" />

  <div class="flex shrink-0 items-center gap-1">
    <Button variant="ghost" size="sm" type="button" class="h-7 px-2" onclick={createNewWorkspace}>New</Button>
    <Button
      variant="ghost"
      size="sm"
      type="button"
      class="h-7 px-2"
      disabled={!workspacePersistenceReady || savedWorkspaces.length === 0}
      onclick={openWorkspaceDialog}
    >
      Open
    </Button>
    <Button
      variant="ghost"
      size="sm"
      type="button"
      class="h-7 px-2"
      disabled={!workspacePersistenceReady || activeTerminalCount === 0}
      onclick={openSaveWorkspaceDialog}
    >
      Save
    </Button>
  </div>
</div>

<div class="min-h-0 flex-1 overflow-auto px-1.5 pb-2 pt-1">
  <div class="terminal-layout-stack relative h-full min-h-full">
    <div class="terminal-grid terminal-grid-content h-full min-h-full">
      {#each terminalSessions as terminal (terminal.id)}
        <div
          animate:flip={{ duration: TERMINAL_DND_FLIP_DURATION_MS }}
          class={!terminalIsLayoutVisible(terminal)
            ? "pointer-events-none fixed left-0 top-0 flex h-[24rem] w-[960px] -translate-x-[200vw] flex-col opacity-0"
            : `relative flex min-h-0 flex-col ${terminalIsSelected(terminal) ? "z-10" : "z-0"}`}
          aria-hidden={!terminalIsLayoutVisible(terminal)}
          onclick={() => selectTerminal(terminal.id)}
        >
          <WTermTerminal
            id={terminal.id}
            kind={terminal.kind}
            piProfileId={terminal.kind === "pi" ? normalizeProfileId(terminal.piProfileId) : undefined}
            piProfileName={terminal.kind === "pi" ? profileDisplayName(terminal.piProfileId) : undefined}
            workingDirectory={terminal.workingDirectory}
            autoFocus={terminalIsSelected(terminal) && !terminal.cached && !terminal.closing}
            closing={terminal.closing}
            layoutActive={terminalIsLayoutVisible(terminal)}
            layoutKey={terminalLayoutKey}
            onReady={handleTerminalReady}
            onClosed={handleTerminalClosed}
            onUserInput={handleTerminalUserInput}
            onClose={terminal.cached ? undefined : () => void requestCloseTerminal(terminal.id)}
            closeDisabled={terminal.closing}
            onSelect={() => selectTerminal(terminal.id)}
            selected={terminalIsSelected(terminal)}
          />
        </div>
      {/each}

      {#if activeTerminalCount === 0}
        <div class="col-span-full relative flex h-full min-h-0 items-center justify-center p-6">
          <div class="relative h-80 w-[min(68rem,100%)] select-none overflow-hidden sm:h-96 md:h-[28rem]" role="img" aria-label="PIOC">
            <ASCIIText text="PIOC" asciiFontSize={7} textFontSize={380} gradientColors={homeAsciiPalette.gradient} planeBaseHeight={16} enableWaves={true} waveStrength={0.35} interactive={false} onTextClick={cycleHomeAsciiPalette} />
          </div>
        </div>
      {/if}
    </div>

    {#if terminalDndEnabled}
      <div
        class="terminal-grid terminal-dnd-grid pointer-events-none absolute inset-0 z-20"
        use:dragHandleZone={terminalDndOptions}
        onconsider={handleTerminalDndConsider}
        onfinalize={handleTerminalDndFinalize}
        aria-label="Terminal layout order"
      >
        {#each terminalDndItems as item (item.id)}
          <div
            animate:flip={{ duration: TERMINAL_DND_FLIP_DURATION_MS }}
            class={cn(
              "terminal-dnd-cell pointer-events-none relative min-h-0",
              terminalDndItemIsShadow(item) && "terminal-dnd-shadow",
              selectedTerminalId === item.terminalId && "terminal-dnd-cell-selected",
            )}
            data-is-dnd-shadow-item-hint={terminalDndItemIsShadow(item) ? "true" : undefined}
            aria-label={terminalDndItemLabel(item)}
          >
            {#if !terminalDndItemIsShadow(item)}
              <button
                use:dragHandle
                type="button"
                class="terminal-dnd-handle pointer-events-auto absolute left-1/2 top-2 inline-flex size-7 -translate-x-1/2 items-center justify-center rounded-sm border border-border/80 bg-background/90 text-muted-foreground opacity-0 shadow-sm backdrop-blur transition hover:border-ring/60 hover:text-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                aria-label={`Move ${terminalDndItemLabel(item)}`}
                title="Drag to rearrange terminal"
                onpointerdown={(event) => {
                  event.stopPropagation();
                  selectTerminal(item.terminalId);
                }}
                onclick={(event) => event.stopPropagation()}
              >
                <RiDragMove2Line aria-hidden="true" />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .terminal-grid {
    display: grid;
    gap: 0.75rem;
    grid-auto-rows: minmax(14rem, 1fr);
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 24rem), 1fr));
  }

  .terminal-layout-stack {
    isolation: isolate;
  }

  .terminal-dnd-cell {
    border: 1px solid transparent;
  }

  .terminal-layout-stack:hover .terminal-dnd-handle,
  .terminal-dnd-cell-selected .terminal-dnd-handle,
  .terminal-dnd-handle:focus-visible,
  :global(#dnd-action-dragged-el) .terminal-dnd-handle {
    opacity: 1;
  }

  .terminal-dnd-handle {
    cursor: grab;
  }

  .terminal-dnd-handle:active,
  :global(#dnd-action-dragged-el) .terminal-dnd-handle {
    cursor: grabbing;
  }

  .terminal-dnd-shadow {
    border-color: color-mix(in oklab, var(--ring) 70%, transparent);
    background: color-mix(in oklab, var(--ring) 12%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--ring) 25%, transparent);
  }
</style>

# PIOC

PIOC is a Tauri 2 desktop workspace for running local [Pi Coding Agent](https://github.com/earendil-works/pi) terminals with profile-aware settings, auth sync, shared add-ons, and lightweight terminal telemetry/control.

## Requirements

- Node.js and `pnpm`
- Rust stable and the Tauri 2 platform prerequisites
- `pi` available on `PATH` (`pnpm` global installs are supported on Windows through the `pi.cmd` shim)

## Development

Use the dev Tauri identity so local testing never shares app data with the installed production app:

```bash
pnpm install
pnpm run app:dev
```

Useful quality gates:

```bash
pnpm run version:check  # package.json, tauri.conf.json, and Cargo.toml versions match
pnpm run check          # svelte-check
pnpm run typecheck      # tsc --noEmit
pnpm run test:js
pnpm run test:rust
pnpm run lint           # cargo fmt --check + clippy -D warnings
pnpm run ci             # version check, lint, tests, frontend build, cargo check
```

Builds:

```bash
pnpm run build:dev        # dev-mode frontend bundle
pnpm run build:prod       # prod-mode frontend bundle
pnpm run app:build:dev    # isolated PIOC Dev desktop build
pnpm run app:build:prod   # signed production build; requires updater env below
```

## Dev/prod split and self-updates

- The default `src-tauri/tauri.conf.json` is intentionally a **dev** identity: `PIOC Dev` / `com.pioc.app.dev`.
- Production builds merge `src-tauri/tauri.prod.conf.json` through `scripts/prepare-prod-tauri-config.mjs`, which writes the ignored `src-tauri/tauri.prod.generated.conf.json` with the real updater public key and endpoint.
- Production updater artifacts are created with `bundle.createUpdaterArtifacts = true`; dev artifacts do not create or check updates.

Before a production build or release, generate updater keys once and configure the build environment:

```bash
pnpm tauri signer generate -- -w ~/.tauri/pioc.key
```

```bash
# Public key printed by the signer command
TAURI_UPDATER_PUBLIC_KEY="..."

# Optional locally; GitHub Actions derives this from GITHUB_REPOSITORY
PIOC_UPDATE_ENDPOINT="https://github.com/<owner>/<repo>/releases/latest/download/latest.json"

# Required when creating updater artifacts
TAURI_SIGNING_PRIVATE_KEY="...private key content or path..."
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="...optional password..."
```

The GitHub release workflow expects:

- repository variable `TAURI_UPDATER_PUBLIC_KEY`
- repository secret `TAURI_SIGNING_PRIVATE_KEY`
- optional repository secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Push a `pioc-v*` tag or run the workflow manually to publish installers, signatures, and `latest.json`. Installed production PIOC can then use **Check for app updates** from the command palette when no terminals are running.

## Profiles and auth sync

Pi profiles are stored under the app data directory in `pi-profiles/<profile-id>`. Each profile has:

- `profile-meta.json` — PIOC metadata and UI-managed Pi defaults
- `settings.json` — generated Pi settings plus preserved custom settings
- `auth.json` — profile-local auth state
- `sessions/` — profile session data

Shared auth lives in `pioc-auth/auth.json`. PIOC merges JSON auth objects from idle profiles by modified time, uses an advisory lock file during sync, and writes the merged auth back to shared auth plus idle profiles. Running profiles are skipped so active Pi sessions do not have their auth file rewritten underneath them.

## Shared Pi add-ons

Shared add-ons are installed under the app data `pi-addons` directory. The UI invokes backend package commands only for `install`, `remove`, `update`, and `list`; package commands run with `GIT_TERMINAL_PROMPT=0` and a timeout so stalled installs do not run forever.

## Security posture

The Tauri webview does not expose `window.__TAURI__`, uses an explicit CSP, and grants the main window only the capabilities currently used by the frontend (`core`, `store`, directory-open dialog, app relaunch, and updater commands). Process, auth, profile, add-on, and PTY operations go through narrow Rust commands instead of broad shell/fs capabilities.

## Known limitations

- Profile running protection is still process-local; a future cross-instance profile lock should complement the auth lock.
- Large Svelte/Rust modules are still candidates for further refactoring into focused stores/services/components.
- Manual runtime testing is still recommended for terminal behavior, Pi model switching, and add-on install/update flows.

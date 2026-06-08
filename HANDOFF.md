# Handoff: Pi Profiles Implementation

Implemented first-pass Pi profiles from `IMPLEMENTATION_PI_PROFILES.md`.

## Files Changed

- `src-tauri/src/lib.rs`
- `src/lib/components/WTermTerminal.svelte`
- `src/lib/components/TerminalWorkspace.svelte`

## What Is Done

### Rust / Tauri

- Added `PiProfile` model.
- Added Tauri commands:
  - `pi_profiles_list`
  - `pi_profile_save`
  - `pi_profile_delete`
  - `pi_profile_reveal_dir`
- App data layout now uses:
  - `<app_data_dir>/pioc-auth/auth.json`
  - `<app_data_dir>/pi-profiles/<profile-id>/`
- Creates default profile: `default` / `Default Pi`.
- Creates profile subdirectories:
  - `skills`
  - `extensions`
  - `prompts`
  - `themes`
  - `sessions`
- Writes `profile-meta.json`.
- Generates `settings.json` from profile config.
  - Unset default provider/model/thinking fields are omitted.
  - Arrays are always included.
- Copies shared auth to profile auth on creation/launch only if profile auth is missing.
- On Pi process exit:
  - Copies launched profile `auth.json` back to shared auth.
  - Propagates shared auth to non-running profiles.
- Tracks running profile IDs to avoid overwriting auth for currently running profiles.
- Extended `pty_start` with `pi_profile_id`.
- Pi PTY now sets:
  - `PI_CODING_AGENT_DIR`
  - `PI_CODING_AGENT_SESSION_DIR`
- Shell terminals remain unchanged.

### Frontend

- `TerminalSession` includes `piProfileId`.
- Saved workspace terminal entries include `piProfileId`.
- `WTermTerminal` accepts:
  - `piProfileId`
  - `piProfileName`
- `WTermTerminal` passes `piProfileId` into `pty_start`.
- Terminal header displays the profile name for Pi terminals.
- `TerminalWorkspace` loads profiles via Rust command.
- Added top-level Pi profile selector.
- Added profile management dialog supporting:
  - New profile
  - Edit profile
  - Save profile
  - Delete non-default profile
  - Open profile folder
  - Package/skills/extensions/prompts/themes as line-separated text
  - Default provider/model/thinking fields
- New Pi terminals use the selected active profile.
- Saved workspaces persist and restore profile IDs.

## Validation Already Run

- `pnpm build` passed.
- `cd src-tauri && cargo check` passed.
- `cd src-tauri && cargo fmt` was run.

## Important Notes

1. The UI is intentionally minimal.
   - It uses raw `<select>` and `<textarea>` because the installed shadcn-svelte UI set appears limited to button/input/dialog/separator/toggle.
   - If improving UI, read/use the shadcn skill first per project instructions.

2. Manual runtime testing still needed in Tauri:
   - Fresh launch creates default profile.
   - Launching Pi starts with profile env vars.
   - `/login` updates profile auth.
   - Closing Pi syncs auth back to shared auth.
   - A second profile receives shared auth after the first profile exits.
   - Workspaces reopen with correct profiles.
   - Shell terminals still work.

3. Timestamp shape:
   - Rust `now_string()` currently returns Unix seconds as a string.
   - Frontend-created profiles use ISO strings.
   - This works structurally, but the desired data model likely expects ISO/RFC3339. Consider switching Rust timestamps to ISO if adding a time crate.

4. Profile deletion:
   - Rust prevents deleting the default profile.
   - Rust prevents deleting a profile with running terminals.
   - Frontend does not show a confirmation prompt yet.

5. Auth sync:
   - This is simple copy-if-changed behavior, not JSON merge.
   - That matches first-pass scope from the implementation plan.

6. `settings.json` generation:
   - Unset default provider/model/thinking fields are omitted.
   - Arrays are always written.
   - Confirm stock Pi accepts the exact generated shape during runtime testing.

7. Environment note:
   - This directory was not a git repo in the previous agent environment, so `git diff` did not work there.

## Suggested Next Steps

1. Run the app with:

   ```bash
   pnpm tauri dev
   ```

2. Test profile creation, profile launch, workspace restore, and auth sync.

3. Add delete confirmation for profiles.

4. Improve profile picker UX when multiple profiles exist.

5. Consider adding profile badges/details in workspace cards.

6. Consider updating `lastUsedAt` when launching profiles.

7. Consider persisting the active selected profile across app restarts.

8. If UI polishing, use the shadcn skill and prefer existing installed components before adding new ones.

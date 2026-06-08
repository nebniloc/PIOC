# PIOC Guidelines

Use `pnpm` for JS packages and scripts.
Use the shadcn skill before doing any work on the frontend.

## Library Roles

- **Tauri 2**: desktop app shell and Rust bridge.
- **Svelte 5**: frontend framework.
- **shadcn-svelte / bits-ui / Tailwind**: UI components and styling.
- **@wterm/dom**: terminal rendering.
- **@wterm/just-bash + just-bash**: lightweight browser-only demo/local shell behavior.
- **Pi RPC (`pi --mode rpc`)**: Pi Agent sessions and extension support.
- **zod**: validate Pi RPC messages/events.
- **svelte-dnd-action**: drag/reorder terminal panes.
- **@tauri-apps/plugin-shell**: spawn/manage local Pi processes.
- **@tauri-apps/plugin-store**: persist layouts and app settings.
- **@tauri-apps/plugin-dialog**: file/confirmation dialogs.
- **@tauri-apps/plugin-clipboard-manager**: terminal copy/paste.
- **@tauri-apps/plugin-fs**: future workspace/file access.

# PIOC Commands

Run these from the project root:

```bash
cd D:/Source/Work/PIOC
```

## Start development app

```bash
pnpm run app:dev
```

This launches the isolated dev desktop app:

- app name: `PIOC Dev`
- app id: `com.pioc.app.dev`
- separate app data from production
- updater disabled

## Common quality commands

```bash
pnpm run check          # Svelte check
pnpm run typecheck      # TypeScript check
pnpm run test           # JS + Rust tests
pnpm run lint           # Rust fmt check + clippy
pnpm run ci:fast        # faster local checks for iteration
pnpm run ci             # full local quality gate before release
```

## Frontend-only commands

```bash
pnpm run dev            # Vite only, no Tauri shell
pnpm run build:dev      # dev-mode frontend build
pnpm run build:prod     # prod-mode frontend build
pnpm run preview        # preview built frontend
```

## Desktop build commands

```bash
pnpm run app:build:dev
```

Builds the isolated `PIOC Dev` desktop app. Windows builds create the NSIS installer only for faster packaging.

```bash
pnpm run app:build:prod
```

Builds the signed production app. This requires updater signing configuration. Windows builds create the NSIS installer only because that is what the updater uses.

## Normal change workflow

```bash
pnpm run ci:fast
git add .
git commit -m "your message"
git push
```

## Production release workflow

1. Bump the version in all three files:

   ```txt
   package.json
   src-tauri/tauri.conf.json
   src-tauri/Cargo.toml
   ```

2. Verify the versions match:

   ```bash
   pnpm run version:check
   ```

3. Run checks. Use `ci:fast` while iterating, then `ci` before tagging if you did not already get a green GitHub CI run:

   ```bash
   pnpm run ci:fast
   pnpm run ci
   ```

4. Commit and push:

   ```bash
   git add .
   git commit -m "Release vX.Y.Z"
   git push origin main
   ```

5. Tag and push the release tag:

   ```bash
   git tag pioc-vX.Y.Z
   git push origin pioc-vX.Y.Z
   ```

GitHub Actions will build and publish the signed production NSIS installer, signature, and `latest.json` updater manifest. Tag pushes do not run the separate CI workflow.

## Updater signing setup

The signing key currently lives locally at:

```txt
C:\Users\ebach\.tauri\pioc.key
```

GitHub is configured with:

- repository variable: `TAURI_UPDATER_PUBLIC_KEY`
- repository secret: `TAURI_SIGNING_PRIVATE_KEY`

If the signing key is ever lost, already-installed production apps will not be able to update to newly signed releases.

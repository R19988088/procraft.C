# AGENTS.md

## Project overview

ProCraft is a cross-platform drawing application. The shared Rust crates contain the canvas, document, image, and GPU layers. The desktop application is Rust/egui under `desktop/`; Android uses Kotlin/Compose under `android/`.

## Local customizations

This fork keeps the upstream project structure while carrying local product changes:

- Chinese (`zh`) is the default interface language.
- Desktop and Android Preferences expose a persistent Chinese/English language selector.
- Desktop Preferences, Actions, Advanced Settings, and Brush Studio include localized labels.
- The Windows portable build is produced at `dist/procraft-windows-x64.exe` outside the source tree.

Keep these changes when synchronizing upstream. Prefer small, isolated commits for future customizations so conflicts are easy to resolve.

## Upstream workflow

- `origin` is the maintained fork: `https://github.com/R19988088/procraft.C.git`.
- `upstream` is the source project: `https://github.com/artcraft-android/procraft.git`.
- Update references with `git fetch upstream` and review upstream changes before merging or rebasing them onto `main`.
- Resolve conflicts in the localization and preference files manually; preserve the local language preference behavior and Chinese default.
- Never commit generated build output, local preference files, or the timestamped source backups.

## Validation

Before publishing changes, run from the repository root:

```text
cargo fmt --all -- --check
cargo check -p procraft-desktop
cargo build -p procraft-desktop --release
```

For Android changes, run the appropriate Gradle task from `android/` when the required SDK and dependencies are available.

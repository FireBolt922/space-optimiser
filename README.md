# Space Optimiser

Space Optimiser is a desktop application in development that helps a person review reclaimable disk space and confirm only the cleanups they want. Nothing in this repository scans a disk or deletes files yet.

The product is independent and general purpose. It is not tied to any existing project.

## Who it is for

People who can see that a disk is full and should not have to know which caches, logs, and package directories are safe to remove. A developer can run a deeper scan for tool caches. A non-developer can stay on the short scan.

## Platforms

The application is built for, and released in this order:

1. macOS 14 or later
2. Ubuntu 22.04, 24.04, and 26.04 LTS
3. Windows 11 (Windows 10 is best effort, not claimed)

A release names only the operating systems it was tested on.

Each platform has its own locations, permission rules, and cleanup tools. A path that is safe on one system is not assumed to be safe on another.

The only cleanups measured so far were on one Apple Silicon Mac. Windows and Ubuntu behavior is proposed, not measured.

## Workflow

1. Choose Normal Scan or Deep Scan.
2. Scan. The app does not delete anything while scanning.
3. Read each result: what it is, where it is, how much space it might recover, and what can go wrong.
4. Select items, or use "Select safe items" for the low-risk items that allow it.
5. Review the exact actions.
6. Confirm. The engine checks every path again, then performs only the allowed operations. Deletions cannot be undone; files do not go to the Trash.
7. See what was removed, what was skipped, what failed, and how much free space changed.

## Safety

This sentence is **not** a guarantee the product can make:

> The application will not delete personal data or modify configurations in ways that disrupt normal day-to-day activities.

A cleanup tool cannot promise that on every machine. Caches can be locked, misclassified, or expensive to rebuild. The Trash can contain personal files. Container volumes and virtual disks can look unused and still be the only copy of data.

The product target is narrower:

> Space Optimiser only removes files in locations it has a reviewed rule for, and only after you confirm. It does not clean your documents, photos, downloads, or source code, and it does not change system settings, passwords, or app configuration. A confirmed cleanup can still make an app rebuild a cache or download files again.

The enforceable rules (allowlist, denylist, re-validation before every removal, no elevation, no shell) are summarized in [AGENTS.md](AGENTS.md).

## Technology

- Rust for scanning, policy, path checks, and deletion.
- TypeScript and React for the interface.
- Tauri 2 as the desktop shell.

Tauri was chosen because the interface cannot use Node to delete files on its own, the installed app stays smaller than Electron, and one Rust engine can serve all three operating systems.

## Status

Policy and paths (milestone M2). The engine resolves protected folders and temp folders on macOS, Ubuntu, and Windows, and finds old temporary files in test fixtures. It measures them and deletes nothing. The window is still empty: every command it can send returns "not supported" until the first scan screen arrives. There is no delete code and no installer.

Planning notes are kept locally and are not tracked in this repository. Cloning the repository does not bring them with it.

## Development

Agents working in this repository must follow [AGENTS.md](AGENTS.md).

```text
Cargo.toml                 workspace
crates/engine/             scan, policy, measure, execute (the only deleter)
apps/desktop/              React UI
apps/desktop/src-tauri/    thin Tauri shell: typed commands, no file logic
fixtures/homes/            fake home directories for tests
```

Requirements: Rust 1.88 (pinned in `rust-toolchain.toml`), Node 24, and on Ubuntu `libwebkit2gtk-4.1-dev`.

```sh
npm --prefix apps/desktop ci
npm --prefix apps/desktop run build    # the Tauri shell embeds the built UI
cargo test --workspace
npm --prefix apps/desktop run tauri dev
```

CI runs formatting, clippy, and `cargo test --workspace` on macOS, Ubuntu, and Windows.

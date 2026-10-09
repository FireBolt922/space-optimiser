# Space Optimiser

Space Optimiser is a planned desktop application that helps a person review reclaimable disk space and confirm only the cleanups they want. It does not exist yet. Nothing in this repository scans a disk or deletes files.

The product is independent and general purpose. It is not tied to any existing project.

## Who it is for

People who can see that a disk is full and should not have to know which caches, logs, and package directories are safe to remove. A developer can run a deeper scan for tool caches. A non-developer can stay on the short scan.

## Platforms

The planned application runs on:

- macOS
- Windows
- Ubuntu Linux

Each platform has its own locations, permission rules, and cleanup tools. A path that is safe on one system is not assumed to be safe on another.

The only cleanups measured so far were on one Apple Silicon Mac. Windows and Ubuntu behavior is proposed, not measured.

## Workflow

1. Choose Normal Scan or Deep Scan.
2. Scan. The app does not delete anything while scanning.
3. Read each result: what it is, where it is, how much space it might recover, and what can go wrong.
4. Select items, or use Select All for the low-risk items that allow it.
5. Review the exact actions.
6. Confirm. The engine checks every path again, then performs only the allowed operations.
7. See what was removed, what was skipped, what failed, and how much free space changed.

## Safety

This sentence is **not** a guarantee the product can make:

> The application will not delete personal data or modify configurations in ways that disrupt normal day-to-day activities.

A cleanup tool cannot promise that on every machine. Caches can be locked, misclassified, or expensive to rebuild. The Trash can contain personal files. Container volumes and virtual disks can look unused and still be the only copy of data.

The product target is narrower:

> Space Optimiser only removes files in locations it has a reviewed rule for, and only after you confirm. It does not clean your documents, photos, downloads, or source code, and it does not change system settings, passwords, or app configuration. A confirmed cleanup can still make an app rebuild a cache or download files again.

The enforceable rules (allowlist, denylist, re-validation before every removal, no elevation, no shell) are summarized in [AGENTS.md](AGENTS.md).

## Proposed technology

- Rust for scanning, policy, path checks, and deletion.
- TypeScript and React for the interface.
- Tauri 2 as the desktop shell.

Tauri is recommended because the interface cannot use Node to delete files on its own, the installed app stays smaller than Electron, and one Rust engine can serve all three operating systems. No framework has been installed.

## Status

Planning only. There is no scanner, no interface, and no installer.

Planning notes are kept locally and are not tracked in this repository. Cloning the repository does not bring them with it.

## Development

Agents working in this repository must follow [AGENTS.md](AGENTS.md). Implementation starts only after the plan is accepted. Do not add application code during planning.

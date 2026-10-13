# Space Optimiser

[![CI](https://github.com/FireBolt922/space-optimiser/actions/workflows/ci.yml/badge.svg)](https://github.com/FireBolt922/space-optimiser/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Conventional Commits](https://img.shields.io/badge/commits-conventional-fe5196.svg)](https://www.conventionalcommits.org/en/v1.0.0/)

**Download:** [firebolt922.github.io/space-optimiser](https://firebolt922.github.io/space-optimiser/) lists the latest build for each system and every earlier one.

Space Optimiser is a desktop application in development that helps a person review reclaimable disk space and confirm only the cleanups they want. It cleans a fixed list of caches and temporary files on macOS, Ubuntu, and Windows. Test builds are unsigned and not yet released for general use.

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

Milestones M1 to M7 are merged: the engine, the Normal and Deep Scan rows below, the review and confirm flow, handle-relative deletion with an audit log, the About and Help screens, and the release workflow. No version has been tagged yet; the first pre-release for testers is next, one operating system at a time. Windows CI is being fixed before that.

**Normal Scan** offers old files in the per-user temporary folder, the HTTP caches of Chrome, Edge, Brave, Chromium, and Firefox (only when the browser is not running), and on Ubuntu the thumbnail and font caches.

**Deep Scan** adds developer caches (npm, npx, pnpm, Yarn, pip, uv, Cargo, Gradle, NuGet), editor caches (VS Code, Cursor, and JetBrains IDEs, only when the editor is not running), old log files on macOS, the Docker build cache when Docker runs locally, and a list of large files in the home folder. The large-file list is information only.

"Select safe items" picks only the low-risk rows: old temporary files, and on Ubuntu the thumbnail and font caches (old logs on macOS in Deep Scan).

Planning notes are kept locally and are not tracked in this repository. Cloning the repository does not bring them with it; the catalog of cleanup rules is summarised in `docs/release-notes-draft.md` and in the engine's `crates/engine/src/policy.rs`.

## Development

Everyone working in this repository follows [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md). Security problems go through [SECURITY.md](SECURITY.md), never a public issue.

```text
Cargo.toml                 workspace
crates/engine/             scan, policy, measure, execute (the only deleter)
apps/desktop/              React UI
apps/desktop/src-tauri/    thin Tauri shell: typed commands, no file logic
site/                      download page for GitHub Pages, built from the releases
fixtures/homes/            fake home directories for tests
```

Requirements: Rust 1.88 (pinned in `rust-toolchain.toml`), Node 24, and on Ubuntu `libwebkit2gtk-4.1-dev`.

```sh
npm ci                                 # root tooling: git hooks, commitlint, prettier
npm --prefix apps/desktop ci
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test         # component tests with a fake engine
npm --prefix apps/desktop run build    # the Tauri shell embeds the built UI
cargo test --workspace
npm --prefix apps/desktop run tauri dev
```

The root `npm ci` installs [lefthook](https://github.com/evilmartians/lefthook) hooks: formatting and lint on commit, Conventional Commits on the message, and clippy plus the full test suite on push. Branch names are `<type>/<topic>`.

To try a cleanup without touching your own files, run `scripts/fixture-home.sh`. It copies the test fixture home into a temporary folder and prints the command that points a debug build at it. Release builds ignore that setting.

CI runs on every pull request and on `main`: the UI lint and tests, formatting, clippy, and `cargo test --workspace` on macOS, Ubuntu, and Windows; Prettier and commit-message checks; `cargo deny` and `npm audit`; and a coverage report. All of it must be green before a merge. Dependabot opens weekly update PRs.

## Releasing

A release is one click in the Actions tab, never a push to `main`.

1. Open Actions → Release → Run workflow on `main`. Leave `bump` on `auto` (a feature since the last tag raises the minor version, a fix the patch version) or pick the part to raise, and pick a channel: `prerelease` for testers, `draft` to check the page first, or `release`.
2. The workflow raises the version in `Cargo.toml`, `Cargo.lock`, `package.json`, and `package-lock.json`, moves the **Unreleased** entries of `CHANGELOG.md` into a dated section, opens a pull request `chore(release): vX.Y.Z`, and runs CI on it.
3. When CI is green, the workflow merges the pull request and starts Publish, which builds a universal macOS `.dmg`, an Ubuntu `.deb`, and Windows `-setup.exe` and `.msi` installers, creates the `vX.Y.Z` tag, and publishes the release page with the changelog section and a `SHA256SUMS.txt`.

The main-branch ruleset still applies to that merge: every required check must be green and the branch up to date. If CI fails, the pull request stays open and nothing is released; fix it on the branch and merge by hand, and Publish starts on the merge. The workflows refuse a version that is already tagged, a commit whose CI has not passed, and a changelog with nothing under **Unreleased**. The builds are unsigned. Testers follow [docs/testing-guide.md](docs/testing-guide.md). To retry a build that failed after the merge, run Actions → Publish → Run workflow on `main`.

After a release, Publish starts the Pages workflow, which rebuilds the [download page](https://firebolt922.github.io/space-optimiser/) from the published releases (drafts are left out). It also runs daily and when `site/` changes, so a release edited by hand shows up within a day, or at once with Actions → Pages → Run workflow. To preview the page locally: `node site/build.mjs --repo FireBolt922/space-optimiser --out _site`, then open `_site/index.html`.

The download page needs Settings → Pages → Source set to "GitHub Actions". A custom domain is set on the same settings page; the page uses relative links, so nothing in the repository changes.

The Release workflow opens and merges pull requests, so Settings → Actions → General → Workflow permissions must allow GitHub Actions to create and approve pull requests.

## Licence

Space Optimiser is released under the [MIT License](LICENSE). The bundled Inter typeface is under the [SIL Open Font License 1.1](apps/desktop/src/assets/fonts/OFL.txt).

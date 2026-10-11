# Agent rules

Read this file and the relevant plan before changing the repository.

## Plans

- Planning notes are kept locally and are not tracked in git. The ignored location is listed in `.gitignore`. If the notes exist on this machine, read their README before implementing. If they do not exist, ask for them before writing application code. An outside contributor without the notes works from an agreed issue instead (see `CONTRIBUTING.md`), and the maintainer updates the notes when merging.
- Start with the implementation-plan overview, then the phase you are executing.
- The history note records past manual cleanups. Do not turn a past manual action into a product feature unless the cleanup catalog marks it as allowed.
- Stay inside the current phase. Do not start a later phase's features early.
- If a requirement changes, update the plan, the cleanup catalog, and the tests in the same change.
- Do not link to, quote paths from, or commit the planning notes in tracked files.

## Safety

- The desktop UI must not delete files, construct shell commands, or choose paths. It sends candidate ids to the Rust engine.
- The engine must re-check every path against the cleanup policy immediately before removal.
- Add a cleanup only by adding an allowlisted operation in the catalog and in Rust. Do not scan an arbitrary directory and delete what looks large or old.
- Do not follow symlinks out of an allowlisted root. Do not delete personal documents, photos, downloads, source trees, credentials, or application configuration.
- Do not empty the Trash or Recycle Bin, prune container volumes, delete virtual-disk images, or edit database rows.
- Select All may include only items the catalog marks as bulk-safe.
- Never claim space was recovered unless a test or a measurement in the session shows it.

## Commits, branches, and hooks

- Branch names are `<type>/<topic>` (`fix/temp-root-project-marker`). Commit messages and pull request titles follow Conventional Commits with the types and scopes in `commitlint.config.cjs`. Pull requests are squash-merged.
- `npm ci` at the repository root installs the lefthook hooks. Never commit or push with `--no-verify`. If a hook fails, fix the cause.
- `CHANGELOG.md` gets a line under **Unreleased** for every user-visible change.
- CI must be green on macOS, Ubuntu, and Windows before a merge. A new GitHub Action is pinned to a commit SHA with its version in a comment.

## Build discipline

- Use test-driven development for policy, path validation, deduplication, and execution. Write the failing test, then the code.
- Keep the scan engine, policy, platform adapters, executor, and UI separate, as described in the architecture phase.
- Do not add dependencies, packaging, or a new platform behavior without a plan update.
- Do not claim a phase is complete without the evidence named in that phase's acceptance criteria.

## Visual design

- Style the UI only with the semantic tokens in `apps/desktop/src/styles/tokens.css` (`surface`, `ink`, `border`, `primary`, ...). Do not use the `palette-*` primitives or hard-coded colours in components.
- Dark is the default theme (`data-theme="dark"` on `<html>`). Every text colour pair must stay at 4.5:1 or more in both themes.
- Inter is bundled locally. Do not load fonts, styles, or icons from a CDN; the CSP blocks remote content.
- No inline styles (the release CSP blocks them), no gradients on controls, no pill buttons. One primary button per view.
- The app icon masters are in `apps/desktop/src-tauri/icons/source/`. Regenerate the platform icons from the `-desktop` variant with `npx tauri icon`.

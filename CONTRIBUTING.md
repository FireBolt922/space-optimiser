# Contributing

Thank you for helping. Space Optimiser is small and careful: it deletes
files, so every change to the engine is held to the safety rules in
[AGENTS.md](AGENTS.md). Read that file first; it applies to people as much
as to coding agents.

## Before you start

- Small fixes, docs, and UI tweaks: just open a pull request.
- For a new cleanup rule or a larger feature, open an issue first so the
  approach can be agreed before you write code. The cleanup catalog is an
  allowlist, and a row is added only after its risks are written down.
- Check [SECURITY.md](SECURITY.md) if your change touches path validation,
  deletion, or process execution.

## Set up

Requirements: Rust 1.88 (pinned in `rust-toolchain.toml`), Node 24, and on
Ubuntu `libwebkit2gtk-4.1-dev`.

```sh
git clone https://github.com/FireBolt922/space-optimiser.git
cd space-optimiser
npm ci                      # root tools: git hooks, commit lint, prettier
npm --prefix apps/desktop ci
```

`npm ci` at the root installs [lefthook](https://github.com/evilmartians/lefthook)
and registers two small git hooks. They take seconds and mostly fix things
for you:

| Hook         | What runs                                                                             |
| ------------ | ------------------------------------------------------------------------------------- |
| `pre-commit` | formats staged Rust (`rustfmt`) and other files (Prettier), large-file guard          |
| `pre-push`   | `scripts/hygiene.sh`: deletes only in the executor, processes only in the tool runner |

CI runs the full suite on macOS, Ubuntu, and Windows for every pull request.
To run the same checks locally first, use `npm run check`.

## Day-to-day commands

```sh
cargo test --workspace                 # engine and shell tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
npm --prefix apps/desktop run tauri dev
scripts/fixture-home.sh                # try a cleanup on a copy of the fixture home
```

## Branches and commits

- Branch from `main`, with any branch name. Maintainers use
  `<type>/<short-topic>`, for example `fix/temp-root-project-marker`.
- Commits on your branch can say anything: pull requests are squash-merged,
  so only the **pull request title** lands on `main`. The title is a
  [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/) line,
  and CI checks it:

  ```text
  <type>(<optional scope>): <summary>
  ```

  Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`,
  `chore`, `revert`. A scope is optional and can be any word that names the
  area, such as `engine`, `desktop`, `ci`, or `updater`. A breaking change
  adds `!` after the type and a `BREAKING CHANGE:` footer.

- The type matters because the release workflow reads it to pick the next
  version. If you get the title wrong, a maintainer can fix it before merging.

## Pull requests

`main` is protected by the rulesets in `.github/rulesets/` (importable from
Settings → Rules → Rulesets): no direct pushes, squash merge only, and the
tests, formatting, and dependency checks must pass. You do not need to
rebase when `main` moves on, unless there is a conflict. Release tags
cannot be moved or deleted.

- Keep a PR to one topic. Small PRs are reviewed quickly.
- CI must be green on all three operating systems before merge. A first
  pull request from a new contributor waits for a maintainer to start CI.
- For an engine change, name the test that fails without your change.
- A line in `CHANGELOG.md` under **Unreleased** is welcome; if you leave it
  out, a maintainer adds it.

## Rules for engine changes

- Test first. Write the failing test, then the code (`crates/engine/tests/`).
- A cleanup is added only as an allowlisted catalog row with a fixture, a
  discover test, and an execute test in the same change.
- Deletes exist only in the executor; processes start only in the tool
  runner. CI greps for violations.
- Never claim space was recovered without a measurement in a test.

## UI changes

- Use the semantic tokens in `apps/desktop/src/styles/tokens.css`; no raw
  colours, no inline styles, no CDN assets.
- File names are untrusted. Render them as text only.
- Keep every text pair at 4.5:1 contrast in both themes
  (`src/contrast.test.ts`).

## Releases

Maintainers run the Release workflow from `main`. It opens a version pull
request, merges it when CI is green, and starts the build that publishes
the installers. See the README.

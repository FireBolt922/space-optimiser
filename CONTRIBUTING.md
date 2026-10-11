# Contributing

Thank you for helping. Space Optimiser is small and careful: it deletes
files, so every change to the engine is held to the safety rules in
[AGENTS.md](AGENTS.md). Read that file first; it applies to people as much
as to coding agents.

## Before you start

- Open an issue for anything bigger than a typo so the approach can be agreed
  before you write code. New cleanup rules especially: the catalog is an
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
and registers the git hooks. They run on every commit and push:

| Hook         | What runs                                                                              |
| ------------ | -------------------------------------------------------------------------------------- |
| `pre-commit` | `cargo fmt` on staged Rust, prettier and eslint on staged UI files, a large-file guard |
| `commit-msg` | commitlint (Conventional Commits, see below)                                           |
| `pre-push`   | `cargo clippy -D warnings`, `cargo test --workspace`, UI lint and tests                |

If a hook fails, fix the cause; do not use `--no-verify`.

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

- Branch from `main`. Name the branch `<type>/<short-topic>`, for example
  `fix/temp-root-project-marker` or `docs/contributing`.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

  ```text
  <type>(<scope>): <summary in the imperative, lower case, no full stop>

  <body: what changed and why, wrapped at 72 columns>
  ```

  Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`,
  `chore`, `revert`. Scopes are optional; use `engine`, `desktop`, `ci`,
  `docs`, or `release` when one fits. A breaking change adds `!` after the
  type and a `BREAKING CHANGE:` footer.

- Pull requests are squash-merged, and the PR title must itself be a valid
  Conventional Commit line; CI checks it.

## Pull requests

`main` is protected by the rulesets in `.github/rulesets/` (importable from
Settings → Rules → Rulesets): no direct pushes, squash merge only, and
every CI job must pass on the latest commit. Release tags cannot be moved
or deleted.

- Keep a PR to one topic. Small PRs are reviewed quickly.
- CI must be green on all three operating systems before merge.
- Fill in the PR template. For an engine change, name the test that fails
  without your change.
- Update `CHANGELOG.md` under **Unreleased**.

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

Maintainers release by hand: `scripts/bump-version.sh`, a PR, then the
Release workflow from `main`. See the README.

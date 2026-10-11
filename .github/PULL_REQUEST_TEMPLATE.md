## What

<!-- One or two sentences. The PR title must be a Conventional Commit line, e.g. `fix(engine): reject a temp root that is a project folder`. -->

## Why

<!-- The problem, or a link to the issue. -->

## Safety

- [ ] No new way to delete, start a process, or accept a path from the window (or explain below).
- [ ] Engine change: the test that fails without this change is `…`.
- [ ] Catalog change: the row, fixture, discover test, and execute test are in this PR.

## Checks

- [ ] `cargo test --workspace`, clippy, and fmt pass locally.
- [ ] `npm --prefix apps/desktop run lint` and `npm --prefix apps/desktop test` pass.
- [ ] `CHANGELOG.md` updated under **Unreleased**.
- [ ] Docs updated if behaviour or commands changed.

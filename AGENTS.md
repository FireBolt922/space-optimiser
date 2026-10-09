# Agent rules

Read this file and the relevant plan before changing the repository.

## Plans

- Planning lives in `develop-eggs/`. That directory is gitignored, so it exists only on a machine that has the files. Read it locally before implementing.
- Start with `develop-eggs/implementation-plan/README.md`, then the phase you are executing.
- `develop-eggs/what_we_Actually_did.md` is history. Do not turn a past manual action into a product feature unless the cleanup catalog marks it as allowed.
- Stay inside the current phase. Do not start a later phase's features early.
- If a requirement changes, update the plan, the cleanup catalog, and the tests in the same change.

## Safety

- The desktop UI must not delete files, construct shell commands, or choose paths. It sends candidate ids to the Rust engine.
- The engine must re-check every path against the cleanup policy immediately before removal.
- Add a cleanup only by adding an allowlisted operation in the catalog and in Rust. Do not scan an arbitrary directory and delete what looks large or old.
- Do not follow symlinks out of an allowlisted root. Do not delete personal documents, photos, downloads, source trees, credentials, or application configuration.
- Do not empty the Trash or Recycle Bin, prune container volumes, delete virtual-disk images, or edit database rows.
- Select All may include only items the catalog marks as bulk-safe.
- Never claim space was recovered unless a test or a measurement in the session shows it.

## Build discipline

- Use test-driven development for policy, path validation, deduplication, and execution. Write the failing test, then the code.
- Keep the scan engine, policy, platform adapters, executor, and UI separate, as described in phase 3.
- Do not add dependencies, packaging, or a new platform behavior without a plan update.
- Do not claim a phase is complete without the evidence named in that phase's acceptance criteria.

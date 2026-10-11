# Changelog

All notable changes to Space Optimiser are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0.0, a
minor version can change behaviour.

## [Unreleased]

### Added

- The Scanning screen says which part of the scan is running (for example
  "Asking uv where its cache is."), lists every stage as Done, Checking, or
  Waiting, and shows how long the scan has been running.
- Results has Scan again at the top as well, and says when the scan ran and
  how long it took.
- Temporary files are grouped by where they came from (app folders, macOS
  system services, unnamed scratch files, other). Each row says whether it is
  a file or a folder and how old it is, and empty items fold into one line.
- "Left alone" groups skipped items by reason, with a sentence on why each
  reason keeps an item out.

### Changed

- Scan again reruns the same scan mode instead of returning to the start
  screen.
- Large counts are written with thousands separators.

## [0.2.0] - 2026-10-11

### Added

- Results are grouped under category buttons (Temporary files, Browsers,
  Developer tools, System caches, Logs, Large files) with a count and size
  each, and a name filter narrows the rows.
- A group longer than ten rows folds behind "Show all"; the report folds
  long Removed and Skipped lists the same way, and Failed always stays open.
- A download page on GitHub Pages shows the latest installers for macOS,
  Ubuntu, and Windows, how to open an unsigned build, and earlier releases.
  It is rebuilt after every release.

### Changed

- The Review, Scan again, Export report, and Done buttons stay at the bottom
  of the window instead of after the last row.
- Releases are prepared by the Release workflow, which raises the version,
  dates the changelog, and opens a pull request; merging it builds and
  publishes the installers.

## [0.1.0] - 2026-10-11

The first vertical slice: Normal Scan and Deep Scan over the allowlisted
catalog, review and confirm, handle-relative deletion with an audit log, and
unsigned installers for macOS, Ubuntu, and Windows.

### Added

- MIT licence, contributing guide, security policy, code of conduct, issue
  and pull request templates, and Dependabot.
- Git hooks (lefthook) for formatting, linting, and Conventional Commits.
- CI on pull requests, dependency licence and advisory checks, and test
  coverage reports; each action is pinned to a commit.
- The temp root is rejected when it is itself a project folder, and on
  Windows when it lies outside Local AppData.
- The Trash folders are on the protected list, so the large-file report
  never looks inside them.

[Unreleased]: https://github.com/FireBolt922/space-optimiser/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/FireBolt922/space-optimiser/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/FireBolt922/space-optimiser/releases/tag/v0.1.0

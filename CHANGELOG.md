# Changelog

All notable changes to Space Optimiser are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0.0, a
minor version can change behaviour.

## [Unreleased]

### Added

- MIT licence, contributing guide, security policy, code of conduct, issue
  and pull request templates, and Dependabot.
- Git hooks (lefthook) for formatting, linting, and Conventional Commits.
- CI on pull requests, dependency licence and advisory checks, and test
  coverage reports.
- The temp root is rejected when it is itself a project folder, and on
  Windows when it lies outside Local AppData.
- The Trash folders are on the protected list, so the large-file report
  never looks inside them.

### Changed

- CI pins each action to a commit.

## [0.1.0] - Unreleased

The first vertical slice: Normal Scan and Deep Scan over the allowlisted
catalog, review and confirm, handle-relative deletion with an audit log, and
unsigned installers for macOS, Ubuntu, and Windows.

[Unreleased]: https://github.com/FireBolt922/space-optimiser/compare/main...HEAD
[0.1.0]: https://github.com/FireBolt922/space-optimiser/releases

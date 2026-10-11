# Security policy

Space Optimiser deletes files. A bug in its path rules matters more than in
most desktop apps, so please report anything that could make it remove the
wrong thing, run something it should not, or send data off the machine.

## Reporting a vulnerability

Do not open a public issue for a security problem.

Use GitHub's private reporting instead:
**Security → Report a vulnerability** on
<https://github.com/FireBolt922/space-optimiser/security/advisories/new>.

Include what you can of:

- the operating system and app version (About screen),
- the catalog row involved (for example `user-temp` or `browser-http-cache`),
- the folder layout, symlink, or environment variable that triggers it,
- what the app did and what you expected,
- the audit log lines for that run, if you are comfortable sharing them.

You will get an acknowledgement within 7 days. Fixes for confirmed reports
ship in the next release, and the release notes credit the reporter unless
they ask otherwise.

## Supported versions

Only the latest release receives fixes. Pre-release test builds are
unsigned and are not supported once the next build is published.

## What is in scope

- Any path outside the allowlisted roots being deleted or modified.
- Following a symlink, junction, or reparse point out of a root.
- Deleting personal documents, photos, downloads, source trees,
  credentials, or application configuration.
- Emptying the Trash or Recycle Bin, pruning container volumes, or
  deleting virtual-disk images.
- Running a shell command, or a tool with arguments that are not fixed.
- Elevation (running as root or administrator).
- Any network request made by the app itself.
- A Tauri command that accepts a path from the window.

## What is out of scope

- An app that rebuilds a cache after a confirmed cleanup.
- Denial of service against the app on the user's own machine.
- Vulnerabilities in the operating system's WebView.

The enforceable rules are listed in [AGENTS.md](AGENTS.md); tests for each
one live in `crates/engine/tests/safety.rs`.

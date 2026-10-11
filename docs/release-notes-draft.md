# Space Optimiser 0.1.0 — release notes (draft)

This is a draft. Fill in every "TBD" from a real test before a tag is made.
Do not publish a section for an operating system that was not tested.

## Tested on

| OS | Version tested | Package | Status |
| --- | --- | --- | --- |
| macOS | TBD | `.dmg` (and `.app`) | Not yet tested on macOS. |
| Ubuntu | TBD | `.deb` | Not yet tested on Ubuntu. |
| Windows | TBD | `.exe` (NSIS) and `.msi` | Not yet tested on Windows. |

Releases go out one OS at a time: macOS, then Ubuntu, then Windows. A
release names only the OSes it tested. Remove a row before publishing if
that OS is not part of this release.

## What Space Optimiser does

Space Optimiser frees disk space by clearing caches and old temporary files
in a fixed list of locations. It shows what it found, explains each item,
and removes only the items you check and confirm.

- **Normal Scan** looks at your temporary folder (files more than a day
  old), browser caches (Chrome, Edge, Brave, Firefox, and Chromium on
  Ubuntu), and on Ubuntu the thumbnail and font caches. On Windows,
  Explorer's thumbnail database is shown for information only.
- **Deep Scan** adds developer caches: npm, npx, pnpm, Yarn, pip, uv, Cargo,
  Gradle, and NuGet, old log files on macOS, Docker build cache when Docker
  runs on this computer, and a list of large files in your home folder.
  The large-file list is for information only; the app never removes those
  files. Deep Scan does not walk the whole disk.

## What it does not do

- Cleanups run only after you review the list and confirm.
- **Select safe items** picks only low-risk items: old temporary files,
  and on Ubuntu thumbnails and font caches (old logs on macOS in Deep
  Scan). It does not include browser caches or package caches; check those
  yourself if you want them removed.
- Docker disk files (such as `Docker.raw`) and Docker volumes are not
  cleaned. The Trash and the Recycle Bin are not emptied.
- Your documents, photos, downloads, and source code folders are outside
  every cleanup rule.
- The app does not run as administrator or root. If it is started that
  way, it refuses to scan.
- The app does not upload a list of your files. It makes no network
  requests of its own.
- The app does not update itself. To update, download a new version from
  the release page.

## Before you clean

- **Cleanups cannot be undone; files do not go to the Trash.**
- Apps rebuild caches as they run, and package managers download packages
  again when a project needs them. A first build or page load after a
  cleanup can be slower.
- **Recovered space can be smaller than the estimate.** The free-space
  number your system shows can lag or hide what was removed:
  - Local snapshots keep deleted files until the snapshot expires (Time
    Machine local snapshots on macOS, Btrfs or ZFS snapshots on Ubuntu,
    restore points on Windows).
  - macOS can count space as "purgeable" and report it later.
  - Some file systems free space a little after the delete.
  - Space removed inside Docker's disk file may not return to your computer
    until Docker restarts.
- The audit log records what each cleanup removed. Its location is shown on
  the About screen. Space Optimiser never deletes the log; you can delete
  it yourself.

## Install notes

### macOS

- Requires macOS 14 or later.
- Signing: TBD. If this build is unsigned and not notarized, say so here:
  "This macOS build is unsigned. macOS will warn before opening it." An
  unsigned build is a test build, not a finished public release.

### Ubuntu

- The `.deb` depends on the WebKitGTK packages Tauri 2 needs:
  `libwebkit2gtk-4.1-0` and `libgtk-3-0`. These are available on Ubuntu
  22.04 and later. `apt` installs them with the package.
- Signing: Ubuntu packages in this release are not signed.

### Windows

- The installer uses the WebView2 bootstrapper. If WebView2 is not already
  installed (it ships with current Windows 10 and 11), the installer
  downloads it from Microsoft during install. The app itself makes no
  network requests.
- Signing: TBD. If this build has no Authenticode signature, say so here:
  "This Windows build is unsigned. SmartScreen will warn before running
  it." An unsigned build is a test build, not a finished public release.

## Licences

Space Optimiser bundles the Inter typeface, © The Inter Project Authors,
under the SIL Open Font License 1.1. The licence text ships with the app
(`licences/Inter-OFL-1.1.txt` in the installed resources) and is shown on
the About screen.

## Release checklist

Copy this list into the release issue and tick each item for every OS this
release names.

- [ ] CI is green on the commit being tagged, for every OS in this release.
- [ ] The per-OS manual checklist is complete for every OS in this release.
      Any OS without one is listed above as "not yet tested on …".
- [ ] Install test on a clean user account for each OS: the package
      installs, and the app starts with the network disconnected.
- [ ] The installed app's capability list still grants no shell or file
      system access, and the app loads no remote content.
- [ ] Privacy check: a packet capture or proxy during a scan and a cleanup
      shows no connections from the app (webview internals noted
      separately if any appear).
- [ ] Dependency review: no analytics or network client in the engine or UI
      dependencies.
- [ ] Packaged-app test: a scan of a real temp folder and a cleanup limited
      to a file the tester created and aged. The audit log shows that file
      and no others, or the tester stopped before confirm. Notes say which.
- [ ] These release notes match the cleanup catalog in this version, and
      use no absolute safety promise.
- [ ] Signing status is stated for each OS. An unsigned build is not
      announced as a finished public release.
- [ ] The About screen shows the version, the safety wording, "This app does
      not upload a list of your files", the audit log location, and the
      Inter licence.
- [ ] Artifacts contain only installers: no audit logs, fixture homes, or
      test output.

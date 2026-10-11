# Testing a Space Optimiser build

Thank you for testing. This guide takes about 30 minutes, plus one step the
next day. You need no technical knowledge beyond opening a terminal once.

**Read this first.** Space Optimiser deletes files, and **cleanups cannot be
undone; files do not go to the Trash.** In this test you only confirm a
cleanup of one file you create yourself. For everything else, look and stop
before you press the final confirm button.

## 1. Download

1. Open the release page you were sent (a "Pre-release" on GitHub).
2. Download the file for your computer:

   | Your computer | File |
   | --- | --- |
   | Mac with macOS 14 or later | the `.dmg` |
   | Ubuntu 22.04 or later | the `.deb` |
   | Windows 11 (Windows 10 also welcome) | the file ending in `-setup.exe` |

3. Optional: compare the file's SHA-256 with `SHA256SUMS.txt` on the same
   page.
   - macOS / Ubuntu: `shasum -a 256 <file>` or `sha256sum <file>`
   - Windows (PowerShell): `Get-FileHash <file>`

## 2. Install

These test builds are **not signed**, so your system will warn you. That is
expected for a test build. Write down the exact warning you saw.

**macOS**

1. Open the `.dmg` and drag Space Optimiser to Applications.
2. Open it. macOS says it cannot verify the app. Click **Done**.
3. Open System Settings → Privacy & Security, scroll down, and click
   **Open Anyway** next to Space Optimiser. Confirm with your password.
4. If macOS instead says the app "is damaged", run this once in Terminal
   and open the app again:
   `xattr -dr com.apple.quarantine "/Applications/Space Optimiser.app"`

**Ubuntu**

1. In a terminal, in the download folder:
   `sudo apt install ./<file>.deb`
2. Open Space Optimiser from the app menu. Do **not** start it with `sudo`.

**Windows**

1. Run the `-setup.exe`. If SmartScreen says "Windows protected your PC",
   click **More info** → **Run anyway**.
2. Start Space Optimiser from the Start menu as your normal user. Do **not**
   use "Run as administrator"; the app refuses to scan when elevated, and
   that refusal is something you can check (step 4).

## 3. First start

- [ ] The window opens. Note how long it took.
- [ ] Turn off Wi-Fi and start the app again. It still opens and works.
- [ ] **About** shows a version number, says the app does not upload a list
      of your files, and shows where the audit log is kept.
- [ ] **Help** opens inside the app.
- [ ] **Settings**: switch between Dark, Light, and Match system. Text stays
      readable in each.

## 4. Look, but do not clean

1. Run **Normal Scan**. Note how long it took.
2. Read the list. For each row, check that the description makes sense.
   - [ ] Nothing from Documents, Desktop, Downloads, Pictures, or your
         project folders appears.
   - [ ] With a browser open, the app says its cache is not offered.
         (Quit the browser and scan again to see it offered.)
3. Click **Select safe items** and see what it picks. Go to the review
   screen, read it, and **stop** — press Back, not confirm.
4. Run **Deep Scan**. If you use npm, pip, Cargo, Docker, and similar tools,
   their caches should appear. Press **Cancel** once during a scan and note
   whether it stopped quickly.
   - [ ] The large-file list is information only; nothing in it can be
         checked.
5. Try **Show in folder** on one row. Your file manager opens there.
6. Keyboard: unplug or ignore the mouse. Can you scan, check a row, and
   reach the review screen with Tab, Space, and Enter only?
7. Windows only: start the app with right-click → **Run as administrator**.
   - [ ] It refuses to scan and says why.

## 5. One real cleanup (next day)

The app only removes temporary files more than a day old, so make a test
file today and clean it tomorrow. On macOS and Ubuntu the age cannot be
faked; you really need to wait a day.

**Today**, make the file:

- macOS (Terminal): `echo test > "$TMPDIR/space-optimiser-test.txt"`
- Ubuntu (terminal): `echo test > /tmp/space-optimiser-test.txt`
  (Ubuntu empties `/tmp` when you restart, so do not restart before
  tomorrow.)
- Windows (PowerShell): you can make it "old" at once:

  ```powershell
  $f = "$env:TEMP\space-optimiser-test.txt"; "test" | Out-File $f
  $old = (Get-Date).AddDays(-3)
  (Get-Item $f).CreationTime = $old; (Get-Item $f).LastWriteTime = $old
  ```

**Tomorrow** (Windows: right away):

1. Run Normal Scan.
   - [ ] A temporary files row is listed.
2. Check **only** the temporary files row. In review, make sure the total
   looks small and the list is what you expect. Then confirm.
3. On the report screen:
   - [ ] The report shows what was removed and how free space changed.
   - [ ] **Export report** saves a text file to your Downloads folder.
   - [ ] The test file is gone (`ls` / `dir` the path you used above).
   - [ ] The audit log (location on the About screen) lists the files that
         were removed.

If anything in the review list surprises you, do not confirm. Take a
screenshot and report it instead.

## 6. Report back

Send the following (a message or a GitHub issue is fine):

- Your system and version (for example "macOS 15.2, M2" or "Windows 11 23H2").
- The app version from About.
- The install warning you saw, word for word if you can.
- Each checkbox above: pass, fail, or skipped.
- Anything confusing, slow, or alarming, with a screenshot.
- The exported report from step 5.

Please do **not** send the audit log or screenshots that show private
file names unless you are happy to share them.

## Uninstall

- macOS: drag Space Optimiser from Applications to the Trash.
- Ubuntu: `sudo apt remove space-optimiser`
- Windows: Settings → Apps → Space Optimiser → Uninstall.

The audit log stays where About said it was; delete it yourself if you
want.

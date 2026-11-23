#!/bin/sh
# Copies the basic fixture home into a new temporary directory for a manual
# run of the window (phase 7 and phase 8 acceptance). Only debug builds read
# SPACE_OPTIMISER_FIXTURE_HOME; release builds always use this computer's
# folders.
#
# The fixture clock runs two days ahead, so the copied files count as old.
# tmp/new-download.part is dated two days ahead, so it counts as new.
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
dest="$(mktemp -d)/home"
cp -R "$repo/fixtures/homes/basic" "$dest"
printf 'partial download\n' > "$dest/tmp/new-download.part"
if stamp=$(date -v+2d +%Y%m%d%H%M 2>/dev/null); then :; else
  stamp=$(date -d '+2 days' +%Y%m%d%H%M)
fi
touch -t "$stamp" "$dest/tmp/new-download.part"

# Content for the tools added in v0.5. It is made here, not checked in: a
# `.git` folder inside the repository's fixtures would be a nested
# repository, and `target/` is ignored by git.
ago() { # a `touch -t` stamp N days back
  date -v-"$1"d +%Y%m%d%H%M 2>/dev/null || date -d "$1 days ago" +%Y%m%d%H%M
}
file() { # file <path> <bytes>
  mkdir -p "$(dirname "$1")"
  head -c "$2" /dev/zero > "$1"
}

# Chrome's on-device AI model, in each OS's user-data folder.
for chrome in "Library/Application Support/Google/Chrome" ".config/google-chrome" \
  "AppData/Local/Google/Chrome/User Data"; do
  file "$dest/$chrome/OptGuideOnDeviceModel/2025.8.21.1028/weights.bin" 4194304
done

# Projects: one untouched for 90 days, one active, one outside source control.
old="$dest/Projects/old-shop"
file "$old/node_modules/left-pad/index.js" 1048576
printf '{}\n' > "$old/package.json"
mkdir -p "$old/.git" "$old/src"
printf 'ref: refs/heads/main\n' > "$old/.git/HEAD"
printf 'export {};\n' > "$old/src/index.js"
find "$old" -exec touch -h -t "$(ago 90)" {} +

active="$dest/Projects/engine"
file "$active/target/debug/engine" 2097152
printf 'Signature: 8a477f597d28d172789f06886806bc55\n' > "$active/target/CACHEDIR.TAG"
printf '[package]\nname = "engine"\n' > "$active/Cargo.toml"
mkdir -p "$active/.git" "$active/src"
printf 'ref: refs/heads/main\n' > "$active/.git/HEAD"
printf 'fn main() {}\n' > "$active/src/main.rs"

app="$dest/Tools/some-app/resources/app"
file "$app/node_modules/runtime/index.js" 524288
printf '{}\n' > "$app/package.json"

# Android: one emulator on one image, and one image nothing uses.
for sdk in "Library/Android/sdk" "Android/Sdk" "AppData/Local/Android/Sdk"; do
  file "$dest/$sdk/system-images/android-36.1/google_apis_playstore/arm64-v8a/system.img" 2097152
  file "$dest/$sdk/system-images/android-30/default/x86_64/system.img" 1048576
done
avd="$dest/.android/avd"
mkdir -p "$avd/pixel_9.avd"
printf 'path.rel=avd/pixel_9.avd\ntarget=android-36\n' > "$avd/pixel_9.ini"
printf 'avd.ini.displayname=Pixel 9\nimage.sysdir.1=system-images/android-36.1/google_apis_playstore/arm64-v8a/\n' \
  > "$avd/pixel_9.avd/config.ini"
file "$avd/pixel_9.avd/userdata-qemu.img.qcow2" 1048576
file "$avd/pixel_9.avd/snapshots/default_boot/ram.bin" 2097152
file "$avd/pixel_9.avd/snapshots/before-login/ram.bin" 1048576

echo "Fixture home: $dest"
echo "Run the window against it with:"
echo "  SPACE_OPTIMISER_FIXTURE_HOME=\"$dest\" npm --prefix apps/desktop run tauri dev"

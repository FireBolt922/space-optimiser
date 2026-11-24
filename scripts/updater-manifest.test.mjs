// Tests for the updater manifest. Run with `node --test scripts/`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { buildManifest } from "./updater-manifest.mjs";

const REPO = "FireBolt922/space-optimiser";
const BASE = `https://github.com/${REPO}/releases/download/v0.6.0/`;

const FILES = [
  { name: "Space.Optimiser.app.tar.gz", signature: "sig-mac" },
  { name: "Space.Optimiser_0.6.0_x64-setup.exe", signature: "sig-nsis" },
  { name: "Space.Optimiser_0.6.0_x64_en-US.msi", signature: "sig-msi" },
  { name: "Space.Optimiser_0.6.0_amd64.deb", signature: "sig-deb" },
];

function manifest(files = FILES) {
  return buildManifest({
    version: "0.6.0",
    notes: "Added things.",
    pubDate: "2026-11-24T10:00:00Z",
    repo: REPO,
    files,
  });
}

test("each platform points at its installer and that installer's signature", () => {
  const { platforms } = manifest();
  assert.deepEqual(platforms["darwin-aarch64"], {
    url: `${BASE}Space.Optimiser.app.tar.gz`,
    signature: "sig-mac",
  });
  assert.deepEqual(platforms["darwin-x86_64"], platforms["darwin-aarch64"]);
  assert.equal(platforms["windows-x86_64-nsis"].signature, "sig-nsis");
  assert.equal(platforms["windows-x86_64"].signature, "sig-nsis");
  assert.equal(platforms["windows-x86_64-msi"].signature, "sig-msi");
  assert.equal(platforms["linux-x86_64-deb"].signature, "sig-deb");
  assert.equal(platforms["linux-x86_64"].url, `${BASE}Space.Optimiser_0.6.0_amd64.deb`);
});

test("the version, notes, and date are copied as given", () => {
  const m = manifest();
  assert.equal(m.version, "0.6.0");
  assert.equal(m.notes, "Added things.");
  assert.equal(m.pub_date, "2026-11-24T10:00:00Z");
});

test("a dmg, the checksums, and signature files are not platforms", () => {
  const m = manifest([
    ...FILES,
    { name: "Space.Optimiser_0.6.0_universal.dmg", signature: null },
    { name: "SHA256SUMS.txt", signature: null },
  ]);
  assert.equal(Object.keys(m.platforms).length, 9);
});

test("an installer without a signature stops the publish", () => {
  assert.throws(
    () => manifest([{ name: "Space.Optimiser_0.6.0_amd64.deb", signature: null }]),
    /no signature for Space\.Optimiser_0\.6\.0_amd64\.deb/,
  );
});

test("a missing platform stops the publish", () => {
  assert.throws(() => manifest(FILES.slice(1)), /no macOS update/);
});

test("a file name GitHub would rename stops the publish", () => {
  assert.throws(
    () => manifest([...FILES, { name: "Space Optimiser.app.tar.gz", signature: "x" }]),
    /GitHub renames/,
  );
});

test("the version must be plain major.minor.patch", () => {
  assert.throws(
    () => buildManifest({ version: "v0.6", notes: "", pubDate: "x", repo: REPO, files: FILES }),
    /version/,
  );
});

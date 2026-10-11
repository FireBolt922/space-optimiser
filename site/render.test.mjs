// Tests for the download page renderer. Run with `node --test site/`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { classifyAsset, formatSize, prepareReleases, renderPage } from "./render.mjs";

const REPO = "FireBolt922/space-optimiser";

function asset(name, size = 1000) {
  return {
    name,
    size,
    browser_download_url: `https://github.com/${REPO}/releases/download/vX/${encodeURIComponent(name)}`,
  };
}

function release(version, overrides = {}) {
  return {
    tag_name: `v${version}`,
    name: `Space Optimiser ${version}`,
    html_url: `https://github.com/${REPO}/releases/tag/v${version}`,
    draft: false,
    prerelease: true,
    published_at: "2026-10-11T08:00:00Z",
    assets: [
      asset(`Space.Optimiser_${version}_universal.dmg`, 6_618_007),
      asset(`Space.Optimiser_${version}_amd64.deb`, 3_389_120),
      asset(`Space.Optimiser_${version}_x64-setup.exe`, 2_172_112),
      asset(`Space.Optimiser_${version}_x64_en-US.msi`, 3_244_032),
      asset("SHA256SUMS.txt", 404),
    ],
    ...overrides,
  };
}

test("classifyAsset maps installer names to a system", () => {
  assert.equal(classifyAsset("Space.Optimiser_0.1.0_universal.dmg"), "macos");
  assert.equal(classifyAsset("Space.Optimiser_0.1.0_amd64.deb"), "ubuntu");
  assert.equal(classifyAsset("Space.Optimiser_0.1.0_x64-setup.exe"), "windows");
  assert.equal(classifyAsset("Space.Optimiser_0.1.0_x64_en-US.msi"), "windowsMsi");
  assert.equal(classifyAsset("SHA256SUMS.txt"), "checksums");
  assert.equal(classifyAsset("notes.md"), null);
  assert.equal(classifyAsset("something.exe"), null, "only the NSIS -setup.exe");
});

test("formatSize uses one decimal in MB and KB", () => {
  assert.equal(formatSize(6_618_007), "6.3 MB");
  assert.equal(formatSize(404), "404 B");
  assert.equal(formatSize(2_048), "2.0 KB");
});

test("prepareReleases drops drafts and sorts newest first", () => {
  const list = prepareReleases([
    release("0.1.0", { published_at: "2026-10-01T00:00:00Z" }),
    release("0.3.0", { draft: true, published_at: null }),
    release("0.2.0", { published_at: "2026-10-09T00:00:00Z", prerelease: false }),
  ]);
  assert.deepEqual(
    list.map((r) => r.version),
    ["0.2.0", "0.1.0"],
  );
  assert.equal(list[0].prerelease, false);
  assert.equal(list[0].date, "2026-10-09");
  assert.equal(list[1].downloads.macos.size, "6.3 MB");
  assert.match(list[1].downloads.ubuntu.url, /amd64\.deb$/);
  assert.equal(list[1].downloads.checksums.name, "SHA256SUMS.txt");
});

test("prepareReleases keeps a release that lacks some installers", () => {
  const [only] = prepareReleases([release("0.1.0", { assets: [asset("x_0.1.0_universal.dmg")] })]);
  assert.ok(only.downloads.macos);
  assert.equal(only.downloads.windows, undefined);
});

test("renderPage shows the latest release with each installer link", () => {
  const html = renderPage(prepareReleases([release("0.1.0")]), { repo: REPO });
  assert.match(html, /<!doctype html>/i);
  assert.match(html, /Version 0\.1\.0/);
  assert.match(html, /Test build/, "a pre-release is labelled");
  for (const name of [
    "Space.Optimiser_0.1.0_universal.dmg",
    "Space.Optimiser_0.1.0_amd64.deb",
    "Space.Optimiser_0.1.0_x64-setup.exe",
    "Space.Optimiser_0.1.0_x64_en-US.msi",
    "SHA256SUMS.txt",
  ]) {
    assert.ok(html.includes(`/${encodeURIComponent(name)}"`), `links ${name}`);
  }
});

test("renderPage gives the main button a download per system", () => {
  const html = renderPage(prepareReleases([release("0.1.0")]), { repo: REPO });
  assert.match(html, /data-href-macos="[^"]+universal\.dmg"/);
  assert.match(html, /data-href-ubuntu="[^"]+amd64\.deb"/);
  assert.match(html, /data-href-windows="[^"]+x64-setup\.exe"/);
  // Without the script the button goes to the download table.
  assert.match(html, /id="primary-download"[^>]*href="#download"/);
});

test("renderPage lists earlier releases and does not label a full release", () => {
  const html = renderPage(
    prepareReleases([
      release("0.2.0", { prerelease: false, published_at: "2026-10-20T00:00:00Z" }),
      release("0.1.0"),
    ]),
    { repo: REPO },
  );
  const [latest, earlier] = html.split('id="earlier"');
  assert.ok(earlier, "has an earlier-releases section");
  assert.match(latest, /Version 0\.2\.0/);
  assert.doesNotMatch(latest, /Test build/);
  assert.match(earlier, /0\.1\.0/);
  assert.match(earlier, /Space\.Optimiser_0\.1\.0_universal\.dmg/);
});

test("renderPage says when nothing is released yet", () => {
  const html = renderPage([], { repo: REPO });
  assert.match(html, /No release yet/);
  assert.doesNotMatch(html, /id="earlier"/);
});

test("renderPage escapes text that comes from the release", () => {
  const html = renderPage(
    prepareReleases([release("0.1.0", { name: '<img src=x onerror="alert(1)">' })]),
    { repo: REPO },
  );
  assert.doesNotMatch(html, /<img src=x/);
});

test("renderPage has no inline styles or inline scripts", () => {
  const html = renderPage(prepareReleases([release("0.1.0")]), { repo: REPO });
  assert.doesNotMatch(html, /\sstyle=/);
  assert.doesNotMatch(html, /<script(?![^>]*\ssrc=)[^>]*>/);
  assert.doesNotMatch(html, /\son[a-z]+=/);
  assert.match(html, /http-equiv="Content-Security-Policy"/);
});

test("renderPage links only to GitHub and the page's own files", () => {
  const html = renderPage(prepareReleases([release("0.1.0")]), { repo: REPO });
  const urls = [...html.matchAll(/(?:href|src)="([^"]+)"/g)].map((m) => m[1]);
  for (const url of urls) {
    assert.ok(
      url.startsWith("#") || url.startsWith(`https://github.com/${REPO}`) || !url.includes(":"),
      `unexpected link ${url}`,
    );
  }
});

// Renders the download page from the GitHub releases API response. Pure
// functions only: build.mjs does the reading and writing. Every value that
// comes from a release passes through esc() before it reaches the page.

const SYSTEMS = [
  {
    key: "macos",
    label: "macOS",
    detail: "macOS 14 or later, Apple silicon and Intel",
    kind: ".dmg",
  },
  {
    key: "ubuntu",
    label: "Ubuntu",
    detail: "Ubuntu 22.04 or later, 64-bit",
    kind: ".deb",
  },
  {
    key: "windows",
    label: "Windows",
    detail: "Windows 11, 64-bit (Windows 10 best effort)",
    kind: "-setup.exe",
  },
];

/** Which download a release file is, from its name; null for anything else. */
export function classifyAsset(name) {
  if (name === "SHA256SUMS.txt") return "checksums";
  if (name.endsWith(".dmg")) return "macos";
  if (name.endsWith(".deb")) return "ubuntu";
  if (name.endsWith("-setup.exe")) return "windows";
  if (name.endsWith(".msi")) return "windowsMsi";
  return null;
}

export function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Published releases, newest first, with their installers by system. */
export function prepareReleases(apiReleases) {
  return apiReleases
    .filter((r) => !r.draft && r.published_at)
    .sort((a, b) => b.published_at.localeCompare(a.published_at))
    .map((r) => {
      const downloads = {};
      for (const a of r.assets ?? []) {
        const key = classifyAsset(a.name);
        if (key && !downloads[key]) {
          downloads[key] = { name: a.name, url: a.browser_download_url, size: formatSize(a.size) };
        }
      }
      return {
        version: r.tag_name.replace(/^v/, ""),
        tag: r.tag_name,
        title: r.name || r.tag_name,
        url: r.html_url,
        date: r.published_at.slice(0, 10),
        prerelease: Boolean(r.prerelease),
        downloads,
      };
    });
}

function esc(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

const CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "font-src 'self'",
  "img-src 'self'",
  "base-uri 'none'",
  "form-action 'none'",
].join("; ");

function badge(release) {
  return release.prerelease ? `<span class="badge">Test build</span>` : "";
}

function primaryButton(latest) {
  if (!latest) {
    return `<a id="primary-download" class="button primary" href="#download">See downloads</a>`;
  }
  const attrs = SYSTEMS.filter((s) => latest.downloads[s.key])
    .map(
      (s) =>
        ` data-href-${s.key}="${esc(latest.downloads[s.key].url)}"` +
        ` data-label-${s.key}="Download for ${esc(s.label)}"`,
    )
    .join("");
  return `<a id="primary-download" class="button primary" href="#download"${attrs}>Choose your download</a>`;
}

function downloadRow(system, release) {
  const file = release.downloads[system.key];
  const msi = system.key === "windows" ? release.downloads.windowsMsi : undefined;
  const cell = file
    ? `<a class="file" href="${esc(file.url)}">${esc(file.name)}</a>
          <span class="size">${esc(file.size)}</span>`
    : `<span class="missing">Not in this release</span>`;
  const alt = msi
    ? `<span class="alt">or <a href="${esc(msi.url)}">the .msi installer</a> (${esc(msi.size)})</span>`
    : "";
  return `
        <li class="system" data-os="${system.key}">
          <div class="system-name">
            <span class="heading">${esc(system.label)}</span>
            <span class="yours" hidden>Your system</span>
            <span class="body-sm muted">${esc(system.detail)}</span>
          </div>
          <div class="system-file">${cell}${alt}</div>
        </li>`;
}

function latestSection(latest, repo) {
  if (!latest) {
    return `
    <section id="download" class="section" aria-labelledby="download-title">
      <h2 id="download-title" class="title-xl">No release yet</h2>
      <p class="muted">The first build will appear here when it is published. Follow the
        <a href="https://github.com/${esc(repo)}/releases">releases on GitHub</a> in the meantime.</p>
    </section>`;
  }
  const guide = `https://github.com/${esc(repo)}/blob/${esc(latest.tag)}/docs/testing-guide.md`;
  const note = latest.prerelease
    ? `<p class="callout">This is a test build for people helping to try the app. It is not
        ready for general use yet. The <a href="${guide}">testing guide</a> says what to check
        and how to report back.</p>`
    : "";
  const sums = latest.downloads.checksums
    ? `<a href="${esc(latest.downloads.checksums.url)}">SHA256SUMS.txt</a>`
    : "";
  return `
    <section id="download" class="section" aria-labelledby="download-title">
      <div class="section-head">
        <h2 id="download-title" class="title-xl">Version ${esc(latest.version)}</h2>
        ${badge(latest)}
        <span class="body-sm muted">Released ${esc(latest.date)}</span>
      </div>
      ${note}
      <ul class="systems">${SYSTEMS.map((s) => downloadRow(s, latest)).join("")}
      </ul>
      <p class="body-sm muted links">
        <a href="${esc(latest.url)}">Release notes</a>
        ${sums ? `<span aria-hidden="true">·</span> Check your download against ${sums}` : ""}
      </p>
    </section>`;
}

function earlierSection(earlier) {
  if (earlier.length === 0) return "";
  const items = earlier
    .map((r) => {
      const files = [
        ["macos", "macOS .dmg"],
        ["ubuntu", "Ubuntu .deb"],
        ["windows", "Windows .exe"],
        ["windowsMsi", "Windows .msi"],
        ["checksums", "Checksums"],
      ]
        .filter(([key]) => r.downloads[key])
        .map(
          ([key, label]) =>
            `<a href="${esc(r.downloads[key].url)}" title="${esc(r.downloads[key].name)}">${label}</a>`,
        )
        .join(`<span aria-hidden="true">·</span>`);
      return `
        <li class="release">
          <div class="release-head">
            <a class="heading" href="${esc(r.url)}">${esc(r.version)}</a>
            ${badge(r)}
            <span class="body-sm muted">${esc(r.date)}</span>
          </div>
          <div class="body-sm release-files">${files}</div>
        </li>`;
    })
    .join("");
  return `
    <section id="earlier" class="section" aria-labelledby="earlier-title">
      <h2 id="earlier-title" class="title-lg">Earlier releases</h2>
      <ul class="releases">${items}
      </ul>
    </section>`;
}

const INSTALL_HELP = `
    <section id="install" class="section" aria-labelledby="install-title">
      <h2 id="install-title" class="title-lg">Opening an unsigned build</h2>
      <p class="muted">The installers are not signed yet, so your system warns you the first
        time. That is expected for now.</p>
      <div class="install-grid">
        <details class="card">
          <summary class="heading">macOS</summary>
          <ol class="body-sm">
            <li>Open the <code>.dmg</code> and drag Space Optimiser to Applications.</li>
            <li>Open it. When macOS says it cannot verify the app, click <strong>Done</strong>.</li>
            <li>In System Settings → Privacy &amp; Security, click <strong>Open Anyway</strong>
              next to Space Optimiser.</li>
            <li>If macOS says the app "is damaged", run this once in Terminal, then open it again:
              <code>xattr -dr com.apple.quarantine "/Applications/Space Optimiser.app"</code></li>
          </ol>
        </details>
        <details class="card">
          <summary class="heading">Ubuntu</summary>
          <ol class="body-sm">
            <li>In a terminal, in the download folder:
              <code>sudo apt install ./&lt;file&gt;.deb</code></li>
            <li>Open Space Optimiser from the app menu. Do not start it with <code>sudo</code>.</li>
          </ol>
        </details>
        <details class="card">
          <summary class="heading">Windows</summary>
          <ol class="body-sm">
            <li>Run the <code>-setup.exe</code>. If SmartScreen says "Windows protected your PC",
              click <strong>More info</strong> → <strong>Run anyway</strong>.</li>
            <li>Start Space Optimiser from the Start menu as your normal user, not as
              administrator.</li>
          </ol>
        </details>
      </div>
    </section>`;

const ABOUT = `
    <section id="about" class="section" aria-labelledby="about-title">
      <h2 id="about-title" class="title-lg">What it does</h2>
      <div class="about-grid">
        <article class="card">
          <h3 class="heading">Normal Scan</h3>
          <p class="body-sm muted">Old files in your temporary folder, browser caches for Chrome,
            Edge, Brave, Chromium, and Firefox (only while the browser is closed), and on Ubuntu
            the thumbnail and font caches.</p>
        </article>
        <article class="card">
          <h3 class="heading">Deep Scan</h3>
          <p class="body-sm muted">Adds developer caches (npm, pnpm, Yarn, pip, uv, Cargo, Gradle,
            NuGet), old logs on macOS, the Docker build cache, and a list of large files that is
            for information only.</p>
        </article>
        <article class="card">
          <h3 class="heading">What it leaves alone</h3>
          <p class="body-sm muted">Documents, photos, downloads, source code, the Trash, Docker
            volumes, virtual disks, settings, and passwords. It does not ask for an administrator
            password and does not upload a list of your files.</p>
        </article>
      </div>
      <p class="callout">Nothing is deleted until you review the list and confirm. Cleanups
        cannot be undone; files do not go to the Trash. A cleanup can make an app rebuild a
        cache or download files again.</p>
    </section>`;

/** The whole page as an HTML string. */
export function renderPage(releases, { repo }) {
  const [latest, ...earlier] = releases;
  const hint = latest
    ? `Version ${esc(latest.version)}${latest.prerelease ? " · test build" : ""} · free and open source`
    : "Free and open source";
  const gh = `https://github.com/${esc(repo)}`;
  return `<!doctype html>
<html lang="en" data-theme="dark">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta http-equiv="Content-Security-Policy" content="${CSP}" />
    <title>Space Optimiser</title>
    <meta
      name="description"
      content="Space Optimiser finds caches and temporary files you can clear on macOS, Ubuntu, and Windows, and deletes only what you confirm."
    />
    <link rel="icon" href="icon.png" />
    <link rel="stylesheet" href="styles/tokens.css" />
    <link rel="stylesheet" href="site.css" />
    <script src="site.js"></script>
  </head>
  <body>
    <a class="skip" href="#download">Skip to downloads</a>
    <header class="topbar">
      <a class="brand heading" href="#top"><img src="icon.png" alt="" width="28" height="28" />Space Optimiser</a>
      <nav class="body-sm" aria-label="Page">
        <a href="#download">Download</a>
        <a href="#about">What it does</a>
        <a href="${gh}">Source code</a>
      </nav>
    </header>
    <main id="top">
      <section class="hero">
        <p class="eyebrow accent">macOS · Ubuntu · Windows</p>
        <h1 class="h1-sm">Clear disk space you can see, and only the parts you confirm.</h1>
        <p class="lead muted">Space Optimiser scans a fixed list of caches and temporary files,
          explains each one, and removes nothing until you have reviewed it.</p>
        <div class="actions">
          ${primaryButton(latest)}
          <span class="body-sm muted">${hint}</span>
        </div>
      </section>
      ${latestSection(latest, repo)}
      ${latest ? INSTALL_HELP : ""}
      ${ABOUT}
      ${earlierSection(earlier)}
    </main>
    <footer class="footer body-sm muted">
      <p>
        <a href="${gh}">GitHub</a>
        <span aria-hidden="true">·</span>
        <a href="${gh}/blob/main/CHANGELOG.md">Changelog</a>
        <span aria-hidden="true">·</span>
        <a href="${gh}/issues">Report a problem</a>
        <span aria-hidden="true">·</span>
        <a href="${gh}/blob/main/LICENSE">MIT licence</a>
      </p>
      <p>This page sets no cookies and has no analytics.</p>
    </footer>
  </body>
</html>
`;
}

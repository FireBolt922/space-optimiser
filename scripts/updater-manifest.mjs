// Writes latest.json, the manifest Tauri's updater reads (phase 15).
//
//   node scripts/updater-manifest.mjs --version 0.6.0 --repo OWNER/NAME \
//     --dir dist-release --notes notes.txt --out dist-release/latest.json
//
// Each update file needs its `.sig` next to it (made with the release's
// private key). The publish fails rather than ship a manifest that leaves
// a system out or points at an unsigned file.
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

/** File-name ending → the updater's platform keys, with the system's name. */
const KINDS = [
  {
    ending: ".app.tar.gz",
    system: "macOS",
    // One universal app serves both chips.
    keys: ["darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"],
  },
  { ending: "-setup.exe", system: "Windows", keys: ["windows-x86_64-nsis", "windows-x86_64"] },
  { ending: ".msi", system: "Windows", keys: ["windows-x86_64-msi"] },
  // Linux checks only; the app links to the download page (a .deb needs
  // an administrator password to install).
  { ending: ".deb", system: "Linux", keys: ["linux-x86_64-deb", "linux-x86_64"] },
];

/**
 * @param {{ version: string, notes: string, pubDate: string, repo: string,
 *   files: { name: string, signature: string | null }[] }} input
 */
export function buildManifest({ version, notes, pubDate, repo, files }) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`bad version: ${version}`);
  const platforms = {};
  for (const { name, signature } of files) {
    const kind = KINDS.find((k) => name.endsWith(k.ending));
    if (!kind) continue;
    if (/\s/.test(name)) {
      throw new Error(`GitHub renames "${name}" on upload (spaces become dots); rename it first`);
    }
    if (!signature) throw new Error(`no signature for ${name}`);
    const url = `https://github.com/${repo}/releases/download/v${version}/${encodeURIComponent(name)}`;
    for (const key of kind.keys) platforms[key] = { url, signature };
  }
  for (const system of ["macOS", "Windows", "Linux"]) {
    const keys = KINDS.filter((k) => k.system === system).flatMap((k) => k.keys);
    if (!keys.some((key) => key in platforms)) throw new Error(`no ${system} update file`);
  }
  return { version, notes, pub_date: pubDate, platforms };
}

async function main() {
  const { values } = parseArgs({
    options: {
      version: { type: "string" },
      repo: { type: "string" },
      dir: { type: "string" },
      notes: { type: "string" },
      out: { type: "string" },
    },
  });
  for (const key of ["version", "repo", "dir", "out"]) {
    if (!values[key]) throw new Error(`--${key} is required`);
  }
  const names = await readdir(values.dir);
  const files = [];
  for (const name of names) {
    if (name.endsWith(".sig")) continue;
    const sig = names.includes(`${name}.sig`)
      ? (await readFile(join(values.dir, `${name}.sig`), "utf8")).trim()
      : null;
    files.push({ name, signature: sig });
  }
  const notes = values.notes ? (await readFile(values.notes, "utf8")).trim() : "";
  const manifest = buildManifest({
    version: values.version,
    notes,
    pubDate: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    repo: values.repo,
    files,
  });
  await writeFile(values.out, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Wrote ${values.out}: ${Object.keys(manifest.platforms).join(", ")}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}

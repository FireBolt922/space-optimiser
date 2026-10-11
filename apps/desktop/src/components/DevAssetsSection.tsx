// Developer assets: Docker's images, containers, volumes, and disk file, and
// a table of the tool caches the scan looked at. Information only: nothing
// here has an id, so nothing can be selected, revealed, or removed.
import { useState } from "react";
import type { Candidate, DevAssets, DockerImage, DockerVolume, Note } from "../engine";
import { formatBytes, plural } from "../format";

interface Props {
  assets: DevAssets | null;
  /** The scan's candidates and notes, for the tool cache table. */
  candidates: Candidate[];
  notes: Note[];
}

/** Rows shown per table before "Show all". */
const TABLE_ROWS = 10;

const size = (bytes: number | null) => (bytes === null ? "Unknown" : formatBytes(bytes));
const bySize = <T extends { size_bytes: number | null }>(a: T, b: T) =>
  (b.size_bytes ?? 0) - (a.size_bytes ?? 0);

/** Anonymous volumes are named by a 64-character hash. */
function volumeName(v: DockerVolume): string {
  return /^[0-9a-f]{64}$/.test(v.name) ? `Unnamed volume (${v.name.slice(0, 12)})` : v.name;
}

function volumeUse(v: DockerVolume): string {
  if (v.in_use_by === null) return "Unknown";
  if (v.in_use_by === 0) return "Not used by a container right now";
  return plural(v.in_use_by, "container", "containers");
}

function imageName(i: DockerImage): string {
  if (i.repository === "<none>" || i.repository === "") return "Untagged image";
  return i.tag && i.tag !== "<none>" ? `${i.repository}:${i.tag}` : i.repository;
}

const capitalise = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** Tool and package cache rows, and what the scan said about each. */
function isToolRow(catalogId: string): boolean {
  return catalogId.startsWith("pkg-") || catalogId === "docker-build-cache";
}

function toolStatus(c: Candidate): string {
  if (c.action === "delete" || c.action === "run_tool") return "Offered";
  if (c.confidence === "uncertain") return "Not offered: not sure";
  return "Not offered";
}

export function DevAssetsSection({ assets, candidates, notes }: Props) {
  const [open, setOpen] = useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setOpen((set) => {
      const next = new Set(set);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  /** The first TABLE_ROWS rows, or all of them once opened. */
  const rows = <T,>(key: string, all: T[]) =>
    open.has(key) || all.length <= TABLE_ROWS ? all : all.slice(0, TABLE_ROWS);
  const more = (key: string, count: number, title: string) =>
    count > TABLE_ROWS && (
      <button
        type="button"
        className="button button-sm group-more"
        aria-expanded={open.has(key)}
        onClick={() => toggle(key)}
      >
        {open.has(key) ? `Show fewer in ${title}` : `Show all ${count} in ${title}`}
      </button>
    );

  const docker = assets?.docker ?? null;
  const volumes = [...(docker?.volumes ?? [])].sort(bySize);
  const images = [...(docker?.images ?? [])].sort(bySize);
  const containers = [...(docker?.containers ?? [])].sort(bySize);
  const tools = candidates.filter((c) => isToolRow(c.catalog_id));
  const toolNotes = notes.filter((n) => isToolRow(n.catalog_id));

  return (
    <section className="card group" aria-label="Developer assets">
      <div className="group-head">
        <h2 className="title">Developer assets</h2>
        <span className="badge">Info</span>
      </div>
      <p className="body-sm ink-muted">
        Information only. Space Optimiser never removes anything listed here.
      </p>

      {docker?.note && <p className="body-sm ink-muted">{docker.note}</p>}

      {volumes.length > 0 && (
        <div className="subgroup">
          <h3 id="assets-volumes" className="heading">
            Docker volumes
          </h3>
          <p className="body-sm ink-muted">
            Volumes hold databases and app data. Space Optimiser never removes them. Remove one
            yourself with Docker only if you are sure you no longer need its data.
          </p>
          <table className="assets-table body-sm" aria-labelledby="assets-volumes">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Size</th>
                <th scope="col">Used by</th>
              </tr>
            </thead>
            <tbody>
              {rows("volumes", volumes).map((v) => (
                <tr key={v.name}>
                  <td className="file-name">{volumeName(v)}</td>
                  <td>{size(v.size_bytes)}</td>
                  <td>{volumeUse(v)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {more("volumes", volumes.length, "Docker volumes")}
        </div>
      )}

      {images.length > 0 && (
        <div className="subgroup">
          <h3 id="assets-images" className="heading">
            Docker images
          </h3>
          <p className="body-sm ink-muted">
            Images share layers, so the sizes overlap. &quot;Only this image&quot; is the part no
            other image uses.
          </p>
          <table className="assets-table body-sm" aria-labelledby="assets-images">
            <thead>
              <tr>
                <th scope="col">Image</th>
                <th scope="col">Size</th>
                <th scope="col">Only this image</th>
                <th scope="col">Containers</th>
              </tr>
            </thead>
            <tbody>
              {rows("images", images).map((i, n) => (
                <tr key={`${i.repository}:${i.tag}:${n}`}>
                  <td className="file-name">{imageName(i)}</td>
                  <td>{size(i.size_bytes)}</td>
                  <td>{size(i.unique_bytes)}</td>
                  <td>{i.containers ?? "Unknown"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {more("images", images.length, "Docker images")}
        </div>
      )}

      {containers.length > 0 && (
        <div className="subgroup">
          <h3 id="assets-containers" className="heading">
            Docker containers
          </h3>
          <table className="assets-table body-sm" aria-labelledby="assets-containers">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Image</th>
                <th scope="col">State</th>
                <th scope="col">Size</th>
              </tr>
            </thead>
            <tbody>
              {rows("containers", containers).map((c, n) => (
                <tr key={`${c.name}:${n}`}>
                  <td className="file-name">{c.name}</td>
                  <td className="file-name">{c.image}</td>
                  <td>
                    {capitalise(c.state)}
                    {c.status && ` (${c.status})`}
                  </td>
                  <td>{size(c.size_bytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {more("containers", containers.length, "Docker containers")}
        </div>
      )}

      {docker?.disk_file && (
        <div className="subgroup">
          <h3 className="heading">Docker disk file</h3>
          <p className="body-sm">
            <span className="file-name">{docker.disk_file.location_label}</span> uses{" "}
            {formatBytes(docker.disk_file.allocated_bytes)} on disk (its maximum size is{" "}
            {formatBytes(docker.disk_file.logical_bytes)}).
          </p>
          <p className="body-sm ink-muted">
            It only shrinks after Docker frees space inside it and restarts. Space Optimiser never
            deletes it.
          </p>
        </div>
      )}

      {(tools.length > 0 || toolNotes.length > 0) && (
        <div className="subgroup">
          <h3 id="assets-tools" className="heading">
            Developer tool caches
          </h3>
          <table className="assets-table body-sm" aria-labelledby="assets-tools">
            <thead>
              <tr>
                <th scope="col">Cache</th>
                <th scope="col">Where</th>
                <th scope="col">Size</th>
                <th scope="col">Status</th>
              </tr>
            </thead>
            <tbody>
              {tools.map((c) => (
                <tr key={c.id}>
                  <th scope="row">{c.location_label}</th>
                  <td className="file-name">{c.display_name}</td>
                  <td>
                    {c.upper_bound_bytes !== null
                      ? `Up to ${formatBytes(c.upper_bound_bytes)}`
                      : formatBytes(c.estimated_bytes)}
                  </td>
                  <td>{toolStatus(c)}</td>
                </tr>
              ))}
              {toolNotes.map((n, i) => (
                <tr key={`note-${i}`}>
                  <th scope="row">{n.location_label}</th>
                  <td colSpan={3}>
                    {n.text}
                    {n.bytes !== null && ` (${formatBytes(n.bytes)})`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

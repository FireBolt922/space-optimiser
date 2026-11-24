// The Overview (phase 14): memory, CPU, the busiest processes, and the disk
// that holds the home folder. Read only: nothing here stops an app or
// removes a file. The readings take no arguments, and process names are
// rendered as text.
import { useEffect, useState } from "react";
import {
  errorMessage,
  type DiskUsage,
  type Engine,
  type Platform,
  type ProcessUsage,
  type SystemUsage,
} from "../engine";
import { formatBytes, formatMemory, plural } from "../format";

/** How often the figures are read while the Overview is shown. */
export const OVERVIEW_POLL_MS = 2000;

/** At or above this share, a meter or the disk chart shows a warning colour. */
const HIGH_PERCENT = 90;

interface Props {
  engine: Pick<Engine, "systemUsage" | "diskUsage">;
  platform: Platform | null;
  onCleanUp: () => void;
}

type Reading<T> = { value: T; error?: undefined } | { value?: T; error: string };

export function Overview({ engine, platform, onCleanUp }: Props) {
  const [system, setSystem] = useState<Reading<SystemUsage> | null>(null);
  const [disk, setDisk] = useState<Reading<DiskUsage> | null>(null);

  // Read now, then every 2 seconds while this view is mounted. A hidden
  // window skips its readings. Each reading keeps its last good value when
  // the other one fails.
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      if (typeof document === "undefined" || !document.hidden) {
        const [s, d] = await Promise.allSettled([engine.systemUsage(), engine.diskUsage()]);
        if (stopped) return;
        setSystem((last) =>
          s.status === "fulfilled"
            ? { value: s.value }
            : { value: last?.value, error: errorMessage(s.reason) },
        );
        setDisk((last) =>
          d.status === "fulfilled"
            ? { value: d.value }
            : { value: last?.value, error: errorMessage(d.reason) },
        );
      }
      if (!stopped) timer = setTimeout(tick, OVERVIEW_POLL_MS);
    };
    timer = setTimeout(tick, 0);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [engine]);

  return (
    <section className="screen screen-wide" aria-labelledby="overview-title">
      <h1 id="overview-title" className="title-xl" tabIndex={-1}>
        Overview
      </h1>
      <p className="body ink-muted">
        How this computer is doing now. The figures update every 2 seconds while this page is open.
      </p>
      <div className="overview-grid">
        <Performance reading={system} />
        <Disk reading={disk} platform={platform} onCleanUp={onCleanUp} />
      </div>
    </section>
  );
}

function Performance({ reading }: { reading: Reading<SystemUsage> | null }) {
  const usage = reading?.value;
  return (
    <section className="card overview-card" aria-labelledby="performance-title">
      <h2 id="performance-title" className="title">
        Performance
      </h2>
      {reading?.error && (
        <p className="notice notice-warning body-sm" role="alert">
          Could not read memory and CPU: {reading.error}
        </p>
      )}
      {!usage && !reading?.error && <p className="body-sm ink-muted">Reading…</p>}
      {usage && (
        <>
          <div className="gauges">
            <Gauge
              label="Memory"
              text={`${formatMemory(usage.memory.used_bytes)} of ${formatMemory(usage.memory.total_bytes)} in use`}
              percent={share(usage.memory.used_bytes, usage.memory.total_bytes)}
            />
            <Gauge
              label="CPU"
              text={
                usage.cpu_percent === null
                  ? "Measuring…"
                  : `${Math.round(usage.cpu_percent)}% of ${plural(usage.logical_cpus, "CPU", "CPUs")}`
              }
              percent={usage.cpu_percent}
            />
            {usage.swap.total_bytes > 0 && (
              <Gauge
                label="Swap"
                text={`${formatMemory(usage.swap.used_bytes)} of ${formatMemory(usage.swap.total_bytes)} in use`}
                percent={share(usage.swap.used_bytes, usage.swap.total_bytes)}
              />
            )}
          </div>
          <div className="process-lists">
            <ProcessTable
              id="top-memory"
              title="Most memory"
              column="Memory"
              rows={usage.top_memory}
              value={(p) => formatMemory(p.memory_bytes)}
              empty="No figures yet."
            />
            <ProcessTable
              id="top-cpu"
              title="Most CPU"
              column="CPU"
              rows={usage.top_cpu}
              value={(p) => `${(p.cpu_percent ?? 0).toFixed(1)}%`}
              empty={usage.cpu_percent === null ? "Measuring…" : "Nothing is busy."}
            />
          </div>
          <p className="caption ink-muted">
            Processes with the same name are added together, so memory they share can be counted
            more than once. CPU is a share of all {plural(usage.logical_cpus, "CPU", "CPUs")}. Space
            Optimiser only reads these figures; it does not stop or change any app.
          </p>
        </>
      )}
    </section>
  );
}

function Gauge({ label, text, percent }: { label: string; text: string; percent: number | null }) {
  return (
    <div className="gauge">
      <div className="gauge-head">
        <span className="label">{label}</span>
        <span className="body-sm ink-muted">{text}</span>
      </div>
      {percent !== null && <Meter label={label} percent={percent} />}
    </div>
  );
}

/** A horizontal bar. SVG attributes, not inline styles, set the width. */
function Meter({ label, percent }: { label: string; percent: number }) {
  const value = clamp(percent);
  return (
    <svg
      className={value >= HIGH_PERCENT ? "meter meter-high" : "meter"}
      viewBox="0 0 100 8"
      preserveAspectRatio="none"
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
      aria-valuetext={`${Math.round(value)}%`}
      focusable="false"
    >
      <rect className="meter-track" x="0" y="0" width="100" height="8" />
      <rect className="meter-fill" x="0" y="0" width={value} height="8" />
    </svg>
  );
}

interface TableProps {
  id: string;
  title: string;
  column: string;
  rows: ProcessUsage[];
  value: (p: ProcessUsage) => string;
  empty: string;
}

function ProcessTable({ id, title, column, rows, value, empty }: TableProps) {
  return (
    <div className="process-list">
      <h3 id={id} className="heading">
        {title}
      </h3>
      {rows.length === 0 ? (
        <p className="body-sm ink-muted">{empty}</p>
      ) : (
        <table className="assets-table process-table body-sm" aria-labelledby={id}>
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">{column}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((p) => (
              <tr key={p.name}>
                <td className="file-name">
                  <span>{p.name}</span>
                  {p.count > 1 && <span className="ink-muted"> × {p.count}</span>}
                </td>
                <td>{value(p)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

interface DiskProps {
  reading: Reading<DiskUsage> | null;
  platform: Platform | null;
  onCleanUp: () => void;
}

function Disk({ reading, platform, onCleanUp }: DiskProps) {
  const disk = reading?.value;
  return (
    <section className="card overview-card" aria-labelledby="disk-title">
      <h2 id="disk-title" className="title">
        Disk
      </h2>
      {reading?.error && (
        <p className="notice notice-warning body-sm" role="alert">
          Could not read the disk: {reading.error}
        </p>
      )}
      {!disk && !reading?.error && <p className="body-sm ink-muted">Reading…</p>}
      {disk && (
        <>
          <p className="body-sm ink-muted">{disk.location_label}</p>
          <div className="disk">
            <Donut disk={disk} />
            <dl className="disk-legend body-sm">
              <div>
                <dt>
                  <span className="swatch swatch-used" aria-hidden="true" />
                  Used
                </dt>
                <dd>{formatBytes(disk.used_bytes)}</dd>
              </div>
              <div>
                <dt>
                  <span className="swatch swatch-free" aria-hidden="true" />
                  Free
                </dt>
                <dd>{formatBytes(disk.free_bytes)}</dd>
              </div>
              <div>
                <dt>Total</dt>
                <dd>{formatBytes(disk.total_bytes)}</dd>
              </div>
            </dl>
          </div>
          {platform === "macos" && (
            <p className="caption ink-muted">
              macOS counts some space as purgeable and frees it when an app needs it, so Finder can
              show more space available than this.
            </p>
          )}
        </>
      )}
      <button type="button" className="button button-primary overview-action" onClick={onCleanUp}>
        Clean up
      </button>
    </section>
  );
}

/** Used and free as a ring. The circle's circumference is 100 units. */
function Donut({ disk }: { disk: DiskUsage }) {
  const used = clamp(share(disk.used_bytes, disk.total_bytes) ?? 0);
  const label =
    `Disk: ${formatBytes(disk.used_bytes)} used, ${formatBytes(disk.free_bytes)} free, ` +
    `of ${formatBytes(disk.total_bytes)}. ${Math.round(used)}% used.`;
  const r = 15.9155;
  return (
    <svg
      className={used >= HIGH_PERCENT ? "donut donut-high" : "donut"}
      viewBox="0 0 42 42"
      role="img"
      aria-label={label}
      focusable="false"
    >
      <circle className="donut-free" cx="21" cy="21" r={r} />
      <circle
        className="donut-used"
        cx="21"
        cy="21"
        r={r}
        strokeDasharray={`${used} ${100 - used}`}
        strokeDashoffset="25"
      />
      <text className="donut-value" x="21" y="21">
        {Math.round(used)}%
      </text>
      <text className="donut-caption" x="21" y="27">
        used
      </text>
    </svg>
  );
}

function share(part: number, total: number): number | null {
  return total > 0 ? (part / total) * 100 : null;
}

function clamp(percent: number): number {
  return Number.isFinite(percent) ? Math.min(100, Math.max(0, percent)) : 0;
}

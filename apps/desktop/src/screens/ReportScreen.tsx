import { useEffect, useRef, useState } from "react";
import {
  errorMessage,
  type ExportedReport,
  type FreeSpace,
  type Platform,
  type Report,
  type ReportItem,
} from "../engine";
import { formatBytes, plural } from "../format";
import { ExportIcon } from "../icons";

interface Props {
  report: Report;
  /** Null until the engine says which OS this is; then every cause is named. */
  platform: Platform | null;
  onExport: (planId: number) => Promise<ExportedReport>;
  onDone: () => void;
}

/** Why free space and file sizes differ, naming only what exists on this OS. */
function causes(platform: Platform | null): string {
  switch (platform) {
    case "macos":
      return (
        "Time Machine local snapshots can keep removed files, macOS counts some space as " +
        "purgeable and frees it later, the system can free space after a delay, and Docker " +
        "Desktop's disk file does not shrink when files inside it are removed"
      );
    case "linux":
      return (
        "Btrfs or ZFS snapshots can keep removed files, the system can free space after a " +
        "delay, and Docker Desktop's disk file does not shrink when files inside it are removed"
      );
    case "windows":
      return (
        "System restore points can keep removed files, the system can free space after a " +
        "delay, and Docker Desktop's disk file does not shrink when files inside it are removed"
      );
    default:
      return (
        "local snapshots (Time Machine, Btrfs or ZFS, Windows restore points) can keep removed " +
        "files, macOS frees purgeable space later, the system can free space after a delay, " +
        "and Docker Desktop's disk file does not shrink when files inside it are removed"
      );
  }
}

type ExportState =
  | { kind: "idle" }
  | { kind: "busy" }
  | { kind: "saved"; file: ExportedReport }
  | { kind: "failed"; message: string };

const SECTIONS: { outcome: ReportItem["outcome"]; title: string }[] = [
  { outcome: "removed", title: "Removed" },
  { outcome: "ran", title: "Tool commands run" },
  { outcome: "skipped", title: "Skipped" },
  { outcome: "failed", title: "Failed" },
  { outcome: "not_run", title: "Not run" },
];

/** A section longer than this starts folded, with its count and size in the summary. */
export const ITEMS_SHOWN = 10;

/** Failures always stay open: they are the part a person must read. */
function startsOpen(outcome: ReportItem["outcome"], count: number): boolean {
  return outcome === "failed" || count <= ITEMS_SHOWN;
}

/** Free space and file sizes disagree by more than rounding. */
function differs(volume: FreeSpace, removed: number): boolean {
  if (volume.before === null || volume.after === null) return false;
  const delta = volume.after - volume.before;
  return Math.abs(delta - removed) > Math.max(1_000_000, removed * 0.1);
}

export function ReportScreen({ report, platform, onExport, onDone }: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  const [exported, setExported] = useState<ExportState>({ kind: "idle" });
  const exportReport = async () => {
    setExported({ kind: "busy" });
    try {
      setExported({ kind: "saved", file: await onExport(report.plan_id) });
    } catch (e) {
      setExported({ kind: "failed", message: errorMessage(e) });
    }
  };
  const count = (outcome: ReportItem["outcome"]) =>
    report.items.filter((i) => i.outcome === outcome).length;

  return (
    <section className="screen" aria-labelledby="report-title">
      <h1 id="report-title" className="title-xl" tabIndex={-1} ref={heading}>
        Report
      </h1>
      <p className="body">
        {report.stopped ? "Stopped. " : ""}
        {plural(count("removed"), "item", "items")} removed, about{" "}
        <strong>{formatBytes(report.bytes_removed)}</strong>.{" "}
        {count("ran") > 0 && `${plural(count("ran"), "tool command", "tool commands")} ran. `}
        {count("skipped")} skipped, {count("failed")} failed.
      </p>
      {count("ran") > 0 && (
        <p className="body-sm ink-muted">
          Tools do not say how much they removed. The free space change below is the measurement.
        </p>
      )}

      {report.free_space.map((volume, i) => (
        <div key={i} className="card free-space">
          <p className="body-sm">
            Free space before: {volume.before === null ? "unknown" : formatBytes(volume.before)}.
            After: {volume.after === null ? "unknown" : formatBytes(volume.after)}, measured{" "}
            {plural(volume.after_delay_secs, "second", "seconds")} after the last item (
            {volume.source}).
          </p>
          {differs(volume, report.bytes_removed) &&
            volume.before !== null &&
            volume.after !== null && (
              <p className="body-sm ink-muted">
                Free space changed by {formatBytes(Math.abs(volume.after - volume.before))}. The
                files removed were about {formatBytes(report.bytes_removed)}. These numbers differ
                because {causes(platform)}. Files that share data with other files, and other apps
                writing at the same time, change the number too.
              </p>
            )}
        </div>
      ))}

      {SECTIONS.map(({ outcome, title }) => {
        const items = report.items.filter((i) => i.outcome === outcome);
        if (items.length === 0) return null;
        const bytes = items.reduce((sum, i) => sum + i.bytes_removed, 0);
        return (
          <section key={outcome} className="card group" aria-label={title}>
            <details open={startsOpen(outcome, items.length)}>
              <summary className="title">
                {title} ({plural(items.length, "item", "items")}
                {bytes > 0 && `, ${formatBytes(bytes)}`})
              </summary>
              <ul className="plain-list body-sm">
                {items.map((item) => (
                  <li key={item.candidate_id}>
                    <span className="file-name">{item.display_name}</span>
                    {item.bytes_removed > 0 && <> — {formatBytes(item.bytes_removed)}</>}
                    {item.detail && <>: {item.detail}</>}
                  </li>
                ))}
              </ul>
              {(outcome === "removed" || outcome === "ran") &&
                [...new Set(items.map((i) => i.recovery_text))].map(
                  (text) =>
                    text && (
                      <p key={text} className="body-sm ink-muted">
                        {text}
                      </p>
                    ),
                )}
            </details>
          </section>
        );
      })}

      {report.audit_log === "memory_only" && (
        <details className="card">
          <summary className="heading">Audit log (not saved to disk)</summary>
          <pre className="code log" tabIndex={0} aria-label="Audit log lines">
            {report.memory_log.join("\n")}
          </pre>
        </details>
      )}

      {exported.kind === "failed" && (
        <p className="notice notice-warning body-sm" role="alert">
          {exported.message}
        </p>
      )}

      {/* Stays at the bottom of the window, so a long report never hides it. */}
      <div className="actions actions-sticky">
        {/* Always present, so screen readers announce the text when it appears. */}
        <p className="body-sm file-name action-status" role="status">
          {exported.kind === "saved" && `Saved to ${exported.file.path}`}
        </p>
        <button
          type="button"
          className="button"
          onClick={exportReport}
          disabled={exported.kind === "busy"}
        >
          <ExportIcon />
          Export report
        </button>
        <button type="button" className="button button-primary" onClick={onDone}>
          Done
        </button>
      </div>
    </section>
  );
}

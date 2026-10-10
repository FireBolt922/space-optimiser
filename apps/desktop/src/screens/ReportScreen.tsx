import { useEffect, useRef } from "react";
import type { FreeSpace, Report, ReportItem } from "../engine";
import { formatBytes, plural } from "../format";

interface Props {
  report: Report;
  onDone: () => void;
}

const SECTIONS: { outcome: ReportItem["outcome"]; title: string }[] = [
  { outcome: "removed", title: "Removed" },
  { outcome: "skipped", title: "Skipped" },
  { outcome: "failed", title: "Failed" },
  { outcome: "not_run", title: "Not run" },
];

/** Free space and file sizes disagree by more than rounding. */
function differs(volume: FreeSpace, removed: number): boolean {
  if (volume.before === null || volume.after === null) return false;
  const delta = volume.after - volume.before;
  return Math.abs(delta - removed) > Math.max(1_000_000, removed * 0.1);
}

export function ReportScreen({ report, onDone }: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
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
        <strong>{formatBytes(report.bytes_removed)}</strong>. {count("skipped")} skipped,{" "}
        {count("failed")} failed.
      </p>

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
                when the system shares file data or delays freeing space, or when other apps
                write files at the same time.
              </p>
            )}
        </div>
      ))}

      {SECTIONS.map(({ outcome, title }) => {
        const items = report.items.filter((i) => i.outcome === outcome);
        if (items.length === 0) return null;
        return (
          <section key={outcome} className="card group" aria-label={title}>
            <h2 className="title">{title}</h2>
            <ul className="plain-list body-sm">
              {items.map((item) => (
                <li key={item.candidate_id}>
                  <span className="file-name">{item.display_name}</span>
                  {item.bytes_removed > 0 && <> — {formatBytes(item.bytes_removed)}</>}
                  {item.detail && <>: {item.detail}</>}
                </li>
              ))}
            </ul>
            {outcome === "removed" && items[0].recovery_text && (
              <p className="body-sm ink-muted">{items[0].recovery_text}</p>
            )}
          </section>
        );
      })}

      {report.audit_log === "memory_only" && (
        <details className="card">
          <summary className="heading">Audit log (not saved to disk)</summary>
          <pre className="code log">{report.memory_log.join("\n")}</pre>
        </details>
      )}

      <div className="actions">
        <button type="button" className="button button-primary" onClick={onDone}>
          Done
        </button>
      </div>
    </section>
  );
}

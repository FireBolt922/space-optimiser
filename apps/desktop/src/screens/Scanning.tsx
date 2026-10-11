import { useEffect, useState } from "react";
import type { ScanStage, ScanView } from "../engine";
import { formatBytes, formatDuration, plural } from "../format";

interface Props {
  view?: ScanView;
  /** When the scan started (`Date.now()`), so the clock survives a view switch. */
  startedAt: number;
  cancelling: boolean;
  onCancel: () => void;
}

/** Stage names, in catalog words rather than paths (phase 7 screen 2). */
const STAGE_NAMES: Record<ScanStage, string> = {
  temp: "Temporary files",
  caches: "App and browser caches",
  logs: "Old logs",
  tools: "Developer tool caches",
  docker: "Docker",
  large_files: "Large files",
};

/** What the scan is doing now, as one sentence. */
function doing(stage: ScanStage, detail: string | null): string {
  switch (stage) {
    case "temp":
      return "Checking temporary files.";
    case "caches":
      return "Checking app and browser caches.";
    case "logs":
      return "Checking old logs.";
    case "tools":
      return detail ? `Asking ${detail} where its cache is.` : "Checking developer tool caches.";
    case "docker":
      return "Asking Docker how much space it uses.";
    case "large_files":
      return "Looking for large files in your home folder.";
  }
}

/** Ticks once a second while mounted. */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

export function Scanning({ view, startedAt, cancelling, onCancel }: Props) {
  const now = useNow();
  const progress = view?.progress;
  const stages = view?.stages ?? [];
  const current = progress ? stages.indexOf(progress.stage) : -1;
  return (
    <section className="screen" aria-labelledby="scanning-title">
      <h1 id="scanning-title" className="title-xl" tabIndex={-1}>
        Scanning
      </h1>
      {/* Only the stage sentence is announced; counts and the clock change too often. */}
      <p className="body" role="status">
        {progress ? doing(progress.stage, progress.detail) : "Starting the scan."}
      </p>
      <p className="body-sm ink-muted">
        {progress && progress.files_seen > 0
          ? `${plural(progress.files_seen, "file", "files")} so far, ${formatBytes(progress.bytes_seen)}.`
          : null}
      </p>
      <p className="body-sm ink-muted">Running for {formatDuration(now - startedAt)}</p>
      {stages.length > 0 && (
        <ol className="stages plain-list" aria-label="Scan stages">
          {stages.map((stage, i) => {
            const state = i < current ? "Done" : i === current ? "Checking" : "Waiting";
            return (
              <li key={stage} className={`stage stage-${state.toLowerCase()}`}>
                <span className="body-sm">{STAGE_NAMES[stage]}</span>
                <span className="badge">{state}</span>
              </li>
            );
          })}
        </ol>
      )}
      <div className="actions">
        <button type="button" className="button" onClick={onCancel} disabled={cancelling}>
          {cancelling ? "Cancelling" : "Cancel"}
        </button>
      </div>
    </section>
  );
}

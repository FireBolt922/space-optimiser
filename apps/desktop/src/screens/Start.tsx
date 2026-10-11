import type { InterruptedRun, ScanMode } from "../engine";
import { plural } from "../format";

interface Props {
  notice?: string;
  interrupted: InterruptedRun | null;
  onDismissInterrupted: () => void;
  onScan: (mode: ScanMode) => void;
}

export function Start({ notice, interrupted, onDismissInterrupted, onScan }: Props) {
  return (
    <section className="screen" aria-labelledby="start-title">
      <h1 id="start-title" className="title-xl" tabIndex={-1}>
        Free up space safely
      </h1>
      <p className="body ink-muted">
        Space Optimiser only removes files in locations it has a reviewed rule for, and only
        after you confirm.
      </p>

      {interrupted && (
        <div className="notice notice-warning" role="status">
          <h2 className="heading">The last cleanup was interrupted</h2>
          <p className="body-sm">
            It started at {new Date(interrupted.started_unix_secs * 1000).toLocaleString()} with{" "}
            {plural(interrupted.items_planned, "item", "items")}. The log recorded{" "}
            {plural(interrupted.actions.length, "action", "actions")} before it stopped.
          </p>
          {interrupted.actions.length > 0 && (
            <ul className="plain-list body-sm">
              {interrupted.actions.map((action, i) => (
                <li key={i}>
                  <span className="file-name">{action.display_name}</span>: {action.outcome}
                </li>
              ))}
            </ul>
          )}
          <p className="body-sm">
            <a
              href="#"
              className="link"
              onClick={(e) => {
                e.preventDefault();
                onDismissInterrupted();
              }}
            >
              Hide this notice
            </a>
          </p>
        </div>
      )}

      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <div className="mode-list">
        <div className="card mode">
          <h2 className="title">Deep Scan</h2>
          <p className="body-sm ink-muted">
            For developers. Everything in Normal Scan, plus package caches, old logs, Docker&apos;s
            build cache, and a list of large files.
          </p>
          <button type="button" className="button button-primary" onClick={() => onScan("deep")}>
            Deep Scan
          </button>
        </div>
        <div className="card mode">
          <h2 className="title">Normal Scan</h2>
          <p className="body-sm ink-muted">
            For everyone. Checks temporary files, thumbnails, font caches, and browser caches.
          </p>
          <button type="button" className="button" onClick={() => onScan("normal")}>
            Normal Scan
          </button>
        </div>
      </div>
    </section>
  );
}

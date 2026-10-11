import type { ReportItem, RunView } from "../engine";
import { formatDuration } from "../format";

interface Props {
  done: number;
  total: number;
  /** The latest poll, once one has arrived. */
  run?: RunView;
  stopping: boolean;
  onStop: () => void;
}

const OUTCOME_WORDS: Record<ReportItem["outcome"], string> = {
  removed: "Removed",
  ran: "Done",
  skipped: "Skipped",
  failed: "Failed",
  not_run: "Not run",
};

export function Running({ done, total, run, stopping, onStop }: Props) {
  const current = run?.current ?? null;
  const counts = run?.counts;
  return (
    <section className="screen" aria-labelledby="running-title">
      <h1 id="running-title" className="title-xl" tabIndex={-1}>
        Cleaning
      </h1>
      {/* Announced once per item; the clock and the output are not. */}
      <p className="body" role="status">
        {current ? `Cleaning ${current.display_name}.` : `${done} of ${total} items done`}
      </p>
      <p className="body-sm ink-muted">
        {done} of {total} items done
        {run && ` · ${formatDuration(run.elapsed_ms)}`}
      </p>
      <progress className="progress" max={total} value={done} aria-label="Cleaning progress" />

      {current && (
        <section className="card run-current" aria-label="Current item">
          <div className="group-head">
            <h2 className="heading file-name">{current.display_name}</h2>
            <span className="body-sm ink-muted">
              Running for {formatDuration(current.elapsed_ms)}
            </span>
          </div>
          {current.output.length > 0 ? (
            <pre
              className="code run-output"
              tabIndex={0}
              aria-label={`Output from ${current.display_name}`}
            >
              {current.output.join("\n")}
            </pre>
          ) : (
            <p className="body-sm ink-muted">Removing files. Large folders can take a while.</p>
          )}
        </section>
      )}

      {run && run.recent.length > 0 && (
        <section className="run-finished" aria-labelledby="finished-title">
          <h2 id="finished-title" className="heading">
            Finished
          </h2>
          <ul className="plain-list body-sm" aria-label="Finished items">
            {run.recent.map((item, i) => (
              <li key={`${item.candidate_id}-${i}`} className="run-item">
                <span className="file-name">{item.display_name}</span>
                <span className={item.outcome === "failed" ? "row-error" : "ink-muted"}>
                  {OUTCOME_WORDS[item.outcome]}
                  {item.detail && `: ${item.detail}`}
                </span>
                {item.hint && <span className="notice notice-warning">{item.hint}</span>}
              </li>
            ))}
          </ul>
        </section>
      )}
      {counts && (
        <p className="body-sm ink-muted">
          {counts.removed} removed, {counts.skipped} skipped, {counts.failed} failed
        </p>
      )}

      <p className="body-sm ink-muted">
        {stopping
          ? "Stopping after the current item."
          : "Stop finishes the current item, then halts."}
      </p>
      <div className="actions">
        <button type="button" className="button" onClick={onStop} disabled={stopping}>
          Stop
        </button>
      </div>
    </section>
  );
}

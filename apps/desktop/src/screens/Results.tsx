import { useEffect, useRef } from "react";
import type { Candidate, ScanView } from "../engine";
import { formatBytes, plural, riskWord } from "../format";

interface Props {
  scan: ScanView;
  selected: number[];
  onSelect: (selected: number[]) => void;
  onReview: () => void;
  onScanAgain: () => void;
}

/** Only a delete can be selected; info and uncertain rows cannot. */
function selectable(c: Candidate): boolean {
  return c.action === "delete";
}

function actionWord(c: Candidate): string {
  switch (c.action) {
    case "delete":
      return "Clean";
    case "run_tool":
      return "Clean with the tool";
    case "report":
      return "Show only";
    case "none":
      return "Not sure — not offered";
  }
}

export function Results({ scan, selected, onSelect, onReview, onScanAgain }: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);

  const groups = new Map<string, Candidate[]>();
  for (const c of scan.candidates) {
    groups.set(c.location_label, [...(groups.get(c.location_label) ?? []), c]);
  }
  const chosen = new Set(selected);
  const toggle = (id: number) =>
    onSelect(chosen.has(id) ? selected.filter((s) => s !== id) : [...selected, id]);
  const selectSafe = () => {
    const safe = scan.candidates.filter((c) => c.bulk_safe && selectable(c)).map((c) => c.id);
    onSelect([...new Set([...selected, ...safe])]);
  };
  const leftOut = scan.candidates.filter(
    (c) => selectable(c) && !c.bulk_safe && !chosen.has(c.id),
  ).length;
  const selectedBytes = scan.candidates
    .filter((c) => chosen.has(c.id))
    .reduce((sum, c) => sum + c.estimated_bytes, 0);
  const anySafe = scan.candidates.some((c) => c.bulk_safe && selectable(c));
  const memoryRoot = scan.roots.some((r) => r.frees_memory);

  return (
    <section className="screen" aria-labelledby="results-title">
      <h1 id="results-title" className="title-xl" tabIndex={-1} ref={heading}>
        Results
      </h1>

      {scan.candidates.length === 0 ? (
        <p className="body">Nothing to clean in this mode</p>
      ) : (
        <>
          <div className="toolbar">
            <button type="button" className="button" onClick={selectSafe} disabled={!anySafe}>
              Select safe items
            </button>
            <span className="body-sm ink-muted" aria-live="polite">
              {plural(chosen.size, "item", "items")} selected, about {formatBytes(selectedBytes)}
            </span>
          </div>
          {leftOut > 0 && (
            <p className="body-sm ink-muted">
              {plural(leftOut, "other item was", "other items were")} left unselected. They need
              a separate choice because they cause a re-download or remove a cache a browser is
              using.
            </p>
          )}
          {memoryRoot && (
            <p className="body-sm ink-muted">
              Your temporary folder is in memory: cleaning it frees memory, not disk.
            </p>
          )}
          {[...groups].map(([label, rows]) => (
            <section key={label} className="card group" aria-label={label}>
              <h2 className="title">{label}</h2>
              <p className="body-sm ink-muted">{rows[0].explanation}</p>
              <ul className="rows">
                {rows.map((c) => (
                  <li key={c.id} className="row">
                    {c.action === "report" ? (
                      <span className="row-name file-name">{c.display_name}</span>
                    ) : (
                      <label className="row-name">
                        <input
                          type="checkbox"
                          checked={chosen.has(c.id)}
                          disabled={!selectable(c)}
                          onChange={() => toggle(c.id)}
                        />
                        <span className="file-name">{c.display_name}</span>
                      </label>
                    )}
                    <span className="badge">{riskWord(c.risk)}</span>
                    <span className="body-sm ink-muted row-action">{actionWord(c)}</span>
                    <span className="body-sm row-size">
                      {formatBytes(c.estimated_bytes)}
                      {c.labels.includes("shared") && " (shared, frees nothing)"}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </>
      )}

      {(scan.skipped.length > 0 || scan.roots.some((r) => r.skipped)) && (
        <details className="card skipped">
          <summary className="heading">
            Left alone ({plural(scan.skipped.length, "item", "items")})
          </summary>
          <ul className="plain-list body-sm">
            {scan.roots
              .filter((r) => r.skipped)
              .map((r, i) => (
                <li key={`root-${i}`}>
                  {r.location_label}: {r.skipped}
                </li>
              ))}
            {scan.skipped.slice(0, 200).map((s, i) => (
              <li key={i}>
                <span className="file-name">{s.display_name}</span>: {s.detail}
              </li>
            ))}
          </ul>
        </details>
      )}

      <div className="actions">
        <button type="button" className="button" onClick={onScanAgain}>
          Scan again
        </button>
        <button
          type="button"
          className="button button-primary"
          onClick={onReview}
          disabled={chosen.size === 0}
        >
          Review
        </button>
      </div>
    </section>
  );
}

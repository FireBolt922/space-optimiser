import { useEffect, useRef, useState } from "react";
import {
  errorMessage,
  type Candidate,
  type Note,
  type ScanView,
  type SkippedView,
} from "../engine";
import { CATEGORIES, categoryBytes, categoryOf, type CategoryId } from "../categories";
import { formatAge, formatBytes, formatDuration, plural, riskWord } from "../format";
import { SKIP_REASONS } from "../skipReasons";
import { TEMP_SOURCES, tempSource } from "../tempSources";
import { FolderIcon } from "../icons";

interface Props {
  scan: ScanView;
  selected: number[];
  onSelect: (selected: number[]) => void;
  onReview: () => void;
  /** When the results arrived (`Date.now()`), if known. */
  finishedAt: number | null;
  /** Reruns the scan in the same mode. */
  onScanAgain: () => void;
  /** Opens the file manager at a row. Sends the id only. */
  onReveal: (candidateId: number) => Promise<void>;
}

/** Rows shown per group before "Show all" (phase 7: a group, not a dump). */
export const ROWS_SHOWN = 10;

/** Deletes and tool commands can be selected; info and uncertain rows cannot. */
function selectable(c: Candidate): boolean {
  return c.action === "delete" || c.action === "run_tool";
}

/** A tool row's number is the most it could free, not what a delete removes. */
function sizeText(c: Candidate): string {
  if (c.upper_bound_bytes !== null) return `Up to ${formatBytes(c.upper_bound_bytes)}`;
  return formatBytes(c.estimated_bytes);
}

/** One accessible name with the row's name, risk, and size (phase 11). */
function checkboxName(c: Candidate): string {
  return `${c.display_name}, ${riskWord(c.risk)} risk, ${sizeText(c)}`;
}

/** "Folder, 12 files · 3 days old" for temp and log rows; null for others. */
function entryDetail(c: Candidate): string | null {
  if (c.kind === null) return null;
  const what = c.kind === "folder" ? `Folder, ${plural(c.file_count, "file", "files")}` : "File";
  return c.age_days === null ? what : `${what} · ${formatAge(c.age_days)}`;
}

/** A delete that frees nothing measurable: empty, or space shared elsewhere. */
function isEmpty(c: Candidate): boolean {
  return c.action === "delete" && c.estimated_bytes === 0 && c.upper_bound_bytes === null;
}

function actionWord(c: Candidate): string {
  if (c.labels.includes("quit_first")) return "Quit the browser first";
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

interface Group {
  label: string;
  category: CategoryId;
  rows: Candidate[];
  notes: Note[];
}

/** One group per catalog row, in scan order, each tagged with its category. */
function groupRows(scan: ScanView): Group[] {
  const groups = new Map<string, Group>();
  const group = (label: string, catalogId: string) => {
    const found = groups.get(label) ?? {
      label,
      category: categoryOf(catalogId),
      rows: [],
      notes: [],
    };
    groups.set(label, found);
    return found;
  };
  for (const c of scan.candidates) group(c.location_label, c.catalog_id).rows.push(c);
  for (const n of scan.notes) group(n.location_label, n.catalog_id).notes.push(n);
  return [...groups.values()];
}

export function Results({
  scan,
  selected,
  onSelect,
  onReview,
  finishedAt,
  onScanAgain,
  onReveal,
}: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  const [revealErrors, setRevealErrors] = useState<Record<number, string>>({});
  const [category, setCategory] = useState<CategoryId | "all">("all");
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const reveal = async (id: number) => {
    setRevealErrors((errors) => {
      const next = { ...errors };
      delete next[id];
      return next;
    });
    try {
      await onReveal(id);
    } catch (e) {
      setRevealErrors((errors) => ({ ...errors, [id]: errorMessage(e) }));
    }
  };

  const groups = groupRows(scan);
  const chosen = new Set(selected);
  const toggle = (id: number) =>
    onSelect(chosen.has(id) ? selected.filter((s) => s !== id) : [...selected, id]);
  // Selection works on the whole scan, whatever is filtered out of view.
  const selectSafe = () => {
    const safe = scan.candidates.filter((c) => c.bulk_safe && selectable(c)).map((c) => c.id);
    onSelect([...new Set([...selected, ...safe])]);
  };
  const leftOut = scan.candidates.filter(
    (c) => selectable(c) && !c.bulk_safe && !chosen.has(c.id),
  ).length;
  const chosenRows = scan.candidates.filter((c) => chosen.has(c.id));
  // A row inside another selected folder is removed with it: count it once.
  const covered = new Set(chosenRows.flatMap((c) => c.covers));
  const selectedBytes = chosenRows
    .filter((c) => !covered.has(c.id))
    .reduce((sum, c) => sum + c.estimated_bytes, 0);
  const toolBytes = chosenRows
    .filter((c) => c.action === "run_tool")
    .reduce((sum, c) => sum + (c.upper_bound_bytes ?? 0), 0);
  const anySafe = scan.candidates.some((c) => c.bulk_safe && selectable(c));
  const memoryRoot = scan.roots.some((r) => r.frees_memory);
  const quitFirst = scan.candidates.some((c) => c.labels.includes("quit_first"));

  // Category tabs: only the categories this scan found, with count and size.
  const categories = CATEGORIES.map((cat) => {
    const rows = groups.filter((g) => g.category === cat.id).flatMap((g) => g.rows);
    return { ...cat, count: rows.length, bytes: categoryBytes(rows) };
  }).filter((cat) => cat.count > 0 || groups.some((g) => g.category === cat.id));
  const needle = query.trim().toLowerCase();
  const matches = (c: Candidate) => needle === "" || c.display_name.toLowerCase().includes(needle);
  const shown = groups
    .filter((g) => category === "all" || g.category === category)
    .map((g) => ({ ...g, rows: g.rows.filter(matches) }))
    .filter((g) => g.rows.length > 0 || (needle === "" && g.notes.length > 0));
  const toggleExpanded = (label: string) =>
    setExpanded((set) => {
      const next = new Set(set);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return next;
    });

  const renderRow = (c: Candidate) => {
    const detail = entryDetail(c);
    return (
      <li key={c.id} className="row">
        {c.action === "report" ? (
          <span className="row-name file-name">{c.display_name}</span>
        ) : (
          <label className="row-name">
            <input
              type="checkbox"
              aria-label={checkboxName(c)}
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
          {sizeText(c)}
          {c.labels.includes("shared") && " (shared, frees nothing)"}
        </span>
        {c.revealable ? (
          <button
            type="button"
            className="button button-sm"
            aria-label={`Show in folder: ${c.display_name}`}
            onClick={() => reveal(c.id)}
          >
            <FolderIcon />
            Show in folder
          </button>
        ) : (
          <span />
        )}
        {detail && <span className="body-sm ink-muted row-detail">{detail}</span>}
        {c.found_by && <span className="body-sm ink-muted row-detail">{c.found_by}</span>}
        {revealErrors[c.id] && (
          <p className="body-sm row-detail row-error" role="alert">
            {revealErrors[c.id]}
          </p>
        )}
      </li>
    );
  };

  /** Up to ROWS_SHOWN rows, then "Show all" for the rest. */
  const folded = (key: string, label: string, rows: Candidate[]) => {
    const open = expanded.has(key) || rows.length <= ROWS_SHOWN;
    const visible = open ? rows : rows.slice(0, ROWS_SHOWN);
    return (
      <>
        <ul className="rows">{visible.map(renderRow)}</ul>
        {rows.length > ROWS_SHOWN && (
          <button
            type="button"
            className="button button-sm group-more"
            aria-expanded={open}
            onClick={() => toggleExpanded(key)}
          >
            {open ? `Show fewer in ${label}` : `Show all ${rows.length} items in ${label}`}
          </button>
        )}
      </>
    );
  };

  /** Temporary files: sub-groups by source, empty items folded into one line. */
  const tempRows = (label: string, rows: Candidate[]) => {
    const empty = rows.filter(isEmpty);
    const sized = rows.filter((c) => !isEmpty(c));
    const sources = TEMP_SOURCES.map((source) => ({
      ...source,
      rows: sized.filter((c) => tempSource(c.display_name) === source.id),
    })).filter((source) => source.rows.length > 0);
    const emptyKey = `${label}/empty`;
    const emptyOpen = expanded.has(emptyKey) || needle !== "";
    return (
      <>
        {sources.length <= 1
          ? folded(label, label, sized)
          : sources.map((source) => (
              <div key={source.id} className="subgroup">
                <div className="group-head">
                  <h3 className="heading">{source.title}</h3>
                  <span className="body-sm ink-muted">
                    {plural(source.rows.length, "item", "items")},{" "}
                    {formatBytes(categoryBytes(source.rows))}
                  </span>
                </div>
                <p className="body-sm ink-muted">{source.description}</p>
                {folded(`${label}/${source.id}`, source.title, source.rows)}
              </div>
            ))}
        {empty.length > 0 && (
          <div className="subgroup">
            <button
              type="button"
              className="button button-sm group-more"
              aria-expanded={emptyOpen}
              onClick={() => toggleExpanded(emptyKey)}
            >
              {emptyOpen
                ? `Hide ${plural(empty.length, "empty item", "empty items")}`
                : `Show ${plural(empty.length, "empty item", "empty items")} (0 bytes)`}
            </button>
            <p className="body-sm ink-muted">
              Empty, or their space is shared with another file, so cleaning them frees almost
              nothing. They are still cleaned when selected.
            </p>
            {emptyOpen && <ul className="rows">{empty.map(renderRow)}</ul>}
          </div>
        )}
      </>
    );
  };

  // Left alone: grouped by reason, largest group first.
  const byReason = new Map<string, SkippedView[]>();
  for (const s of scan.skipped) byReason.set(s.reason, [...(byReason.get(s.reason) ?? []), s]);
  const reasons = [...byReason.entries()]
    .map(([reason, items]) => ({
      reason,
      items,
      title: SKIP_REASONS[reason]?.title ?? items[0].detail,
      explanation: SKIP_REASONS[reason]?.explanation ?? null,
    }))
    .sort((a, b) => b.items.length - a.items.length);

  return (
    <section className="screen" aria-labelledby="results-title">
      <div className="screen-head">
        <h1 id="results-title" className="title-xl" tabIndex={-1} ref={heading}>
          Results
        </h1>
        <button type="button" className="button" onClick={onScanAgain}>
          Scan again
        </button>
      </div>
      <p className="body-sm ink-muted">
        {[
          finishedAt !== null &&
            `Scanned at ${new Date(finishedAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`,
          `took ${formatDuration(scan.duration_ms)}`,
          `checked ${plural(scan.progress.files_seen, "file", "files")}`,
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>

      {scan.candidates.length === 0 && scan.notes.length === 0 ? (
        <p className="body">Nothing to clean in this mode</p>
      ) : (
        <>
          <div className="toolbar">
            <button type="button" className="button" onClick={selectSafe} disabled={!anySafe}>
              Select safe items
            </button>
            <label className="search">
              <span className="body-sm">Filter by name</span>
              <input
                type="search"
                className="input"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
          </div>
          {categories.length > 1 && (
            <div className="chips" role="group" aria-label="Categories">
              <button
                type="button"
                className="chip"
                aria-pressed={category === "all"}
                onClick={() => setCategory("all")}
              >
                All ({scan.candidates.length})
              </button>
              {categories.map((cat) => (
                <button
                  key={cat.id}
                  type="button"
                  className="chip"
                  aria-pressed={category === cat.id}
                  onClick={() => setCategory(cat.id)}
                >
                  {cat.title} ({cat.count}
                  {cat.bytes > 0 && `, ${formatBytes(cat.bytes)}`})
                </button>
              ))}
            </div>
          )}
          {quitFirst && (
            <p className="body-sm ink-muted">
              A browser is open, so its cache is not offered. Quit the browser, then scan again.
            </p>
          )}
          {leftOut > 0 && (
            <p className="body-sm ink-muted">
              {plural(leftOut, "other item was", "other items were")} left unselected. They need a
              separate choice because they cause a re-download or remove a cache a browser is using.
            </p>
          )}
          {memoryRoot && (
            <p className="body-sm ink-muted">
              Your temporary folder is in memory: cleaning it frees memory, not disk.
            </p>
          )}
          {shown.length === 0 && <p className="body">No items match this filter.</p>}
          {shown.map(({ label, rows, notes }) => (
            <section key={label} className="card group" aria-label={label}>
              <div className="group-head">
                <h2 className="title">{label}</h2>
                {rows.length > 0 && (
                  <span className="body-sm ink-muted">
                    {plural(rows.length, "item", "items")}, {formatBytes(categoryBytes(rows))}
                  </span>
                )}
              </div>
              {rows.length > 0 && <p className="body-sm ink-muted">{rows[0].explanation}</p>}
              {notes.map((n, i) => (
                <p key={i} className="body-sm ink-muted">
                  {n.text}
                  {n.bytes !== null && ` (${formatBytes(n.bytes)})`}
                </p>
              ))}
              {rows.length > 0 && rows[0].catalog_id === "user-temp"
                ? tempRows(label, rows)
                : folded(label, label, rows)}
            </section>
          ))}
        </>
      )}

      {(scan.skipped.length > 0 || scan.roots.some((r) => r.skipped)) && (
        <details className="card skipped">
          <summary className="heading">
            Left alone ({plural(scan.skipped.length, "item", "items")})
          </summary>
          <p className="body-sm ink-muted">
            Space Optimiser checked these and did not offer them. Nothing here is removed.
          </p>
          {scan.roots.some((r) => r.skipped) && (
            <ul className="plain-list body-sm">
              {scan.roots
                .filter((r) => r.skipped)
                .map((r, i) => (
                  <li key={`root-${i}`}>
                    {r.location_label}: {r.skipped}
                  </li>
                ))}
            </ul>
          )}
          {reasons.map(({ reason, items, title, explanation }) => {
            const key = `skipped/${reason}`;
            const open = expanded.has(key) || items.length <= ROWS_SHOWN;
            return (
              <div key={reason} className="subgroup">
                <h3 className="heading">
                  {title} ({plural(items.length, "item", "items")})
                </h3>
                {explanation && <p className="body-sm ink-muted">{explanation}</p>}
                <ul className="plain-list body-sm">
                  {(open ? items : items.slice(0, ROWS_SHOWN)).map((s, i) => (
                    <li key={i} className="file-name">
                      {s.display_name}
                    </li>
                  ))}
                </ul>
                {items.length > ROWS_SHOWN && (
                  <button
                    type="button"
                    className="button button-sm group-more"
                    aria-expanded={open}
                    onClick={() => toggleExpanded(key)}
                  >
                    {open ? `Show fewer in ${title}` : `Show all ${items.length} items in ${title}`}
                  </button>
                )}
              </div>
            );
          })}
        </details>
      )}

      {/* Stays at the bottom of the window, so a long list never hides it. */}
      <div className="actions actions-sticky">
        <p className="body-sm action-status" aria-live="polite">
          {scan.candidates.length > 0 && (
            <>
              {plural(chosen.size, "item", "items")} selected, about {formatBytes(selectedBytes)}
              {toolBytes > 0 && `. Tools may free up to ${formatBytes(toolBytes)} more.`}
            </>
          )}
        </p>
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

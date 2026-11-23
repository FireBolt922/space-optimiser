import type { InterruptedRun, ScanMode } from "../engine";
import { plural } from "../format";

interface Props {
  notice?: string;
  interrupted: InterruptedRun | null;
  onDismissInterrupted: () => void;
  onScan: (mode: ScanMode) => void;
}

interface Choice {
  mode: ScanMode;
  title: string;
  /** The button's words. One primary button per view: Deep Scan. */
  button: string;
  primary?: boolean;
  description: string;
}

/** The scans clean caches across the computer. */
const SCANS: Choice[] = [
  {
    mode: "deep",
    title: "Deep Scan",
    button: "Deep Scan",
    primary: true,
    description:
      "For developers. Everything in Normal Scan, plus package caches, editor caches, old logs, Docker's build cache, and a list of large files.",
  },
  {
    mode: "normal",
    title: "Normal Scan",
    button: "Normal Scan",
    description:
      "For everyone. Checks temporary files, thumbnails, font caches, and browser caches, and shows the AI model Chrome keeps on this computer.",
  },
];

/** Each tool looks at one area and nothing else. */
const TOOLS: Choice[] = [
  {
    mode: "projects",
    title: "Projects",
    button: "Scan projects",
    description:
      "Finds node_modules, Rust target, Python environment, and .next folders in your projects, and shows which projects you have not touched for 30 days.",
  },
  {
    mode: "android",
    title: "Android",
    button: "Scan Android",
    description:
      "Finds emulator Quick Boot snapshots and system images that no emulator uses. Apps and data inside an emulator are never removed.",
  },
];

export function Start({ notice, interrupted, onDismissInterrupted, onScan }: Props) {
  const choices = (list: Choice[]) => (
    <div className="mode-list">
      {list.map((choice) => (
        <div key={choice.mode} className="card mode">
          <h3 className="title">{choice.title}</h3>
          <p className="body-sm ink-muted">{choice.description}</p>
          <button
            type="button"
            className={choice.primary ? "button button-primary" : "button"}
            onClick={() => onScan(choice.mode)}
          >
            {choice.button}
          </button>
        </div>
      ))}
    </div>
  );

  return (
    <section className="screen" aria-labelledby="start-title">
      <h1 id="start-title" className="title-xl" tabIndex={-1}>
        Free up space safely
      </h1>
      <p className="body ink-muted">
        Space Optimiser only removes files in locations it has a reviewed rule for, and only after
        you confirm.
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

      <section className="tool-group" aria-labelledby="start-scans">
        <h2 id="start-scans" className="heading">
          Scans
        </h2>
        {choices(SCANS)}
      </section>
      <section className="tool-group" aria-labelledby="start-tools">
        <h2 id="start-tools" className="heading">
          Tools
        </h2>
        <p className="body-sm ink-muted">
          Each tool looks at one area. Nothing is removed until you review the list and confirm.
        </p>
        {choices(TOOLS)}
      </section>
    </section>
  );
}

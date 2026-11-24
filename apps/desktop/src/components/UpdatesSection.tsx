// "Check for updates" in Settings (phase 15). The shell keeps the updater's
// state, so leaving Settings loses nothing. Nothing is checked until the
// person asks, and nothing is downloaded until they press Download and
// install. Release notes are rendered as text.
import { useCallback, useEffect, useState } from "react";
import { errorMessage, type Engine, type UpdateView } from "../engine";
import { formatBytes } from "../format";

/** How often a download's progress is read. */
const PROGRESS_POLL_MS = 500;

type UpdateEngine = Pick<
  Engine,
  "checkForUpdate" | "updateStatus" | "installUpdate" | "restartApp" | "openDownloadPage"
>;

interface Props {
  engine: UpdateEngine;
  version: string | null;
  autoCheck: boolean;
  onAutoCheck: (on: boolean) => void;
}

const LINK_REASONS: Record<string, string> = {
  linux_package:
    "Install it from the download page. Updating a .deb package needs an administrator password, which Space Optimiser never asks for.",
  app_folder_locked:
    "Your account cannot change the folder Space Optimiser is in, so it cannot update itself without an administrator password. Download the new version instead.",
  not_installed:
    "This copy is not an installed app, so it cannot update itself. Download the new version instead.",
};

export function UpdatesSection({ engine, version, autoCheck, onAutoCheck }: Props) {
  const [view, setView] = useState<UpdateView>({ state: "idle" });
  const [error, setError] = useState<string | null>(null);

  // Show what the updater is doing now, such as a download started earlier.
  useEffect(() => {
    engine.updateStatus().then(setView, () => undefined);
  }, [engine]);

  // Follow a download until it is installed, waiting, or failed.
  const active = view.state === "downloading" || view.state === "waiting";
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    const timer = setInterval(() => {
      engine.updateStatus().then(
        (next) => {
          if (!stopped) setView(next);
        },
        () => undefined,
      );
    }, PROGRESS_POLL_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [engine, active]);

  const run = useCallback(async (action: () => Promise<UpdateView | void>) => {
    setError(null);
    try {
      const next = await action();
      if (next) setView(next);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);

  const busy = view.state === "checking" || active;

  return (
    <section className="card stack" aria-labelledby="updates-title">
      <h2 id="updates-title" className="title">
        Updates
      </h2>
      <p className="body-sm">You have version {version ?? "unknown"}.</p>
      <label className="choice">
        <input
          type="checkbox"
          checked={autoCheck}
          onChange={(e) => onAutoCheck(e.target.checked)}
        />
        <span>Check for updates when Space Optimiser starts</span>
      </label>
      <p className="body-sm ink-muted">
        A check asks the Space Optimiser download page on GitHub for the newest version. It sends no
        file names or paths. Every update is checked against the project&apos;s signing key before
        it is installed.
      </p>
      <div className="update-actions">
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => run(() => engine.checkForUpdate())}
        >
          Check for updates
        </button>
      </div>
      <div role="status" aria-live="polite" className="stack">
        <Status
          view={view}
          onInstall={() => run(() => engine.installUpdate())}
          onRestart={() => run(() => engine.restartApp())}
          onDownloadPage={() => run(() => engine.openDownloadPage())}
        />
      </div>
      {error && (
        <p className="notice notice-warning body-sm" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

interface StatusProps {
  view: UpdateView;
  onInstall: () => void;
  onRestart: () => void;
  onDownloadPage: () => void;
}

function Status({ view, onInstall, onRestart, onDownloadPage }: StatusProps) {
  switch (view.state) {
    case "idle":
      return null;
    case "checking":
      return <p className="body-sm">Checking…</p>;
    case "up_to_date":
      return (
        <p className="body-sm">Space Optimiser is up to date (version {view.current_version}).</p>
      );
    case "failed":
      return (
        <p className="notice notice-warning body-sm" role="alert">
          {errorMessage(view.message)}
        </p>
      );
    case "available":
      return (
        <>
          <p className="heading">
            Version {view.version} is available. You have {view.current_version}.
          </p>
          {view.notes && <p className="body-sm release-notes">{view.notes}</p>}
          {view.route === "in_app" ? (
            <div className="update-actions">
              <button type="button" className="button button-primary" onClick={onInstall}>
                Download and install
              </button>
            </div>
          ) : (
            <>
              <p className="body-sm ink-muted">{LINK_REASONS[view.route]}</p>
              <div className="update-actions">
                <button type="button" className="button" onClick={onDownloadPage}>
                  Open download page
                </button>
              </div>
            </>
          )}
        </>
      );
    case "downloading":
      return (
        <>
          <p className="body-sm">
            Downloading version {view.version}: {formatBytes(view.downloaded)}
            {view.total !== null && ` of ${formatBytes(view.total)}`}
          </p>
          <progress
            className="progress"
            aria-label="Download progress"
            max={view.total ?? undefined}
            value={view.total !== null ? view.downloaded : undefined}
          />
        </>
      );
    case "waiting":
      return (
        <p className="body-sm">
          Version {view.version} is downloaded and verified. It installs when the scan or cleanup
          finishes.
        </p>
      );
    case "installed":
      return (
        <>
          <p className="body-sm">
            Version {view.version} is installed. Restart Space Optimiser to use it.
          </p>
          <div className="update-actions">
            <button type="button" className="button button-primary" onClick={onRestart}>
              Restart now
            </button>
          </div>
        </>
      );
  }
}

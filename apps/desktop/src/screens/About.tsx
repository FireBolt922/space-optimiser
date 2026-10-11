import licenceText from "../../../../LICENSE?raw";
import oflText from "../assets/fonts/OFL.txt?raw";
import type { AppInfo } from "../engine";
import { NO_UPLOAD, SAFETY_WORDING } from "../copy";

interface Props {
  info: AppInfo | null;
  onHelp: () => void;
}

// Everything here is local: no external links and no network fetch.
export function About({ info, onHelp }: Props) {
  return (
    <section className="screen" aria-labelledby="about-title">
      <h1 id="about-title" className="title-xl" tabIndex={-1}>
        About
      </h1>
      <div className="card stack">
        <p className="heading">Space Optimiser</p>
        <p className="body-sm">Version {info?.version ?? "unknown"}</p>
        <p className="body-sm ink-muted">{SAFETY_WORDING}</p>
        <p className="body-sm">{NO_UPLOAD}</p>
        <p className="body-sm">This version does not update itself.</p>
      </div>

      <div className="card stack">
        <h2 className="title">Audit log</h2>
        <p className="body-sm">Each cleanup is recorded in a file on this computer:</p>
        <p className="code path">{info?.audit_log_path ?? "unknown"}</p>
        <p className="body-sm ink-muted">
          You can delete this file yourself. Space Optimiser does not clean it.
        </p>
      </div>

      <div className="card stack">
        <h2 className="title">Licences</h2>
        <p className="body-sm">Space Optimiser is free software under the MIT License.</p>
        <details>
          <summary className="body-sm summary">Show the MIT License text</summary>
          <pre className="code log" tabIndex={0} aria-label="MIT License text">
            {licenceText}
          </pre>
        </details>
        <p className="body-sm">The Inter typeface is used under the SIL Open Font License 1.1.</p>
        <details>
          <summary className="body-sm summary">Show the Open Font License text</summary>
          <pre className="code log" tabIndex={0} aria-label="Open Font License text">
            {oflText}
          </pre>
        </details>
      </div>

      <div className="actions actions-start">
        <button type="button" className="button" onClick={onHelp}>
          Open Help
        </button>
      </div>
    </section>
  );
}

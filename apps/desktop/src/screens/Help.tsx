import type { AppInfo } from "../engine";

interface Props {
  info: AppInfo | null;
}

// A static, local page. It answers the questions the review and report raise.
export function Help({ info }: Props) {
  return (
    <section className="screen" aria-labelledby="help-title">
      <h1 id="help-title" className="title-xl" tabIndex={-1}>
        Help
      </h1>

      <section className="card stack" aria-labelledby="help-modes">
        <h2 id="help-modes" className="title">
          Normal Scan and Deep Scan
        </h2>
        <p className="body-sm">
          Normal Scan is for everyone. It checks temporary files, thumbnails, font caches, and
          browser caches.
        </p>
        <p className="body-sm">
          Deep Scan is for developers. It does everything Normal Scan does, and also checks package
          caches, old logs, and Docker&apos;s build cache, and lists large files. Large files are
          shown so you can decide; they are never removed.
        </p>
        <p className="body-sm ink-muted">
          A scan only looks. Nothing is removed until you review the list and confirm.
        </p>
      </section>

      <section className="card stack" aria-labelledby="help-safe">
        <h2 id="help-safe" className="title">
          What Select safe items includes
        </h2>
        <p className="body-sm">
          Select safe items ticks only Low risk items that apps recreate on their own, such as old
          temporary files, thumbnails, and old log files.
        </p>
        <p className="body-sm">
          It leaves out items that cause a download again or that a browser is using, such as
          browser caches, package caches, and tool commands. You can still choose those one at a
          time. Items marked &ldquo;Not sure — not offered&rdquo; cannot be chosen.
        </p>
      </section>

      <section className="card stack" aria-labelledby="help-undo">
        <h2 id="help-undo" className="title">
          A cleanup cannot be undone
        </h2>
        <p className="body-sm">
          Removed files do not go to the Trash or the Recycle Bin. The review screen lists every
          action before anything runs.
        </p>
      </section>

      <section className="card stack" aria-labelledby="help-space">
        <h2 id="help-space" className="title">
          Why free space can change by a different amount
        </h2>
        <p className="body-sm">
          The report shows the size of the files removed and the free space before and after. These
          can differ because:
        </p>
        <ul className="bullets body-sm">
          <li>
            Local snapshots keep removed files until the snapshot goes: Time Machine on macOS, Btrfs
            or ZFS snapshots on Linux, restore points on Windows.
          </li>
          <li>macOS counts some space as purgeable and frees it later, when space is needed.</li>
          <li>The system can free space a little after the files are removed.</li>
          <li>
            Docker Desktop keeps its data in one disk file, which does not shrink when files inside
            it are removed.
          </li>
          <li>Other apps can write files while the cleanup runs.</li>
        </ul>
      </section>

      <section className="card stack" aria-labelledby="help-log">
        <h2 id="help-log" className="title">
          Audit log and privacy
        </h2>
        <p className="body-sm">
          Each cleanup is recorded in an audit log on this computer
          {info ? ":" : "."}
        </p>
        {info && <p className="code path">{info.audit_log_path}</p>}
        <p className="body-sm">
          You can delete the log yourself. Space Optimiser does not upload a list of your files, and
          Export report saves the report only to a file on this computer.
        </p>
      </section>

      <section className="card stack" aria-labelledby="help-admin">
        <h2 id="help-admin" className="title">
          Administrator access
        </h2>
        <p className="body-sm">
          Space Optimiser never runs as administrator or root and never asks for your password. If
          it is started that way, it stops and says so.
        </p>
      </section>
    </section>
  );
}

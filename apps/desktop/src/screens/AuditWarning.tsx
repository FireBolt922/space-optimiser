interface Props {
  diskFull: boolean;
  onBack: () => void;
  onContinue: () => void;
}

// The second confirm for a log that cannot be written (phase 2 item 10).
export function AuditWarning({ diskFull, onBack, onContinue }: Props) {
  return (
    <section className="screen" aria-labelledby="audit-title">
      <h1 id="audit-title" className="title-xl" tabIndex={-1}>
        No log file
      </h1>
      <p className="body" role="alert">
        {diskFull
          ? "The audit log could not be written because the disk is full."
          : "The audit log could not be written."}
      </p>
      <p className="body-sm ink-muted">
        Nothing has been removed. If you continue, the log is kept in memory and shown in the
        report instead of being saved. This cannot be undone.
      </p>
      <div className="actions">
        <button type="button" className="button" onClick={onBack}>
          Back
        </button>
        <button type="button" className="button button-primary" onClick={onContinue}>
          Clean without a log file
        </button>
      </div>
    </section>
  );
}

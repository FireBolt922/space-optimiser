interface Props {
  done: number;
  total: number;
  stopping: boolean;
  onStop: () => void;
}

export function Running({ done, total, stopping, onStop }: Props) {
  return (
    <section className="screen" aria-labelledby="running-title">
      <h1 id="running-title" className="title-xl" tabIndex={-1}>
        Cleaning
      </h1>
      <p className="body" role="status">
        {done} of {total} items done
      </p>
      <progress className="progress" max={total} value={done} aria-label="Cleaning progress" />
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

import type { ScanView } from "../engine";
import { formatBytes, plural } from "../format";

interface Props {
  view?: ScanView;
  cancelling: boolean;
  onCancel: () => void;
}

export function Scanning({ view, cancelling, onCancel }: Props) {
  const progress = view?.progress;
  return (
    <section className="screen" aria-labelledby="scanning-title">
      <h1 id="scanning-title" className="title-xl">
        Scanning
      </h1>
      <p className="body" role="status">
        Checking temporary files.
        {progress && progress.files_seen > 0 && (
          <>
            {" "}
            {plural(progress.files_seen, "file", "files")} so far,{" "}
            {formatBytes(progress.bytes_seen)}.
          </>
        )}
      </p>
      <div className="actions">
        <button type="button" className="button" onClick={onCancel} disabled={cancelling}>
          {cancelling ? "Cancelling" : "Cancel"}
        </button>
      </div>
    </section>
  );
}

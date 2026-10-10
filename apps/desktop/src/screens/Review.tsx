import type { Plan } from "../engine";
import { formatBytes } from "../format";

interface Props {
  plan: Plan;
  expired: boolean;
  busy: boolean;
  error?: string;
  onBack: () => void;
  onConfirm: () => void;
  onScanAgain: () => void;
}

// Nothing runs on arrival: the plan runs only from the Confirm button.
export function Review({ plan, expired, busy, error, onBack, onConfirm, onScanAgain }: Props) {
  return (
    <section className="screen" aria-labelledby="review-title">
      <h1 id="review-title" className="title-xl">
        Review
      </h1>
      <p className="body">These actions run when you confirm:</p>
      <ul className="card plain-list review-list">
        {plan.items.map((item) => (
          <li key={item.candidate_id} className="review-item">
            <span className="file-name heading">{item.display_name}</span>
            <span className="body-sm">{item.action_text}</span>
            <span className="body-sm ink-muted">About {formatBytes(item.estimated_bytes)}</span>
          </li>
        ))}
      </ul>
      <p className="body">
        About <strong>{formatBytes(plan.estimated_bytes)}</strong> will be removed.
      </p>
      <p className="body-sm ink-muted">
        Space Optimiser only removes files in locations it has a reviewed rule for, and only
        after you confirm. It does not clean your documents, photos, downloads, or source code,
        and it does not change system settings, passwords, or app configuration. A confirmed
        cleanup can still make an app rebuild a cache or download files again.
      </p>
      <p className="body warning-text">
        <strong>This cannot be undone.</strong> Deleted files do not go to the Trash.
      </p>
      {error && (
        <p className="notice notice-warning" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="button" className="button" onClick={onBack} disabled={busy}>
          Back
        </button>
        {expired ? (
          <button type="button" className="button button-primary" onClick={onScanAgain}>
            Scan again
          </button>
        ) : (
          <button
            type="button"
            className="button button-primary"
            onClick={onConfirm}
            disabled={busy}
          >
            Confirm
          </button>
        )}
      </div>
    </section>
  );
}

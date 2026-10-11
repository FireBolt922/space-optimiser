import type { Plan } from "../engine";
import { SAFETY_WORDING } from "../copy";
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
  const hasDeletes = plan.items.some((i) => i.upper_bound_bytes === null);
  return (
    <section className="screen" aria-labelledby="review-title">
      <h1 id="review-title" className="title-xl" tabIndex={-1}>
        Review
      </h1>
      <p className="body">These actions run when you confirm:</p>
      <ul className="card plain-list review-list">
        {plan.items.map((item) => (
          <li key={item.candidate_id} className="review-item">
            <span className="file-name heading">{item.display_name}</span>
            <span className="body-sm">{item.action_text}</span>
            <span className="body-sm ink-muted">
              {item.upper_bound_bytes === null
                ? `About ${formatBytes(item.estimated_bytes)}`
                : `Up to ${formatBytes(item.upper_bound_bytes)}`}
            </span>
          </li>
        ))}
      </ul>
      {hasDeletes && (
        <p className="body">
          About <strong>{formatBytes(plan.estimated_bytes)}</strong> will be removed.
        </p>
      )}
      {plan.tool_upper_bound_bytes > 0 && (
        <p className="body">
          Tools may free up to {formatBytes(plan.tool_upper_bound_bytes)} more. Each tool decides
          what to keep, so the space freed can be smaller.
        </p>
      )}
      <p className="body-sm ink-muted">{SAFETY_WORDING}</p>
      <p className="body warning-text">
        <strong>This cannot be undone.</strong> Deleted files do not go to the Trash.
      </p>
      {error && (
        <p className="notice notice-warning" role="alert">
          {error}
        </p>
      )}
      <div className="actions actions-sticky">
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

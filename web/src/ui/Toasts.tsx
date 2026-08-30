import { Button } from "./Button";
import { AlertIcon, CheckCircleIcon, CloseIcon, InfoIcon } from "./Icon";

export type ToastTone = "success" | "danger" | "info";

export interface Toast {
  id: number;
  tone: ToastTone;
  title: string;
  detail?: string;
}

export function Toasts({ toasts, closeLabel, onDismiss }: {
  toasts: Toast[];
  closeLabel: string;
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast--${toast.tone}`}>
          <span className="toast__icon">
            {toast.tone === "success" ? <CheckCircleIcon /> : null}
            {toast.tone === "danger" ? <AlertIcon /> : null}
            {toast.tone === "info" ? <InfoIcon /> : null}
          </span>
          <span className="toast__text">
            <strong>{toast.title}</strong>
            {toast.detail !== undefined && <span>{toast.detail}</span>}
          </span>
          <Button
            variant="ghost"
            size="sm"
            iconOnly
            aria-label={closeLabel}
            onClick={() => onDismiss(toast.id)}
          >
            <CloseIcon size={14} />
          </Button>
        </div>
      ))}
    </div>
  );
}

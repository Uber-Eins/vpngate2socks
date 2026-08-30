import { useEffect, useRef, type ReactNode } from "react";

import { Button } from "./Button";
import { CloseIcon } from "./Icon";

/**
 * Modal built on the native `<dialog>` element, which brings focus trapping, the
 * top layer, and Escape handling without a library — and without the inline
 * positioning styles the served CSP blocks.
 */
export function Dialog({ open, title, subtitle, closeLabel, footer, onClose, children }: {
  open: boolean;
  title: string;
  subtitle?: string;
  closeLabel: string;
  footer?: ReactNode;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const node = ref.current;
    if (node === null) return;
    // jsdom did not always implement the top layer, so the fallback keeps tests honest.
    if (open && !node.open) {
      if (typeof node.showModal === "function") node.showModal();
      else node.setAttribute("open", "");
    }
    if (!open && node.open) node.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-label={title}
      onClose={onClose}
      onClick={(event) => {
        if (event.target === ref.current) onClose();
      }}
    >
      {open && (
        <>
          <header className="dialog__header">
            <div className="dialog__title">
              <div>
                <h2>{title}</h2>
                {subtitle !== undefined && <p>{subtitle}</p>}
              </div>
            </div>
            <Button variant="ghost" iconOnly aria-label={closeLabel} onClick={onClose}>
              <CloseIcon />
            </Button>
          </header>
          <div className="dialog__body">{children}</div>
          {footer !== undefined && <footer className="dialog__footer">{footer}</footer>}
        </>
      )}
    </dialog>
  );
}

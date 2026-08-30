import type { ButtonHTMLAttributes } from "react";

import { cx } from "../utils/cx";
import { Spinner } from "./Feedback";

type Variant = "default" | "primary" | "danger" | "ghost";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: "sm" | "md";
  /** Square button with no label; `aria-label` is then required by callers. */
  iconOnly?: boolean;
  wide?: boolean;
  busy?: boolean;
}

export function Button({
  variant = "default",
  size = "md",
  iconOnly = false,
  wide = false,
  busy = false,
  className,
  children,
  disabled,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      {...rest}
      type={type === "submit" ? "submit" : "button"}
      disabled={disabled === true || busy}
      className={cx(
        "button",
        variant !== "default" && `button--${variant}`,
        size === "sm" && "button--sm",
        iconOnly && "button--icon",
        wide && "button--wide",
        className
      )}
    >
      {busy ? <Spinner /> : null}
      {children}
    </button>
  );
}

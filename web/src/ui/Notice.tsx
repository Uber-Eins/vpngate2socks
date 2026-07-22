import type { ReactNode } from "react";

export function Notice({ children, tone }: {
  children: ReactNode;
  tone: "neutral" | "danger" | "success";
}) {
  return (
    <div className={`notice notice--${tone}`} role={tone === "danger" ? "alert" : "status"}>
      <span className="notice__icon" aria-hidden="true">{tone === "danger" ? "!" : "i"}</span>
      <span>{children}</span>
    </div>
  );
}

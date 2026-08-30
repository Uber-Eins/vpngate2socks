import { useCallback, useEffect, useState } from "react";

export const VIEWS = ["overview", "nodes", "policy", "security"] as const;
export type View = (typeof VIEWS)[number];

function readHash(): View {
  const value = window.location.hash.replace(/^#\/?/, "");
  return (VIEWS as readonly string[]).includes(value) ? (value as View) : "overview";
}

/** Keeps the active view in the URL fragment so a reload stays on the same screen. */
export function useView(): [View, (view: View) => void] {
  const [view, setView] = useState<View>(readHash);

  useEffect(() => {
    const sync = () => setView(readHash());
    window.addEventListener("hashchange", sync);
    return () => window.removeEventListener("hashchange", sync);
  }, []);

  const navigate = useCallback((next: View) => {
    window.location.hash = `#/${next}`;
    setView(next);
  }, []);

  return [view, navigate];
}

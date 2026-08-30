import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import "./styles/index.css";

async function bootstrap(): Promise<void> {
  // Tree-shaken out of production builds; see src/mock/install.ts.
  if (import.meta.env.DEV && import.meta.env.VITE_MOCK === "1") {
    await import("./mock/install");
  }

  const root = document.getElementById("root");
  if (root === null) throw new Error("missing root element");

  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>
  );
}

void bootstrap();

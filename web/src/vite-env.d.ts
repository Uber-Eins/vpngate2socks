/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set to "1" by `npm run dev:mock` to load the in-browser control-plane stub. */
  readonly VITE_MOCK?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Company server baked in at build time, e.g. https://time.example.com (see widget/README.md). */
  readonly VITE_TIMEPILOT_SERVER_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

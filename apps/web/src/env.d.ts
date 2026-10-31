/// <reference types="svelte" />
/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" = modo mock (solo desarrollo, sin server). */
  readonly VITE_MOCK?: string;
  /** Server por defecto del desktop (apps/web/.env.local; no va en el repo). */
  readonly VITE_DEFAULT_SERVER?: string;
}

// hls.js trae los tipos solo para la version completa; la liviana tiene la misma API.
declare module "hls.js/light" {
  import Hls from "hls.js";
  export * from "hls.js";
  export default Hls;
}

// View Transitions API (no esta en todas las versiones de lib.dom).
interface ViewTransitionLike {
  finished: Promise<void>;
  ready: Promise<void>;
  updateCallbackDone: Promise<void>;
  skipTransition(): void;
}
interface Document {
  startViewTransition?(cb: () => Promise<void> | void): ViewTransitionLike;
}

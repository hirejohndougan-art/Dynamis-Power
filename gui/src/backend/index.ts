import type { Backend } from "./types";
import { forwardChanges, tauriBackend } from "./tauri";
import { mockBackend } from "./mock";

/** Inside the Tauri app use the real service; in a plain browser (development,
 * tests, design review) use an in-memory stand-in. */
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const backend: Backend = inTauri ? tauriBackend : mockBackend();
if (inTauri) forwardChanges();
export type * from "./types";

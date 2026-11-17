import type { Backend } from "./types";
import { forwardChanges, tauriBackend } from "./tauri";
import { mockBackend } from "./mock";

/** Inside the Tauri app use the real service; in a plain browser (development,
 * tests, design review) use an in-memory stand-in. */
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
// In the installed app (__STAND_IN__ false) this folds to the real service
// and the stand-in is left out of the bundle.
export const backend: Backend = !__STAND_IN__ ? tauriBackend : inTauri ? tauriBackend : mockBackend();
if (inTauri) forwardChanges();
export type * from "./types";

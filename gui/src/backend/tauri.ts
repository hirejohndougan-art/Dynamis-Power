// The real backend: Tauri commands in src-tauri, which call the service over D-Bus.
import { invoke } from "@tauri-apps/api/core";
import type { Backend } from "./types";

export const tauriBackend: Backend = {
  status: () => invoke("status"),
  journal: (n) => invoke("journal", { n }),
  settings: () => invoke("settings"),
  inventory: () => invoke("inventory"),
  learned: () => invoke("learned"),
  setProfile: (profile) => invoke("set_profile", { profile }),
  setAuto: (on) => invoke("set_auto", { on }),
  setLearning: (on) => invoke("set_learning", { on }),
  undo: (setting) => invoke("undo", { setting }),
  restore: () => invoke("restore"),
  setSetting: (setting, value, allowRisky) => invoke("set_setting", { setting, value, allowRisky }),
  setSafeMode: (on) => invoke("set_safe_mode", { on }),
  forget: (setting) => invoke("forget", { setting }),
  conflicts: () => invoke("conflicts"),
  switchOffConflict: (unit) => invoke("switch_off_conflict", { unit }),
  autoPrefs: () => invoke("auto_prefs"),
  setAutoPrefs: (prefs) => invoke("set_auto_prefs", { prefs }),
  saveFile: (name, contents) => invoke("save_file", { name, contents }),
};

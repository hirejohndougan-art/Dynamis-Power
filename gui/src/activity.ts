// Turning journal entries into plain-language Activity items.
import type { JournalEntry } from "./backend";
import { appName } from "./apps";
import { t, valueName, type Key } from "./i18n";

export type Category = "auto" | "mine" | "tests" | "rollbacks" | "problems";

export interface Item {
  entry: JournalEntry;
  title: string;
  why: string;
  category: Category;
  /** Undo puts the setting back to its original value. */
  undoable: boolean;
  /** Auto mode's switches within one hour, shown as one line (§3). */
  group?: Item[];
}

/** Auto mode's switches (entries whose reason starts "auto:") come in runs;
 * three or more in the same hour become one line that opens to show them,
 * so they do not flood Activity (everyday problem §3). Order is kept. */
export function groupAuto(items: Item[]): Item[] {
  const out: Item[] = [];
  let run: Item[] = [];
  const isAuto = (i: Item) => (i.entry.reason ?? "").startsWith("auto:");
  const hour = (i: Item) => Math.floor(i.entry.time / 3600);
  const flush = () => {
    if (run.length >= 3) {
      out.push({ ...run[0], title: t("ev_auto_group", { n: run.length }), why: run[0].why, undoable: false, group: run });
    } else {
      out.push(...run);
    }
    run = [];
  };
  for (const i of items) {
    if (isAuto(i) && (run.length === 0 || hour(run[0]) === hour(i))) {
      run.push(i);
      continue;
    }
    flush();
    if (isAuto(i)) run.push(i);
    else out.push(i);
  }
  flush();
  return out;
}

const strip = (reason = "", prefix: string) => reason.replace(prefix, "").replace(/; ok$/, "").trim();

/** `null` for technical entries the Activity tab hides (Logs shows everything). */
export function describe(e: JournalEntry): Item | null {
  const reason = e.reason ?? "";
  const label = e.label || e.id;
  const v = { label, from: valueName(e.id, e.from), to: valueName(e.id, e.to) };
  const item = (title: string, why: string, category: Category, undoable = false): Item => ({ entry: e, title, why, category, undoable });

  switch (e.event) {
    case "start":
    case "undone":
      return null;
    case "done": {
      if (e.from === e.to) return null;
      if (reason.startsWith("auto:")) return item(t("ev_changed", v), t("why_auto", { reason: strip(reason, "auto:") }), "auto");
      if (reason.startsWith("learned:")) return item(t("ev_changed", v), t("why_learned", { reason: strip(reason, "learned:") }), "auto", true);
      if (reason.startsWith("calibration")) return item(t("ev_changed", v), t("why_test", { reason: strip(reason, "calibration:") }), "tests");
      if (reason.includes("learned settings") || reason.includes("learning switched off")) return item(t("ev_changed", v), reason, "auto");
      return item(t("ev_changed", v), t("why_mine"), "mine", true);
    }
    case "keep":
      return item(t("ev_kept", v), reason || t("why_kept"), "tests", true);
    case "revert": {
      const why = strip(reason, "calibration:");
      if (reason.includes("health")) return item(t("ev_reverted", v), why, "problems");
      return item(t("ev_reverted", v), why, reason.startsWith("calibration") ? "tests" : "rollbacks");
    }
    case "restore":
      return item(t("ev_restored", v), "", "rollbacks");
    case "recover":
      return item(t("ev_recovered", v), strip(reason, ""), "problems");
    case "quirk":
      return item(t("ev_quirk", v), strip(reason, ""), "rollbacks");
    case "block":
      return item(t("ev_blocked", v), reason, "problems");
    case "device-power": {
      const dev = e.label || e.id.replace(/^(usb|pci):/, "");
      return item(t(e.to === "auto" ? "ev_dev_sleep" : "ev_dev_awake", { dev }), reason, reason.startsWith("on battery") ? "auto" : "mine");
    }
    case "charge-behaviour":
      return item(t(e.to === "force-discharge" ? "ev_bat_run_down" : "ev_bat_normal"), reason, reason.includes("you stopped") ? "mine" : "auto");
    case "radio":
      return item(t(e.to === "off" ? "ev_radio_off" : "ev_radio_on", { name: e.label || e.id }), reason, reason === "restore" ? "rollbacks" : "auto");
    case "import":
      return item(t("ev_import"), reason, "mine");
    case "kernel":
      return item(t("ev_kernel", { what: t(`kernel_kind_${e.id}` as Key) }), reason, "problems");
    case "problem":
      return item(t("ev_problem"), reason, "problems");
    case "brightness":
      return item(t("ev_brightness", { from: e.from ?? "", to: e.to ?? "" }), reason, reason.startsWith("you chose") ? "mine" : "auto");
    case "outside": {
      const tool = reason.match(/likely by (.+)$/)?.[1];
      return item(t("ev_outside", v), tool ? t("why_outside_tool", { tool }) : t("why_outside"), "problems");
    }
    case "failed":
      return item(t("ev_failed", v), reason, "problems");
    case "unblock":
      return item(t("ev_unblocked", v), "", "mine");
    case "safe-mode-on":
      return item(t("ev_safe_on"), reason, "rollbacks");
    case "safe-mode-off":
      return item(t("ev_safe_off"), "", "mine");
    case "would":
      return item(t("ev_would", v), strip(reason, ""), "mine");
    case "dry-run-on":
      return item(t("ev_dry_on"), "", "mine");
    case "dry-run-off":
      return item(t("ev_dry_off"), "", "mine");
    case "app-weight": {
      const app = appName(e.id);
      const to = Number(e.to), from = Number(e.from);
      const title = to < from ? t("ev_app_slow", { app }) : to > 100 ? t("ev_app_front", { app }) : t("ev_app_normal", { app });
      return item(title, reason, reason.startsWith("you chose") ? "mine" : "auto");
    }
    default:
      return null;
  }
}

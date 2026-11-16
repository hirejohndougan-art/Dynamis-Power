// An in-memory stand-in for the service, used in a plain browser (development,
// design review) and in tests. It behaves like the real thing closely enough
// to exercise every screen: changes are journaled, Undo works, safe mode
// refuses changes.
import type { AutoPrefs, Backend, Conflict, JournalEntry, Learned, Profile, SettingRow, Status } from "./types";

const now = () => Math.floor(Date.now() / 1000);

export function mockBackend(): Backend {
  let profile: Profile = "balanced";
  let auto = true;
  let learning = true;
  let safeMode: string | null = null;
  const values: Record<string, string> = {
    "cpu.epp": "balance_performance",
    "cpu.turbo": "on",
    "cpu.max_perf_pct": "100",
    "vm.swappiness": "60",
    "vm.dirty_writeback": "500",
    "kernel.nmi_watchdog": "off",
    "audio.power_save": "1",
    "pcie.aspm": "default",
  };
  const original: Record<string, string> = { "kernel.nmi_watchdog": "on" };
  const t0 = now();
  const journal: JournalEntry[] = [
    { v: 1, time: t0 - 7200, event: "done", id: "cpu.epp", label: "CPU speed preference", from: "balance_performance", to: "power", reason: "auto: Nobody is using the laptop: saving power" },
    { v: 1, time: t0 - 6800, event: "done", id: "cpu.epp", label: "CPU speed preference", from: "power", to: "balance_performance", reason: "auto: On battery" },
    { v: 1, time: t0 - 5400, event: "revert", id: "cpu.turbo", label: "Turbo boost", from: "off", to: "on", reason: "calibration: made the laptop less responsive (+4.8 points of stall); ok" },
    { v: 1, time: t0 - 3600, event: "keep", id: "kernel.nmi_watchdog", label: "Watchdog wakeups", from: "on", to: "off", reason: "" },
    { v: 1, time: t0 - 600, event: "done", id: "cpu.epp", label: "CPU speed preference", from: "balance_performance", to: "balance_performance", reason: "auto: On battery" },
  ];
  const records: Learned["records"] = [
    { setting: "cpu.turbo", value: "off", baseline: "on", verdict: "reject", reason: "made the laptop less responsive (+4.8 points of stall)", effect: { mean: -0.79, ci_low: -0.9, ci_high: -0.68, confidence: 1 }, stall: { mean: 4.8 }, time: t0 - 5400 },
    { setting: "vm.dirty_writeback", value: "1500", baseline: "500", verdict: "reject", reason: "no useful saving", effect: { mean: 0.01, ci_low: -0.04, ci_high: 0.06, confidence: 0.2 }, stall: { mean: 0 }, time: t0 - 4500 },
    { setting: "kernel.nmi_watchdog", value: "off", baseline: "on", verdict: "keep", reason: "saves 0.39 W (about 6%)", effect: { mean: -0.39, ci_low: -0.45, ci_high: -0.33, confidence: 1 }, stall: { mean: 0 }, time: t0 - 3600 },
  ];
  let watts = 5.6;
  let conflicts: Conflict[] = [{ unit: "tlp.service", name: "TLP", active: true, state: "enabled" }];
  let prefs: AutoPrefs = { on_ac: "balanced", on_battery: "balanced", boost_on_ac: true, low_battery_pct: 30, critical_battery_pct: 15 };

  const log = (e: Omit<JournalEntry, "v" | "time">) => journal.push({ v: 1, time: now(), ...e });
  const refuse = () => {
    if (safeMode) throw new Error(`safe mode is on (${safeMode}); changes are refused, restore still works`);
  };
  const label = (id: string) => rows().find((r) => r.id === id)?.label ?? id;
  const kept = () => records.filter((r) => r.verdict === "keep");

  const rows = (): SettingRow[] => {
    const row = (id: string, lbl: string, risk: SettingRow["risk"], riskLabel: string, allowed: string, available = true): SettingRow => ({
      id, label: lbl, risk, risk_label: riskLabel, available, allowed,
      current: available ? values[id] ?? null : null, original: original[id] ?? null, blocked: null,
      not_allowed: id === "pcie.aspm" ? [[["powersave"], "Realtek RTL8821CE Wi-Fi is widely reported to drop out or freeze the system when PCIe link power saving is forced on."]] : [],
    });
    return [
      row("cpu.epp", "CPU speed preference", "safe", "Safe", "performance | balance_performance | balance_power | power"),
      row("cpu.turbo", "Turbo boost", "safe", "Safe", "on | off"),
      row("cpu.max_perf_pct", "CPU speed limit", "safe", "Safe", "30–100%"),
      row("platform.profile", "Firmware power mode", "safe", "Safe", "not offered by this machine", false),
      row("vm.swappiness", "Swap eagerness", "safe", "Safe", "0–200"),
      row("vm.dirty_writeback", "Disk write batching", "usually_safe", "Usually safe", "100–6000 cs"),
      row("kernel.nmi_watchdog", "Watchdog wakeups", "safe", "Safe", "on | off"),
      row("audio.power_save", "Sound chip sleep (seconds idle, 0 = never)", "usually_safe", "Usually safe", "0–60 s"),
      row("pcie.aspm", "PCIe link power saving", "usually_safe", "Usually safe", "default | performance | powersave"),
      row("battery.charge_limit", "Battery charge limit", "safe", "Safe", "this battery has no charge limit control", false),
    ];
  };

  const setValue = (id: string, value: string, reason: string) => {
    const from = values[id];
    if (from === value) return;
    if (!(id in original)) original[id] = from;
    values[id] = value;
    log({ event: "done", id, label: label(id), from, to: value, reason });
  };

  const status = (): Status => {
    watts = Math.max(3.5, Math.min(8, watts + (Math.random() - 0.5) * 0.4));
    return {
      version: "0.1.0 (preview)",
      profile,
      profiles: ["power-saver", "balanced", "performance"],
      holds: [],
      auto: { on: auto, workload: auto ? "light_interactive" : null, reason: auto ? "On battery" : "Auto mode is off" },
      learning: {
        on: learning,
        status: learning ? "Waiting: light use right now" : "Learning is off",
        tests: records.length,
        tested_settings: new Set(records.filter((r) => r.verdict !== "inconclusive").map((r) => r.setting)).size,
        kept: kept().map((r) => ({ setting: r.setting, label: label(r.setting), value: r.value, baseline: r.baseline, saves_w: -r.effect.mean, reason: r.reason })),
        saves_w: kept().reduce((a, r) => a - r.effect.mean, 0),
      },
      power: {
        on_ac: false, has_battery: true, battery_pct: 79, charging: false,
        watts: Math.round(watts * 100) / 100, hours_left: Math.round((31.2 / watts) * 10) / 10, hours_to_full: null,
        health_pct: 90.5, cycles: 212, design_wh: 42, full_wh: 38, charge_limit: null,
      },
      safe_mode: safeMode,
      pending: null,
      display: { w_per_pct: 0.031, base_w: 3.9, r2: 0.86, samples: 412 },
    };
  };

  const delay = <T,>(v: T) => new Promise<T>((r) => setTimeout(() => r(v), 30));

  return {
    status: async () => delay(status()),
    journal: async (n) => delay([...journal].reverse().slice(0, n)),
    settings: async () => delay(rows()),
    inventory: async () => delay({ machine: { vendor: "Example Computers Inc.", model: "Synthetic Laptop 14", form_factor: "laptop" }, cpu: { model: "Example CPU", cores: 2 } }),
    learned: async () => delay({ records, learned: kept(), display: status().display }),
    setProfile: async (p) => {
      refuse();
      profile = p;
      const epp = { "power-saver": "power", balanced: "balance_performance", performance: "performance" }[p];
      setValue("cpu.epp", epp, `profile ${p} chosen`);
    },
    setAuto: async (on) => { refuse(); auto = on; },
    setLearning: async (on) => {
      learning = on;
      if (!on) for (const r of kept()) setValue(r.setting, r.baseline, "learning switched off");
    },
    undo: async (id) => {
      refuse();
      const o = original[id];
      if (o === undefined || values[id] === o) return `${label(id)} was never changed`;
      const idx = records.findIndex((r) => r.setting === id && r.verdict === "keep");
      if (idx >= 0) records.splice(idx, 1);
      log({ event: "restore", id, label: label(id), to: o, reason: "undo; ok" });
      values[id] = o;
      return `${label(id)} is back to ${o}`;
    },
    restore: async () => {
      auto = false;
      learning = false;
      const done = Object.entries(original)
        .filter(([id, v]) => values[id] !== v)
        .map(([id, v]) => {
          values[id] = v;
          log({ event: "restore", id, label: label(id), to: v, reason: "ok" });
          return { id, label: label(id), value: v, ok: true };
        });
      return done;
    },
    setSetting: async (id, value) => { refuse(); setValue(id, value, "set in the app"); },
    setSafeMode: async (on) => {
      safeMode = on ? "switched on in the app" : null;
      log({ event: on ? "safe-mode-on" : "safe-mode-off", id: "", label: "", reason: on ? "switched on in the app" : "" });
      return safeMode ?? "";
    },
    forget: async (id) => {
      const had = records.some((r) => r.setting === id);
      for (let i = records.length - 1; i >= 0; i--) if (records[i].setting === id) records.splice(i, 1);
      if (original[id] !== undefined) values[id] = original[id];
      return had;
    },
    conflicts: async () => delay([...conflicts]),
    switchOffConflict: async (unit) => { conflicts = conflicts.filter((c) => c.unit !== unit); },
    autoPrefs: async () => delay({ ...prefs }),
    setAutoPrefs: async (p) => {
      if (p.low_battery_pct < 10 || p.low_battery_pct > 60) throw new Error("low battery level must be between 10 and 60");
      prefs = { ...p };
    },
    saveFile: async (name, contents) => {
      const url = URL.createObjectURL(new Blob([contents], { type: "text/plain" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = name;
      a.click();
      URL.revokeObjectURL(url);
      return `your Downloads folder (${name})`;
    },
  };
}

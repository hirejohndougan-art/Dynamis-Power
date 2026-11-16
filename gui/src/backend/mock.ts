// An in-memory stand-in for the service, used in a plain browser (development,
// design review) and in tests. It behaves like the real thing closely enough
// to exercise every screen: changes are journaled, Undo works, safe mode
// refuses changes.
import type { AppPolicy, AppsInfo, Predict, ProfileOverrides, ProfileSettings, Sensors, SystemInfo, AutoPrefs, Backend, Conflict, JournalEntry, Learned, Profile, SettingRow, Status } from "./types";

const now = () => Math.floor(Date.now() / 1000);

export function mockBackend(): Backend {
  let profile: Profile = "balanced";
  let auto = true;
  let pausedByHand = false;
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
    "battery.charge_limit": "100",
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
    { setting: "cpu.max_perf_pct", value: "90", baseline: "100", verdict: "keep", reason: "saves 0.21 W (about 4%)", effect: { mean: -0.21, ci_low: -0.27, ci_high: -0.15, confidence: 1 }, stall: { mean: 0.1 }, time: t0 - 2400, group: { "cpu.turbo": "on", "cpu.epp": "balance_performance" } },
  ];
  let watts = 5.6;
  let conflicts: Conflict[] = [{ unit: "tlp.service", name: "TLP", active: true, state: "enabled" }];
  let prefs: AutoPrefs = { on_ac: "balanced", on_battery: "balanced", boost_on_ac: true, low_battery_pct: 30, critical_battery_pct: 15, app_rules: [] };
  let appPolicy: AppPolicy = { v: 1, track: true, slow_background: false, boost_front: false, never_slow: [], always_slow: [] };
  let usage = {
    today: [
      { app: "firefox", cpu_s: 1840, wh: 4.1 },
      { app: "org.kde.dolphin", cpu_s: 120, wh: 0.3 },
      { app: "thunderbird", cpu_s: 410, wh: 0.9 },
      { app: "System", cpu_s: 600, wh: 1.2 },
    ],
    week: [
      { app: "firefox", cpu_s: 12800, wh: 27.5 },
      { app: "System", cpu_s: 4100, wh: 8.0 },
      { app: "thunderbird", cpu_s: 2900, wh: 6.1 },
      { app: "org.kde.dolphin", cpu_s: 800, wh: 1.9 },
      { app: "steam", cpu_s: 3000, wh: 0 },
    ],
  };
  let target: { until: number } | null = null;
  let adaptive = false;
  let tray = true;
  let dryRun = false;
  let overrides: ProfileOverrides = { v: 1, profiles: {} };
  const eppChoices = ["performance", "balance_performance", "balance_power", "power"];
  const profileDefaults: Record<Profile, string> = { "power-saver": "power", balanced: "balance_performance", performance: "performance" };
  const profileSettings = (): ProfileSettings => ({
    profiles: (["power-saver", "balanced", "performance"] as Profile[]).map((p) => ({
      profile: p,
      settings: [{ id: "cpu.epp", label: "CPU speed preference", choices: eppChoices, value: overrides.profiles[p]?.["cpu.epp"] ?? profileDefaults[p], default: profileDefaults[p] }],
    })),
    overrides: JSON.parse(JSON.stringify(overrides)),
  });
  let energy = 1_000_000_000;
  let idleUs = [5e9, 2e10, 9e10];
  const sensorsInfo = (): Sensors => {
    energy += 1_400_000 + Math.random() * 300_000;
    idleUs = idleUs.map((v, i) => v + [5e3, 2e5, 1.5e6][i] * (0.8 + Math.random() * 0.4));
    return {
      time: Date.now() / 1000,
      on_ac: false,
      battery_w: Math.round(watts * 100) / 100,
      battery_pct: 79,
      rapl: [{ id: "intel-rapl:0", name: "package-0", energy_uj: Math.round(energy), max_energy_range_uj: 262143328850 }],
      pressure: { cpu: 1.2, memory: 0, io: 0.4 },
      thermal: [{ name: "thermal_zone0", kind: "x86_pkg_temp", temp_c: 48 + Math.random() * 3 }, { name: "thermal_zone1", kind: "acpitz", temp_c: 41 }],
      cpu_mhz: 1100 + Math.round(Math.random() * 400),
      idle: { cpus: 2, states: ["POLL", "C1", "C10"].map((name, index) => ({ name, index, time_us: Math.round(idleUs[index]) })) },
      brightness_pct: 60,
    };
  };
  const systemInfo = (): SystemInfo => ({
    version: "0.1.0 (preview)", kernel: "6.17.0-generic", os: "Example Linux 26.04", uptime_s: 86_400, cpu_s: 95, cpu_pct: 0.11, memory_mb: 9.4, state_dir: "/var/lib/dynamis-power",
  });
  const predictInfo = (): Predict => {
    const n = now();
    return {
      forecast_end: n + 5.6 * 3600,
      typical_w: 5.4,
      usual_unplug: n + 14 * 3600,
      unplugs_seen: 23,
      target: target && { until: target.until, level: "no_boost", budget_w: 5.1, recent_w: 5.6, expected_end: n + 5 * 3600, on_track: false, reason: "Saving more to reach your time" },
      target_ended: null,
      adaptive_charging: adaptive,
      charge: adaptive ? { limit_pct: 80, until: n + 11.5 * 3600, reason: "Holding at 80% to spare the battery; it charges fully before you usually unplug" } : null,
      charge_supported: true,
    };
  };
  const appsInfo = (): AppsInfo => {
    const running = ["firefox", "org.kde.dolphin", "thunderbird"];
    const front = "firefox";
    const share = (app: string) => {
      if (appPolicy.never_slow.includes(app)) return null;
      if (appPolicy.always_slow.includes(app)) return 25;
      if (appPolicy.boost_front && app === front) return 400;
      if (appPolicy.slow_background && app !== front) return 25;
      return null;
    };
    return {
      policy: { ...appPolicy },
      rules: [...(prefs.app_rules ?? [])],
      front,
      running,
      now: [
        { app: "firefox", cpu_pct: 12.5, watts: 1.4, share: share("firefox") },
        { app: "thunderbird", cpu_pct: 1.2, watts: 0.1, share: share("thunderbird") },
      ],
      today: appPolicy.track ? [...usage.today].sort((a, b) => b.wh - a.wh) : [],
      week: appPolicy.track ? usage.week : [],
    };
  };

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
      row("battery.charge_limit", "Battery charge limit", "safe", "Safe", "50–100%"),
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
      auto: {
        on: auto,
        workload: auto ? "light_interactive" : null,
        reason: !auto ? "Auto mode is off" : pausedByHand ? "You picked a mode by hand; auto mode resumes when the charger is plugged in or out" : "On battery",
      },
      learning: {
        on: learning,
        status: learning ? "Waiting: light use right now" : "Learning is off",
        tests: records.length,
        tested_settings: new Set(records.filter((r) => r.verdict !== "inconclusive").map((r) => r.setting)).size,
        kept: kept().map((r) => ({ setting: r.setting, label: label(r.setting), value: r.value, baseline: r.baseline, saves_w: -r.effect.mean, reason: r.reason })),
        saves_w: kept().reduce((a, r) => a - r.effect.mean, 0),
        progress: (() => {
          const done = records.length + 1;
          const left = Math.max(0, 12 - done);
          return { pct: Math.floor((done / (done + left)) * 100), done, left, stage: left === 0 ? "done" : done < 8 ? "single" : "together", hours_left: (left * 780) / 3600 };
        })(),
      },
      power: {
        on_ac: false, has_battery: true, battery_pct: 79, charging: false,
        watts: Math.round(watts * 100) / 100, hours_left: Math.round((31.2 / watts) * 10) / 10, hours_to_full: null,
        health_pct: 90.5, cycles: 212, design_wh: 42, full_wh: 38, charge_limit: null,
      },
      safe_mode: safeMode,
      dry_run: dryRun,
      pending: null,
      display: { w_per_pct: 0.031, base_w: 3.9, r2: 0.86, samples: 412 },
      last_until: target && { until: target.until, level: "no_boost", reason: "Saving more to reach your time", on_track: false },
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
      // As the service does: picking a mode by hand pauses auto mode.
      if (auto) pausedByHand = true;
      const epp = { "power-saver": "power", balanced: "balance_performance", performance: "performance" }[p];
      setValue("cpu.epp", epp, `profile ${p} chosen`);
    },
    setAuto: async (on) => { refuse(); auto = on; pausedByHand = false; },
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
      appPolicy = { ...appPolicy, slow_background: false, boost_front: false, always_slow: [] };
      target = null;
      adaptive = false;
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
    apps: async () => delay(appsInfo()),
    setAppPolicy: async (p) => {
      refuse();
      if (p.never_slow.some((a) => p.always_slow.includes(a))) throw new Error("an app cannot be both never and always slowed");
      appPolicy = { ...p, v: 1 };
    },
    forgetAppUsage: async () => { usage = { today: [], week: [] }; },
    predict: async () => delay(predictInfo()),
    setTarget: async (until) => {
      refuse();
      if (until !== 0 && until < now() + 600) throw new Error("pick a time at least ten minutes from now");
      target = until === 0 ? null : { until };
    },
    setAdaptiveCharging: async (on) => { refuse(); adaptive = on; },
    trayShown: async () => delay(tray),
    profiles: async () => delay(profileSettings()),
    setProfileOverrides: async (o) => {
      refuse();
      for (const m of Object.values(o.profiles)) for (const v of Object.values(m)) if (!eppChoices.includes(v)) throw new Error(`'${v}' is not offered for 'cpu.epp'`);
      overrides = JSON.parse(JSON.stringify(o));
      const want = overrides.profiles[profile]?.["cpu.epp"] ?? profileDefaults[profile];
      setValue("cpu.epp", want, "profile settings changed in Advanced");
    },
    sensors: async () => delay(sensorsInfo()),
    system: async () => delay(systemInfo()),
    advise: async (format) => {
      if (format !== "tlp" && format !== "tuned") throw new Error("format must be 'tlp' or 'tuned'");
      return delay(format === "tlp"
        ? "# Written by Dynamis Power 0.1.0 from the balanced mode and what it learned on this laptop.\nCPU_ENERGY_PERF_POLICY_ON_AC=balance_performance\nCPU_ENERGY_PERF_POLICY_ON_BAT=balance_performance\nNMI_WATCHDOG=0\n"
        : "# Written by Dynamis Power 0.1.0 from the balanced mode and what it learned on this laptop.\n[main]\ninclude=balanced\n\n[sysctl]\nkernel.nmi_watchdog=0\n");
    },
    setDryRun: async (on) => {
      dryRun = on;
      log({ event: on ? "dry-run-on" : "dry-run-off", id: "", label: "", reason: "" });
    },
    setTray: async (on) => { tray = on; },
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

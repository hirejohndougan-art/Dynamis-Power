// An in-memory stand-in for the service, used in a plain browser (development,
// design review) and in tests. It behaves like the real thing closely enough
// to exercise every screen: changes are journaled, Undo works, safe mode
// refuses changes.
import type { AppPolicy, AppsInfo, Predict, ProfileOverrides, ProfileSettings, Sensors, SystemInfo, AutoPrefs, Backend, Conflict, JournalEntry, Learned, Profile, SettingRow, Status, Unclean, DeviceInfo, DevicePolicy, BrightnessPrefs, RecalInfo, RadioPolicy, SleepTestInfo, LearnSession, Suggestion, SleepRecord, Trouble, TroubleKind, DeepIdle, DesktopInfo, Tip } from "./types";

const now = () => Math.floor(Date.now() / 1000);

/** Things only the demo can do, such as pretending the laptop froze. */
export const demoControls: { freeze?: () => void; outside?: () => void; touch?: () => void } = {};

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
    "system.mem_sleep": "s2idle",
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
  let prefs: AutoPrefs = { on_ac: "balanced", on_battery: "balanced", boost_on_ac: true, low_battery_pct: 30, critical_battery_pct: 15, app_rules: [], power_source: "detect", startup_mode: null };
  let appPolicy: AppPolicy = { v: 1, track: true, slow_background: false, boost_front: false, never_slow: [], always_slow: [] };
  let usage = {
    today: [
      { app: "firefox", cpu_s: 1840, wh: 4.1 },
      { app: "org.kde.dolphin", cpu_s: 120, wh: 0.3 },
      { app: "thunderbird", cpu_s: 410, wh: 0.9 },
      { app: "System", cpu_s: 600, wh: 1.2 },
    ],
    trends: [{ app: "firefox", last_w: 1.4, this_w: 2.0, change_pct: 42.9 }],
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
  const keptSettings = new Map<string, string>();
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
  let gpuEnergy = 200_000_000;
  let lastRead = Date.now() / 1000 - 2;
  let idleUs = [5e9, 2e10, 9e10];
  // The screen: its level now and the level chosen for each mode.
  let brightnessPct = 60;
  let brightLevels: BrightnessPrefs["levels"] = {};
  const setBrightness = (pct: number, reason: string) => {
    if (Math.round(pct) === Math.round(brightnessPct)) return;
    log({ event: "brightness", id: "display.brightness", label: "Screen brightness", from: `${Math.round(brightnessPct)}%`, to: `${Math.round(pct)}%`, reason });
    brightnessPct = pct;
  };
  const screenExtraW = () => (brightnessPct - 60) * 0.031;
  const sensorsInfo = (): Sensors => {
    const t = Date.now() / 1000;
    const dt = Math.max(0.1, t - lastRead);
    lastRead = t;
    energy += dt * (1_250_000 + Math.random() * 400_000);
    gpuEnergy += dt * (220_000 + Math.random() * 120_000);
    idleUs = idleUs.map((v, i) => v + [5e3, 2e5, 1.5e6][i] * (0.8 + Math.random() * 0.4));
    return {
      time: Date.now() / 1000,
      on_ac: false,
      battery_w: Math.round((watts + screenExtraW()) * 100) / 100,
      battery_pct: 79,
      rapl: [
        { id: "intel-rapl:0", name: "package-0", energy_uj: Math.round(energy), max_energy_range_uj: 262143328850 },
        { id: "intel-rapl:0:1", name: "uncore", energy_uj: Math.round(gpuEnergy), max_energy_range_uj: 262143328850 },
      ],
      pressure: { cpu: 1.2, memory: 0, io: 0.4 },
      thermal: [{ name: "thermal_zone0", kind: "x86_pkg_temp", temp_c: 48 + Math.random() * 3 }, { name: "thermal_zone1", kind: "acpitz", temp_c: 41 }],
      cpu_mhz: 1100 + Math.round(Math.random() * 400),
      idle: { cpus: 2, states: ["POLL", "C1", "C10"].map((name, index) => ({ name, index, time_us: Math.round(idleUs[index]) })) },
      brightness_pct: brightnessPct,
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
        { app: "thunderbird", cpu_pct: 1.2, watts: 0.1, share: share("thunderbird"), wakeups_per_s: 240 },
      ],
      today: appPolicy.track ? [...usage.today].sort((a, b) => b.wh - a.wh) : [],
      week: appPolicy.track ? usage.week : [],
      trends: appPolicy.track ? usage.trends : [],
    };
  };

  const log = (e: Omit<JournalEntry, "v" | "time">) => journal.push({ v: 1, time: now(), ...e });
  const refuse = () => {
    if (safeMode) throw new Error(`safe mode is on (${safeMode}); changes are refused, restore still works`);
  };
  const label = (id: string) => rows().find((r) => r.id === id)?.label ?? id;
  const kept = () => records.filter((r) => r.verdict === "keep");

  const blockedIds = new Map<string, string>();
  // "Learn now" (a mockup: the service does not have it yet). Each test
  // lasts STEP_MS here instead of about 9 minutes.
  const STEP_MS = 1500;
  const PLAN: [string, string, string, boolean, LearnSession["results"][number]["verdict"], string, number | null][] = [
    ["kernel.nmi_watchdog", "Watchdog wakeups", "off", false, "keep", "saves 0.39 W (about 6%)", 0.39],
    ["audio.power_save", "Sound chip sleep", "1", false, "keep", "saves 0.12 W (about 2%)", 0.12],
    ["vm.dirty_writeback", "Disk write batching", "1500", false, "unclear", "too small to tell; will retry in everyday use", null],
    ["vm.swappiness", "Swap eagerness", "10", false, "reject", "no useful saving", null],
    ["cpu.turbo", "Turbo boost", "off", true, "reject", "made the laptop less responsive (+4.8 points of lag)", null],
    ["cpu.epp", "CPU speed preference", "balance_power", true, "keep", "saves 0.45 W with no measurable lag", 0.45],
    ["cpu.max_perf_pct", "CPU speed limit", "90", true, "keep", "saves 0.21 W with no measurable lag", 0.21],
    ["storage.sata_alpm", "SATA link power saving", "med_power_with_dipm", false, "keep", "saves 0.08 W (about 1%)", 0.08],
  ];
  let session: LearnSession = { phase: "idle", total: PLAN.length, done: 0, current: null, minutes_left: null, results: [], stopped_reason: null, saves_w: 0 };
  let sessionTimer: ReturnType<typeof setTimeout> | undefined;
  let sessionLoad = true;
  const sessionStep = () => {
    const i = session.done;
    if (i >= PLAN.length) {
      session = { ...session, phase: "done", current: null, minutes_left: 0 };
      sleepChanged();
      return;
    }
    const [setting, lbl, value, load] = PLAN[i];
    const underLoad = load && sessionLoad;
    session = { ...session, phase: "testing", current: { setting, label: lbl, value, under_load: underLoad }, minutes_left: (PLAN.length - i) * 9 };
    sleepChanged();
    sessionTimer = setTimeout(() => {
      const [, , , , verdict, reason, saves] = PLAN[i];
      // Without the workload, speed settings cannot be judged for lag.
      const r = load && !sessionLoad
        ? { verdict: "unclear" as const, reason: "idle only: lag cannot be measured without a workload", saves_w: null }
        : { verdict, reason, saves_w: saves };
      const results = [...session.results, { setting, label: lbl, value, under_load: underLoad, ...r }];
      session = { ...session, done: i + 1, results, saves_w: results.reduce((a, x) => a + (x.verdict === "keep" ? x.saves_w ?? 0 : 0), 0) };
      sessionStep();
    }, STEP_MS);
  };
  demoControls.touch = () => {
    if (session.phase === "waiting_screen" || session.phase === "testing") {
      clearTimeout(sessionTimer);
      session = { ...session, phase: "stopped", current: null, stopped_reason: "you used the laptop" };
      sleepChanged();
    }
  };

  // The sleep test, played out quickly: each "sleep" lasts SLEEP_MS.
  const SLEEP_MS = 1200;
  let sleep: SleepTestInfo = { phase: "idle", current: null, left: [], minutes: 30, results: [], recommended: null, original: null, note: null, offered: ["s2idle", "deep"], active: "s2idle", alarm: true, hibernate: { ready: false, locked_down: false, missing: ["swap", "resume"] } };
  let sleepTimer: ReturnType<typeof setTimeout> | undefined;
  const sleepChanged = () => {
    if (typeof window !== "undefined") window.dispatchEvent(new Event("dynamis:changed"));
  };
  const sleepStep = () => {
    const kind = sleep.left[0];
    if (!kind) {
      values["system.mem_sleep"] = sleep.original ?? "s2idle";
      const good = sleep.results.filter((r) => r.clean && r.wh_per_h != null).sort((a, b) => a.wh_per_h! - b.wh_per_h!);
      sleep = { ...sleep, phase: "done", current: null, recommended: good[0]?.kind ?? null, active: values["system.mem_sleep"] };
      sleepChanged();
      return;
    }
    values["system.mem_sleep"] = kind;
    sleep = { ...sleep, phase: "sleeping", current: kind, left: sleep.left.slice(1), active: kind };
    sleepChanged();
    sleepTimer = setTimeout(() => {
      const wh = kind === "deep" ? 0.36 : 1.14;
      sleep = { ...sleep, results: [...sleep.results, { kind, minutes: sleep.minutes, wh_per_h: wh, pct_per_h: Math.round((wh / 38) * 1000) / 10, clean: true, problems: [], invalid: null }] };
      sleepStep();
    }, SLEEP_MS);
  };
  let radioPolicy: RadioPolicy = { off_on_battery: [], wifi_off_when_wired: false, off_at_startup: [], off_when_docked: [], keyboard_light_off_on_battery: false };
  let learnHeavy = false;
  // The battery drains slowly on battery and fills when "plugged in", so
  // the panel icon's fill moves as in real use.
  let battery = { pct: 79, at: now(), mains: false };
  const batteryPct = () => {
    const t = now();
    const mains = prefs.power_source === "mains";
    const limit = Number(values["battery.charge_limit"]) || 100;
    const dt = t - battery.at;
    const pct = battery.mains ? Math.min(limit, battery.pct + dt / 6) : Math.max(5, battery.pct - dt / 45);
    battery = { pct, at: t, mains };
    return Math.round(pct);
  };
  // "Something stopped working".
  let trouble: Trouble | null = null;
  let desktop: DesktopInfo = {
    policy: { pause_indexing: true, lower_refresh: false, dark_style: false },
    want: { pause_indexing: true, lower_refresh: false, dark_style: false },
    found: { desktop: "kde", indexer: "baloo", refresh_hz: 120, low_hz: 60, can_style: true },
    oled: true,
  };
  let screenPct = 60;
  let tips: Tip[] = [{ id: "video_decode", pct: 42 }];
  let deepIdle: DeepIdle = { running: false, seconds_left: null, at: null, report: null };
  const KINDS: TroubleKind[] = ["wifi", "bluetooth", "sound", "input", "screen", "storage", "sleep", "freeze", "other"];
  const SUSPECTS: Partial<Record<TroubleKind, string[]>> = {
    wifi: ["wifi.power_save", "pcie.aspm"], sound: ["audio.power_save", "pcie.aspm"], screen: ["pcie.aspm"],
    storage: ["vm.dirty_writeback", "pcie.aspm"], sleep: ["system.mem_sleep"], freeze: ["pcie.aspm", "system.mem_sleep"],
  };
  const held = new Set<string>();
  // A habit noticed from modes picked by hand.
  let suggestion: Suggestion | null = { kind: "time", id: "time:21:power-saver", from_hour: 21, to_hour: 23, profile: "power-saver", days: 3 };
  // The last nights asleep.
  const night = (daysAgo: number, from: number, hours: number, before: number, after: number, woke: SleepRecord["woke_by"]): SleepRecord => {
    const d = new Date(); d.setHours(from, 0, 0, 0);
    const start = d.getTime() / 1000 - daysAgo * 86400;
    return { from: start, to: start + hours * 3600, woke_by: woke, battery_before: before, battery_after: after, on_ac: false, pct_per_hour: Math.round(((before - after) / hours) * 10) / 10 };
  };
  const sleepLog: SleepRecord[] = [
    night(0, 0, 7.5, 86, 82, [{ name: "PNP0C0D:00", kind: "lid" }]),
    night(1, 23, 8, 91, 69, [{ name: "0000:00:14.0", kind: "usb" }]),
    night(2, 1, 6, 74, 71, [{ name: "PNP0C0D:00", kind: "lid" }]),
  ];
  // "It feels slow".
  let relief: number | null = null;
  let learningStrict = false;
  // Battery recalibration, played out quickly.
  const RECAL_MS = 4000;
  let recal: RecalInfo = { supported: true, phase: null, battery_pct: 79, floor_pct: 15, min_start_pct: 25, ended: null, full_before_wh: null, full_after_wh: null };
  let recalTimer: ReturnType<typeof setTimeout> | undefined;
  let fullCharge: string | null = null;
  let fullChargeEnded: string | null = null;
  let devicePolicy: DevicePolicy = { usb: false, pci: false, never: [] };
  const deviceList: (Omit<DeviceInfo, "sleeps" | "managed" | "kept_awake"> & { own: boolean })[] = [
    { id: "usb:1-3", kind: "usb", name: "Integrated Camera", what: "usb_device", address: "1-3", model: "usb:8086:a9e3", excluded: null, own: false },
    { id: "usb:1-5", kind: "usb", name: "Fingerprint reader", what: "usb_device", address: "1-5", model: "usb:8086:59f0", excluded: null, own: false },
    { id: "usb:1-2", kind: "usb", name: "Logitech USB Receiver", what: "usb_device", address: "1-2", model: "usb:8086:43c8", excluded: "input", own: false },
    { id: "usb:1-10", kind: "usb", name: "Bluetooth", what: "usb_device", address: "1-10", model: "usb:8086:2c42", excluded: "bluetooth", own: false },
    { id: "usb:usb1", kind: "usb", name: "xHCI Host Controller", what: "usb_device", address: "usb1", model: "usb:8086:a609", excluded: null, own: true },
    { id: "pci:0000:00:02.0", kind: "pci", name: "Intel UHD Graphics 600", what: "graphics", address: "0000:00:02.0", model: "pci:8086:c991", excluded: "graphics", own: false },
    { id: "pci:0000:00:14.0", kind: "pci", name: "Intel Celeron N4020 USB 3.0 xHCI Controller", what: "usb_controller", address: "0000:00:14.0", model: "pci:8086:b98b", excluded: null, own: false },
    { id: "pci:0000:00:16.0", kind: "pci", name: "Intel Trusted Execution Engine", what: "communication", address: "0000:00:16.0", model: "pci:8086:bcbd", excluded: "management", own: false },
    { id: "pci:0000:00:17.0", kind: "pci", name: "Intel SATA AHCI Controller", what: "storage", address: "0000:00:17.0", model: "pci:8086:52e5", excluded: null, own: false },
    { id: "pci:0000:01:00.0", kind: "pci", name: "Realtek RTL8821CE 802.11ac Wi-Fi Adapter", what: "network", address: "0000:01:00.0", model: "pci:8086:08f3", excluded: null, own: true },
  ];
  // Unclean shutdowns, as the service counts them (dynamis-core boots.rs).
  let unclean: Unclean = { last: null, in_safe_mode: 0, suspects: [] };
  let uncleanInARow = 0;
  // Another tool (here TuneD) setting the speed preference back.
  demoControls.outside = () => {
    const id = "cpu.epp";
    if (blockedIds.has(id)) return;
    const ours = values[id];
    const theirs = ours === "performance" ? "balance_power" : "performance";
    values[id] = theirs;
    log({ event: "outside", id, label: label(id), from: ours, to: theirs, reason: "changed outside Dynamis Power, likely by TuneD" });
    const day = now() - 86400;
    if (journal.filter((e) => e.event === "outside" && e.id === id && e.time >= day).length >= 2) {
      blockedIds.set(id, "another tool changes this setting: TuneD");
      log({ event: "block", id, label: label(id), reason: "another tool changes this setting: TuneD" });
    }
  };
  demoControls.freeze = () => {
    const t = now();
    const recent = [...new Set(journal.filter((e) => e.event === "done" && t - e.time < 600 && e.id).map((e) => e.id))]
      .map((id) => ({ id, label: label(id) }));
    const inSafe = !!safeMode;
    unclean = {
      last: { found_at: Math.max(t, (unclean.last?.found_at ?? 0) + 1), ended_at: t - 30, kernel: "nothing", detail: null, in_progress: null, recent: inSafe ? [] : recent, in_safe_mode: inSafe },
      in_safe_mode: unclean.in_safe_mode + (inSafe ? 1 : 0),
      suspects: inSafe ? unclean.suspects : [...unclean.suspects, ...recent.filter((r) => !unclean.suspects.some((x) => x.id === r.id))],
    };
    uncleanInARow += 1;
    if (uncleanInARow >= 3 && !safeMode) {
      safeMode = `${uncleanInARow} unclean shutdowns in a row`;
      log({ event: "safe-mode-on", id: "", label: "", reason: safeMode });
    }
  };

  const rows = (): SettingRow[] => {
    const row = (id: string, lbl: string, risk: SettingRow["risk"], riskLabel: string, allowed: string, available = true): SettingRow => ({
      id, label: lbl, risk, risk_label: riskLabel, available, allowed,
      current: available ? values[id] ?? null : null, original: original[id] ?? null, blocked: blockedIds.get(id) ?? null,
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
      row("system.mem_sleep", "Sleep type", "risky", "Risky", "s2idle | deep"),
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
      profile: relief && relief > now() && profile === "power-saver" ? "balanced" : profile,
      profiles: ["power-saver", "balanced", "performance"],
      holds: [],
      auto: {
        on: auto,
        workload: auto ? "light_interactive" : null,
        reason: !auto ? "Auto mode is off" : pausedByHand ? "You picked a mode by hand; auto mode resumes when the charger is plugged in or out" : "On battery",
      },
      learning: {
        on: learning,
        status: learning ? "Waiting: power use is changing" : "Learning is off",
        tests: records.length,
        tested_settings: new Set(records.filter((r) => r.verdict !== "inconclusive").map((r) => r.setting)).size,
        heavy: learnHeavy,
        activity: "everyday",
        activity_tests: { video: 4, heavy: learnHeavy ? 2 : 0 },
        rechecks: { count: 1, why: "after a system update (kernel 6.8.0-45-generic → 6.11.0-21-generic)" },
        kept: kept().map((r) => ({ setting: r.setting, label: label(r.setting), value: r.value, baseline: r.baseline, saves_w: -r.effect.mean, reason: r.reason })),
        saves_w: kept().reduce((a, r) => a - r.effect.mean, 0),
        waiting: learning ? { reason: "unsteady", share_pct: 62 } : null,
        power_update_s: 15,
        progress: (() => {
          const done = records.length + 1;
          const left = Math.max(0, 12 - done);
          return { pct: Math.floor((done / (done + left)) * 100), done, left, stage: left === 0 ? "done" : done < 8 ? "single" : "together", hours_left: (left * 780) / 3600 };
        })(),
      },
      power: {
        on_ac: prefs.power_source === "mains", has_battery: true, temp_c: Math.round(48 + Math.random() * 3), battery_pct: batteryPct(), charging: prefs.power_source === "mains" && batteryPct() < Number(values["battery.charge_limit"]),
        watts: Math.round((watts + screenExtraW()) * 100) / 100, hours_left: Math.round((31.2 / (watts + screenExtraW())) * 10) / 10, hours_to_full: null,
        health_pct: 90.5, cycles: 212, design_wh: 42, full_wh: 38, charge_limit: Number(values["battery.charge_limit"]),
      },
      safe_mode: safeMode,
      relief: relief && relief > now() ? relief : null,
      suggestion,
      hotter: null,
      trouble,
      deep_idle: deepIdle,
      tips,
      desktop,
      kernel_watch: true,
      learning_strict: learningStrict,
      unclean,
      full_charge: { supported: true, active: fullCharge !== null, restore_to: fullCharge, ended: fullChargeEnded },
      dry_run: dryRun,
      kept: { settings: [...keptSettings].map(([id, value]) => ({ id, label: label(id), value, trusted: true })), held_back: [], charge_limit_ignored: false },
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
      if (target && p !== "power-saver") target = null;
      const before = profile;
      relief = null;
      profile = p;
      const pName = { "power-saver": "Longer battery", balanced: "Balanced", performance: "Faster" }[p];
      if (before !== p && brightLevels[p] != null) setBrightness(brightLevels[p]!, `your brightness for ${pName}`);
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
      keptSettings.clear();
      devicePolicy = { usb: false, pci: false, never: devicePolicy.never ?? [] };
      brightLevels = {};
      fullCharge = null;
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
    setSetting: async (id, value) => {
      refuse();
      setValue(id, value, "set in the app");
      if (id === "battery.charge_limit" && fullCharge !== null) {
        fullCharge = null;
        fullChargeEnded = "you set the limit by hand";
      }
      if (id.startsWith("battery.charge") || keptSettings.has(id)) keptSettings.set(id, value);
    },
    setKept: async (id, keep) => {
      if (keep) keptSettings.set(id, values[id]);
      else keptSettings.delete(id);
    },
    setSafeMode: async (on) => {
      if (!on && safeMode) {
        if (unclean.in_safe_mode === 0) for (const x of unclean.suspects) blockedIds.set(x.id, "changed shortly before freezes that stopped in safe mode");
        unclean = { ...unclean, in_safe_mode: 0, suspects: [] };
        uncleanInARow = 0;
      }
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
    learnSession: async () => delay({ ...session }),
    startLearnSession: async (withWorkload) => {
      refuse();
      if (status().power.on_ac) throw new Error("unplug the charger first: learning measures the battery");
      sessionLoad = withWorkload;
      clearTimeout(sessionTimer);
      session = { ...session, phase: "waiting_screen", done: 0, results: [], current: null, stopped_reason: null, saves_w: 0, minutes_left: PLAN.length * 9 + 5 };
      sessionTimer = setTimeout(sessionStep, STEP_MS);
    },
    stopLearnSession: async () => {
      clearTimeout(sessionTimer);
      if (session.phase === "waiting_screen" || session.phase === "testing") {
        session = { ...session, phase: "stopped", current: null, stopped_reason: "you stopped it" };
      }
    },
    sleepTest: async () => delay({ ...sleep, active: values["system.mem_sleep"] }),
    openFull: async () => {
      window.location.hash = "home";
      window.dispatchEvent(new Event("dynamis:open-full"));
    },
    closeQuick: async () => {
      window.dispatchEvent(new Event("dynamis:close-quick"));
    },
    screenBrightness: async () => delay(screenPct),
    setScreenBrightness: async (pct) => {
      screenPct = Math.max(5, Math.min(100, Math.round(pct)));
    },
    dismissTip: async (id) => {
      refuse();
      tips = tips.filter((x) => x.id !== id);
      return delay(undefined);
    },
    setDesktopPolicy: async (p) => {
      refuse();
      desktop = { ...desktop, policy: { ...p }, want: { ...p } };
      return delay(undefined);
    },
    checkDeepIdle: async () => {
      refuse();
      deepIdle = { ...deepIdle, running: true, seconds_left: 60 };
      // The demo shortens the minute to a moment.
      setTimeout(() => {
        deepIdle = {
          running: false,
          seconds_left: null,
          at: Math.floor(Date.now() / 1000),
          report: {
            v: 1,
            seconds: 60,
            deepest: "C10",
            core_deepest_pct: 62,
            package_pct: 18,
            platform_pct: null,
            held_awake: [{ id: "pci:0000:00:14.0", name: null, what: "usb_controller" }],
            no_link_sleep: [],
            no_deeper_link_sleep: [{ id: "pci:0000:01:00.0", name: "NVMe SSD Controller", what: "storage" }],
            touched: false,
          },
        };
      }, 300);
      return delay(undefined);
    },
    startSleepTest: async (minutes) => {
      refuse();
      if (["sleeping", "settling", "ready"].includes(sleep.phase)) throw new Error("a sleep test is already running");
      sleep = { ...sleep, phase: "ready", minutes, results: [], recommended: null, note: null, original: values["system.mem_sleep"], left: ["s2idle", "deep"] };
      sleepTimer = setTimeout(sleepStep, 50);
    },
    stopSleepTest: async () => {
      clearTimeout(sleepTimer);
      if (sleep.phase !== "idle" && sleep.phase !== "done") {
        values["system.mem_sleep"] = sleep.original ?? values["system.mem_sleep"];
        sleep = { ...sleep, phase: "done", current: null, left: [], note: "you stopped it" };
      }
    },
    diagnostics: async () =>
      delay(
        [
          "Dynamis Power diagnostic report (version 0.1.0 (preview))",
          "",
          "== System ==",
          "Model:          Example Computers Inc. Synthetic Laptop 14 (laptop)",
          "Kernel:         6.14.0-synthetic",
          "",
          "== Service ==",
          `Running:        yes`,
          `Mode:           ${profile}`,
          `Safe mode:      ${safeMode ?? "off"}`,
          "",
          "== Settings ==",
          ...Object.entries(values).map(([id, v]) => `${id.padEnd(24)} ${v.padEnd(22)} ${original[id] !== v ? original[id] : ""}`),
        ].join("\n"),
      ),
    devices: async () =>
      delay({
        policy: devicePolicy,
        blocked: { usb: null, pci: null, links: null },
        devices: deviceList.map(({ own, ...x }) => {
          const kept_awake = !!x.model && (devicePolicy.never ?? []).includes(x.model);
          const managed = devicePolicy[x.kind] && !x.excluded && !own && !kept_awake;
          const link = x.kind === "pci" && x.what === "storage";
          const link_sleep = link ? { on: !!devicePolicy.deeper_link_sleep && !kept_awake, managed: !!devicePolicy.deeper_link_sleep && !kept_awake, excluded: null } : null;
          return { ...x, kept_awake, managed, sleeps: own || managed, link_sleep };
        }),
        link_sleep_capable: deviceList.filter((x) => x.kind === "pci" && x.what === "storage").length,
      }),
    setDevicePolicy: async (p) => {
      refuse();
      for (const k of ["usb", "pci"] as const) {
        if (p[k] === devicePolicy[k]) continue;
        for (const x of deviceList.filter((d) => d.kind === k && !d.excluded && !d.own)) {
          log({ event: "device-power", id: x.id, label: x.name ?? x.address, from: p[k] ? "on" : "auto", to: p[k] ? "auto" : "on", reason: p[k] ? "on battery: let it sleep when idle" : "switched off" });
        }
      }
      if (p.deeper_link_sleep !== devicePolicy.deeper_link_sleep) {
        for (const x of deviceList.filter((d) => d.kind === "pci" && d.what === "storage")) {
          log({ event: "link-sleep", id: x.id, label: x.name ?? x.address, from: p.deeper_link_sleep ? "off" : "on", to: p.deeper_link_sleep ? "on" : "off", reason: p.deeper_link_sleep ? "on battery: deeper link sleep" : "switched off" });
        }
      }
      devicePolicy = p;
    },
    radios: async () => delay({ policy: { ...radioPolicy }, radios: [{ id: "rfkill0", kind: "bluetooth", name: "hci0", off: radioPolicy.off_on_battery.includes("bluetooth"), hard_off: false, managed: radioPolicy.off_on_battery.includes("bluetooth") }, { id: "rfkill1", kind: "wlan", name: "phy0", off: false, hard_off: false, managed: false }], wired: false, keyboard_lights: 1 }),
    setRadioPolicy: async (p) => {
      refuse();
      if (p.off_on_battery.includes("bluetooth") && !radioPolicy.off_on_battery.includes("bluetooth")) log({ event: "radio", id: "rfkill0", label: "Bluetooth", from: "on", to: "off", reason: "on battery" });
      if (p.keyboard_light_off_on_battery && !radioPolicy.keyboard_light_off_on_battery) log({ event: "keyboard-light", id: "/sys/class/leds/asus::kbd_backlight", label: "Keyboard light", from: "2", to: "0", reason: "on battery" });
      radioPolicy = { ...p };
    },
    feelsSlow: async (lasting) => {
      refuse();
      const was = profile;
      const end = new Date(); end.setHours(24, 0, 0, 0);
      relief = end.getTime() / 1000;
      if (profile === "power-saver") setValue("cpu.epp", "balance_performance", "it feels slow: Balanced instead of Longer battery today");
      if (lasting) learningStrict = true;
      return { mode_was: was, mode_now: profile === "power-saver" ? "balanced" : profile, learned: kept().filter((r) => r.setting === "cpu.max_perf_pct").map((r) => r.setting), apps_slowed: appPolicy.slow_background, until: relief, lasting };
    },
    endRelief: async () => { relief = null; },
    answerSuggestion: async (id, yes) => {
      refuse();
      if (!suggestion || suggestion.id !== id) throw new Error("that suggestion is no longer open");
      if (yes) {
        if (suggestion.kind === "time") prefs = { ...prefs, time_rules: [...(prefs.time_rules ?? []), { from_hour: suggestion.from_hour, to_hour: suggestion.to_hour, profile: suggestion.profile }] };
        else prefs = { ...prefs, app_rules: [...(prefs.app_rules ?? []), { app: suggestion.app, profile: suggestion.profile }] };
        auto = true;
      }
      suggestion = null;
    },
    sleeps: async () => delay({ sleeps: sleepLog }),
    setLearnHeavy: async (on) => { refuse(); learnHeavy = on; },
    somethingBroke: async () => {
      const at = now();
      const recent = journal.filter((j) => j.time >= at - 3600 && j.event === "done" && j.id in original);
      const seen = new Set<string>();
      const undone: Trouble["undone"] = [];
      for (const j of [...recent].reverse()) {
        if (seen.has(j.id)) continue;
        seen.add(j.id);
        const to = recent.find((r) => r.id === j.id)?.from ?? original[j.id];
        if (values[j.id] === to) continue;
        values[j.id] = to;
        log({ event: "restore", id: j.id, label: label(j.id), from: j.to, to, reason: "something stopped working; ok" });
        undone.push({ id: j.id, label: label(j.id), to, ok: true });
        held.add(j.id);
      }
      trouble = { at, until: at + 12 * 3600, undone, kinds: KINDS };
      return delay(trouble);
    },
    whatBroke: async (kind) => {
      if (kind !== "none" && !KINDS.includes(kind)) throw new Error(`${kind} is not a kind of trouble`);
      const undone = trouble?.undone.map((u) => u.id) ?? [];
      const suspects = kind === "none" ? [] : kind === "other" ? undone : SUSPECTS[kind] ?? [];
      const blocked = suspects.filter((id) => undone.includes(id) || (id in original && values[id] !== original[id]));
      for (const id of blocked) {
        if (id in original) values[id] = original[id];
        log({ event: "block", id, label: label(id), reason: "you said it stopped working after it was changed" });
      }
      log({ event: "problem", id: kind, label: kind, reason: blocked.length ? `blocked: ${blocked.join(", ")}` : "nothing Dynamis Power changed could cause it" });
      held.clear();
      trouble = null;
      return delay({ kind, blocked: blocked.map((id) => ({ id, label: label(id) })) });
    },
    recalibration: async () => delay({ ...recal, battery_pct: recal.phase === "running_down" ? 41 : recal.phase === "charging" ? 63 : 79 }),
    startRecalibration: async () => {
      refuse();
      if (!(prefs.power_source === "mains")) throw new Error("Plug the charger in first: the battery runs down while plugged in, then charges fully.");
      recal = { ...recal, phase: "running_down", ended: null, full_before_wh: null, full_after_wh: null };
      log({ event: "charge-behaviour", id: "battery.charge_behaviour", label: "Battery", from: "auto", to: "force-discharge", reason: "recalibration: running the battery down" });
      clearTimeout(recalTimer);
      recalTimer = setTimeout(() => {
        recal = { ...recal, phase: "charging" };
        log({ event: "charge-behaviour", id: "battery.charge_behaviour", label: "Battery", from: "force-discharge", to: "auto", reason: "recalibration: charging fully" });
        recalTimer = setTimeout(() => { recal = { ...recal, phase: null, ended: "done", full_before_wh: 38.0, full_after_wh: 36.4 }; }, RECAL_MS);
      }, RECAL_MS);
    },
    stopRecalibration: async () => {
      clearTimeout(recalTimer);
      if (recal.phase) log({ event: "charge-behaviour", id: "battery.charge_behaviour", label: "Battery", from: "force-discharge", to: "auto", reason: "recalibration ended: you stopped it" });
      recal = { ...recal, phase: null, ended: "you stopped it" };
    },
    tlpImport: async () =>
      delay({
        found: true,
        lines: [
          { tlp: 'NMI_WATCHDOG="0"', setting: "kernel.nmi_watchdog", label: "Watchdog wakeups", value: "off", target: "setting", skipped: null, note: null },
          { tlp: 'CPU_ENERGY_PERF_POLICY_ON_BAT="power"', setting: "cpu.epp", label: "CPU speed preference", value: "power", target: "power-saver", skipped: null, note: null },
          { tlp: 'CPU_ENERGY_PERF_POLICY_ON_AC="balance_performance"', setting: "cpu.epp", label: "CPU speed preference", value: "balance_performance", target: "performance", skipped: null, note: null },
          { tlp: 'SOUND_POWER_SAVE_ON_AC="0"  SOUND_POWER_SAVE_ON_BAT="1"', setting: "audio.power_save", label: "Sound card power saving", value: "1", target: "setting", skipped: null, note: "Dynamis Power uses one value for this setting; the battery value is used" },
          { tlp: 'MEM_SLEEP_ON_BAT="deep"', setting: "system.mem_sleep", label: "Sleep type", value: null, target: null, skipped: "risky: set it in Advanced, where it asks for a yes", note: null },
          { tlp: 'USB_AUTOSUSPEND="1"', setting: null, label: null, value: null, target: null, skipped: "use Devices → Let idle USB devices sleep", note: null },
          { tlp: 'DISK_APM_LEVEL_ON_BAT="128 128"', setting: null, label: null, value: null, target: null, skipped: "Dynamis Power has no setting for this yet", note: null },
        ],
      }),
    applyTlpImport: async (lines) => {
      refuse();
      log({ event: "import", id: "tlp", label: "TLP", reason: `${lines.length} values imported from TLP's configuration` });
      if (lines.includes(0)) setValue("kernel.nmi_watchdog", "off", "set in the app");
      return { applied: lines.length, failed: [] };
    },
    brightness: async () => delay({ supported: true, now_pct: brightnessPct, min_pct: 5, levels: { ...brightLevels } }),
    setBrightnessPrefs: async (b) => {
      refuse();
      if (Object.values(b.levels).some((v) => v == null || v < 5 || v > 100)) throw new Error("brightness must be 5–100%");
      const now = b.levels[profile];
      const changed = now !== brightLevels[profile];
      brightLevels = { ...b.levels };
      const pName = { "power-saver": "Longer battery", balanced: "Balanced", performance: "Faster" }[profile];
      if (changed && now != null) setBrightness(now, `you chose it for ${pName}`);
    },
    forgetAppUsage: async () => { usage = { today: [], week: [], trends: [] }; },
    predict: async () => delay(predictInfo()),
    setTarget: async (until) => {
      refuse();
      if (until !== 0 && until < now() + 600) throw new Error("pick a time at least ten minutes from now");
      target = until === 0 ? null : { until };
    },
    setAdaptiveCharging: async (on) => { refuse(); adaptive = on; },
    chargeFullyOnce: async (on) => {
      refuse();
      const lim = "battery.charge_limit";
      if (on && fullCharge === null) {
        if (values[lim] === "100") throw new Error("the battery already charges fully");
        fullCharge = values[lim];
        log({ event: "done", id: lim, label: label(lim), from: values[lim], to: "100", reason: "charge fully once" });
        values[lim] = "100";
        fullChargeEnded = null;
      } else if (!on && fullCharge !== null) {
        log({ event: "done", id: lim, label: label(lim), from: "100", to: fullCharge, reason: "charge fully once ended: you stopped it" });
        values[lim] = fullCharge;
        fullCharge = null;
        fullChargeEnded = "you stopped it";
      }
    },
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
    openIssue: async () => {},
    trayHost: async () => true,
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

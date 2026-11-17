import { describe as describeEntry } from "../activity";
import { build, scrub, summary } from "../exporter";
import { duration, tn } from "../i18n";
import type { JournalEntry } from "../backend";

const e = (over: Partial<JournalEntry>): JournalEntry => ({ v: 1, time: 1000, event: "done", id: "cpu.epp", label: "CPU speed preference", from: "a", to: "b", reason: "", ...over });

describe("activity", () => {
  it("sorts entries into the right filters", () => {
    expect(describeEntry(e({ reason: "auto: On battery" }))?.category).toBe("auto");
    expect(describeEntry(e({ reason: "set in the app" }))?.category).toBe("mine");
    expect(describeEntry(e({ reason: "calibration: step 2 of 8" }))?.category).toBe("tests");
    expect(describeEntry(e({ event: "revert", reason: "calibration health check: GPU hang" }))?.category).toBe("problems");
    expect(describeEntry(e({ event: "recover" }))?.category).toBe("problems");
    expect(describeEntry(e({ event: "quirk" }))?.category).toBe("rollbacks");
    const k = describeEntry(e({ event: "kernel", id: "sound", reason: "azx_get_response timeout; undone and blocked: audio.power_save" }));
    expect(k?.title).toBe("The kernel reported trouble with sound");
    expect(k?.category).toBe("problems");
    expect(describeEntry(e({ event: "problem", id: "wifi", reason: "blocked: wifi.power_save" }))?.category).toBe("problems");
    expect(describeEntry(e({ event: "system-update", id: "system", reason: "kernel 6.8 → 6.11" }))?.title).toBe("The system was updated");
    expect(describeEntry(e({ event: "vanished", id: "pcie.aspm", label: "PCIe link power saving" }))?.title).toBe("PCIe link power saving is no longer offered");
  });
  it("hides technical entries and no-op changes", () => {
    expect(describeEntry(e({ event: "start" }))).toBeNull();
    expect(describeEntry(e({ from: "x", to: "x" }))).toBeNull();
  });
  it("offers Undo only where it means something", () => {
    expect(describeEntry(e({ reason: "set in the app" }))?.undoable).toBe(true);
    expect(describeEntry(e({ event: "keep" }))?.undoable).toBe(true);
    expect(describeEntry(e({ reason: "auto: On battery" }))?.undoable).toBe(false);
    expect(describeEntry(e({ event: "restore" }))?.undoable).toBe(false);
  });
  it("uses plain language", () => {
    expect(describeEntry(e({ reason: "auto: On battery" }))?.title).toBe("CPU speed preference: a → b");
    expect(describeEntry(e({ from: "balance_performance", to: "power", reason: "set in the app" }))?.title).toBe(
      "CPU speed preference: leaning to speed → saving power",
    );
    expect(describeEntry(e({ id: "cpu.max_perf_pct", from: "100", to: "80" }))?.title).toBe("CPU speed preference: 100% → 80%");
    expect(describeEntry(e({ reason: "auto: On battery" }))?.why).toBe("Automatic: On battery");
  });
});

describe("export", () => {
  const data = {
    journal: [e({ time: 100, reason: "old" }), e({ time: 9_000_000, reason: "set in /home/john/x from 192.168.1.20" })],
    learned: null,
    inventory: { machine: { model: "Book", serial: "ABC123" }, cpu: { model: "N4020" }, wifi: [{ mac: "aa:bb:cc:dd:ee:ff" }] },
    settings: [],
    version: "0.1.0",
  };
  it("removes personal details when asked", () => {
    const out = build({ journal: true, results: false, hardware: true, settings: false, diagnostics: false, rangeDays: null, format: "json", privacy: true }, data);
    expect(out).not.toMatch(/john|192\.168|aa:bb|ABC123/);
    expect(out).toContain("[user]");
    const raw = build({ journal: true, results: false, hardware: true, settings: false, diagnostics: false, rangeDays: null, format: "json", privacy: false }, data);
    expect(raw).toContain("ABC123");
  });
  it("respects the time range and the sections chosen", () => {
    const out = build({ journal: true, results: false, hardware: false, settings: false, diagnostics: false, rangeDays: 1, format: "text", privacy: true }, data, 9_000_100);
    expect(out).not.toContain("old");
    expect(out).toContain("== Change journal");
    expect(out).not.toContain("== Hardware");
  });
  it("adds the diagnostic report when chosen", () => {
    const d = { ...data, diagnostics: "== System ==\nModel: Book\nseen in /home/john/x" };
    const out = build({ journal: false, results: false, hardware: false, settings: false, diagnostics: true, rangeDays: null, format: "text", privacy: true }, d);
    expect(out).toContain("== Diagnostic report");
    expect(out).toContain("Model: Book");
    expect(out).toContain("/home/[user]");
  });
  it("makes a short summary", () => {
    expect(summary(data)).toContain("Machine: ? Book");
    expect(scrub("ip 10.0.0.1")).toBe("ip [ip]");
  });
});

describe("strings", () => {
  it("formats durations and plurals", () => {
    expect(duration(4.17)).toBe("4 h 10 min");
    expect(duration(0.5)).toBe("30 min");
    expect(tn("restore_done", 1)).toBe("Restored 1 setting.");
    expect(tn("restore_done", 3)).toBe("Restored 3 settings.");
  });
});

describe("apps", () => {
  it("names apps for people", async () => {
    const { appName } = await import("../apps");
    expect(appName("org.kde.dolphin")).toBe("Dolphin");
    expect(appName("firefox")).toBe("Firefox");
    expect(appName("System")).toBe("System");
  });
  it("keeps each app in at most one list, and rules unique", async () => {
    const { shareOf, withShare, withRule } = await import("../apps");
    const p0 = { track: true, slow_background: false, boost_front: false, never_slow: ["a"], always_slow: [] };
    const p1 = withShare(p0, "a", "always");
    expect(p1.never_slow).toEqual([]);
    expect(shareOf(p1, "a")).toBe("always");
    expect(shareOf(withShare(p1, "a", "normal"), "a")).toBe("normal");
    const r = withRule([{ app: "a", profile: "balanced" }], "a", "performance");
    expect(r).toEqual([{ app: "a", profile: "performance" }]);
    expect(withRule(r, "a", null)).toEqual([]);
  });
  it("shows bars by energy, or by processor time when plugged in", async () => {
    const { bars } = await import("../apps");
    const b = bars([{ app: "x", cpu_s: 10, wh: 1 }, { app: "y", cpu_s: 100, wh: 2 }]);
    expect(b.map((r) => [r.row.app, r.pct, r.byEnergy])).toEqual([["y", 100, true], ["x", 50, true]]);
    expect(bars([{ app: "x", cpu_s: 10, wh: 0 }, { app: "y", cpu_s: 40, wh: 0 }])[1].pct).toBe(25);
  });
  it("describes share changes in Activity", () => {
    const w = (from: string, to: string, reason: string) => describeEntry(e({ event: "app-weight", id: "org.mozilla.firefox", from, to, reason }));
    expect(w("100", "25", "in the background on battery")?.title).toBe("Firefox given a smaller share of the processor");
    expect(w("100", "400", "the app in front gets priority")?.title).toBe("Firefox given priority");
    expect(w("25", "100", "back to a normal share")?.title).toBe("Firefox back to a normal share");
    expect(w("100", "25", "you chose to always slow this app")?.category).toBe("mine");
  });
});

describe("sensors", () => {
  it("turns two readings into processor watts and deep-sleep share", async () => {
    const { rates } = await import("../screens/AdvancedMore");
    const base = { on_ac: false, battery_w: 5, battery_pct: 80, pressure: { cpu: 0, memory: 0, io: 0 }, thermal: [], cpu_mhz: 1000, brightness_pct: null };
    const a = { ...base, time: 100, rapl: [{ id: "r0", name: "package-0", energy_uj: 1_000_000, max_energy_range_uj: 10_000_000 }], idle: { cpus: 2, states: [{ name: "C1", index: 1, time_us: 0 }, { name: "C6", index: 2, time_us: 0 }] } };
    const b = { ...base, time: 102, rapl: [{ id: "r0", name: "package-0", energy_uj: 5_000_000, max_energy_range_uj: 10_000_000 }], idle: { cpus: 2, states: [{ name: "C1", index: 1, time_us: 1e6 }, { name: "C6", index: 2, time_us: 3e6 }] } };
    const r = rates(a, b);
    expect(r.cpu_w).toBeCloseTo(2);
    expect(r.deep_pct).toBeCloseTo(75);
    // The counter wrapped.
    const w = rates(b, { ...b, time: 104, rapl: [{ ...b.rapl[0], energy_uj: 1_000_000 }] });
    expect(w.cpu_w).toBeCloseTo(3);
  });
});

describe("what uses the battery", () => {
  const base = { on_ac: false, battery_w: 6, battery_pct: 80, pressure: { cpu: 0, memory: 0, io: 0 }, thermal: [], cpu_mhz: 1000, brightness_pct: 50, idle: null };
  const counters = (time: number, pkg: number, gpu: number) => ({
    ...base,
    time,
    rapl: [
      { id: "r0", name: "package-0", energy_uj: pkg, max_energy_range_uj: 1e12 },
      { id: "r1", name: "uncore", energy_uj: gpu, max_energy_range_uj: 1e12 },
    ],
  });
  const display = { w_per_pct: 0.03, base_w: 3, r2: 0.9, samples: 100 };

  it("splits the total between screen, processor, graphics and the rest", async () => {
    const { powerSplit } = await import("../power");
    const s = powerSplit(counters(10, 0, 0), counters(12, 4e6, 1e6), display);
    const w = Object.fromEntries(s.parts.map((p) => [p.key, p.w]));
    expect(s.total).toBe(6);
    expect(w.screen).toBeCloseTo(1.5);
    expect(w.graphics).toBeCloseTo(0.5);
    expect(w.processor).toBeCloseTo(1.5); // 2 W for the chip, less graphics
    expect(w.rest).toBeCloseTo(2.5); // 6 - 2 - 1.5
    expect(s.parts[0].key).toBe("rest"); // biggest first
    // Plugged in: no total, no "everything else".
    const p = powerSplit(counters(10, 0, 0), { ...counters(12, 4e6, 1e6), on_ac: true }, null);
    expect(p.total).toBeNull();
    expect(p.parts.map((x) => x.key).sort()).toEqual(["graphics", "processor"]);
  });

  it("estimates battery minutes for another brightness", async () => {
    const { brightnessMinutes } = await import("../power");
    const power = { on_ac: false, has_battery: true, battery_pct: 80, charging: false, watts: 6, hours_left: 5, hours_to_full: null, health_pct: null, cycles: null, design_wh: null, full_wh: null, charge_limit: null };
    // 30 Wh left. Dimming 50% → 30% saves 0.6 W: 30/5.4 h instead of 5 h.
    expect(brightnessMinutes(30, 50, display, power)).toBeCloseTo((30 / 5.4 - 5) * 60);
    expect(brightnessMinutes(70, 50, display, power)!).toBeLessThan(0);
    expect(brightnessMinutes(30, 50, null, power)).toBeNull();
    expect(brightnessMinutes(30, 50, display, { ...power, on_ac: true })).toBeNull();
  });
});

describe("activity grouping", () => {
  it("folds three or more automatic changes in one hour into one line", async () => {
    const { groupAuto } = await import("../activity");
    const item = (time: number, reason: string) => describeEntry(e({ time, reason }))!;
    const items = [
      item(7200 + 10, "auto: On battery"),
      item(7200 + 20, "auto: Video"),
      item(7200 + 30, "auto: On battery"),
      item(7200 + 40, "set in the app"),
      item(3600 + 10, "auto: Busy"),
      item(3600 + 20, "auto: On battery"),
    ];
    const g = groupAuto(items);
    expect(g).toHaveLength(4);
    expect(g[0].group).toHaveLength(3);
    expect(g[0].title).toMatch(/3 times/);
    expect(g[1].category).toBe("mine");
    expect(g[2].group).toBeUndefined();
  });
});

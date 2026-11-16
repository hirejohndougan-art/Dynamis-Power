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
    const out = build({ journal: true, results: false, hardware: true, settings: false, rangeDays: null, format: "json", privacy: true }, data);
    expect(out).not.toMatch(/john|192\.168|aa:bb|ABC123/);
    expect(out).toContain("[user]");
    const raw = build({ journal: true, results: false, hardware: true, settings: false, rangeDays: null, format: "json", privacy: false }, data);
    expect(raw).toContain("ABC123");
  });
  it("respects the time range and the sections chosen", () => {
    const out = build({ journal: true, results: false, hardware: false, settings: false, rangeDays: 1, format: "text", privacy: true }, data, 9_000_100);
    expect(out).not.toContain("old");
    expect(out).toContain("== Change journal");
    expect(out).not.toContain("== Hardware");
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

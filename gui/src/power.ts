// Where the power goes: splitting the laptop's power use between the
// screen, the processor, graphics and everything else, from two sensor
// readings and the learned screen model.
import type { Sensors, Status } from "./backend";

export type UsePart = { key: "screen" | "processor" | "graphics" | "rest"; w: number };
export type PowerSplit = {
  parts: UsePart[];
  /** Whole-laptop power, measured only on battery. */
  total: number | null;
};

/** Watts from one energy counter between two readings, null if missing. */
function counterW(a: Sensors, b: Sensors, pick: (name: string) => boolean): number | null {
  const dt = b.time - a.time;
  if (dt <= 0) return null;
  const da = a.rapl.find((d) => pick(d.name));
  const db = b.rapl.find((d) => d.id === da?.id);
  if (da?.energy_uj == null || db?.energy_uj == null) return null;
  let de = db.energy_uj - da.energy_uj;
  if (de < 0 && db.max_energy_range_uj) de += db.max_energy_range_uj;
  return de < 0 ? null : de / 1e6 / dt;
}

/**
 * The split. The processor counter covers the whole chip, graphics
 * included (on Intel, graphics is the "uncore" part), so graphics is taken
 * out of it. The screen comes from the learned model at the brightness now.
 * "Everything else" is what is left of the measured total, on battery only.
 */
export function powerSplit(a: Sensors, b: Sensors, display: Status["display"]): PowerSplit {
  const pkg = counterW(a, b, (n) => n.startsWith("package"));
  const gpu = counterW(a, b, (n) => n === "uncore");
  const screen = display && b.brightness_pct != null ? Math.max(0, display.w_per_pct * b.brightness_pct) : null;
  const total = !b.on_ac && b.battery_w != null && b.battery_w > 0 ? b.battery_w : null;
  const parts: UsePart[] = [];
  if (screen != null) parts.push({ key: "screen", w: screen });
  if (pkg != null) parts.push({ key: "processor", w: Math.max(0, pkg - (gpu ?? 0)) });
  if (gpu != null) parts.push({ key: "graphics", w: gpu });
  if (total != null) parts.push({ key: "rest", w: Math.max(0, total - (pkg ?? 0) - (screen ?? 0)) });
  parts.sort((x, y) => y.w - x.w);
  return { parts, total };
}

/**
 * Battery minutes gained (positive) or lost at brightness `pct` instead of
 * the brightness now, from the screen model. Null when it cannot be told
 * (plugged in, no model, nothing measured).
 */
export function brightnessMinutes(pct: number, nowPct: number | null, display: Status["display"], power: Status["power"]): number | null {
  if (!display || nowPct == null || power.on_ac || !power.watts || !power.hours_left) return null;
  const energy = power.watts * power.hours_left;
  const w = power.watts + display.w_per_pct * (pct - nowPct);
  if (w <= 0) return null;
  return (energy / w - power.hours_left) * 60;
}

// The translation layer. Use t("key", { name: value }) for every string a
// person reads. tn("key", n, vars) picks key_one or key_other.
import { en, type Key } from "./en";
export type { Key };

type Catalog = { [K in Key]?: string };
const catalogs: Record<string, Catalog> = { en };

const lang = (typeof navigator !== "undefined" ? navigator.language : "en").split("-")[0];
const active: Catalog = catalogs[lang] ?? en;

export function t(key: Key, vars: Record<string, string | number> = {}): string {
  const text = active[key] ?? en[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, name) => (name in vars ? String(vars[name]) : `{${name}}`));
}

type PluralBase<K> = K extends `${infer B}_one` ? B : never;

export function tn(base: PluralBase<Key>, n: number, vars: Record<string, string | number> = {}): string {
  const rule = new Intl.PluralRules(lang).select(n) === "one" ? "one" : "other";
  return t(`${base}_${rule}` as Key, { n, ...vars });
}

/** "4 h 10 min", "40 min". */
export function duration(hours: number): string {
  const total = Math.max(1, Math.round(hours * 60));
  const h = Math.floor(total / 60);
  const m = total % 60;
  return h > 0 ? t("time_h_m", { h, m }) : t("time_m", { m });
}

export function profileName(p: string): string {
  return t(`profile_${p.replace("-", "_")}` as Key);
}

/** A clock time, with the day when it is not today: "5:00 PM", "Tue 8:10 AM". */
export function clock(unix: number): string {
  const d = new Date(unix * 1000);
  const sameDay = d.toDateString() === new Date().toDateString();
  return d.toLocaleString(undefined, sameDay ? { timeStyle: "short" } : { weekday: "short", hour: "numeric", minute: "2-digit" });
}

/** An hour of the day in the person's clock style: "9 PM" or "21". */
export function hourName(h: number): string {
  return new Date(2000, 0, 1, h).toLocaleTimeString(undefined, { hour: "numeric" });
}

export function formatTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** A setting value in plain language ("saving power" rather than "power"),
 * falling back to the value itself. Technical values stay available behind
 * the Details switch in Advanced. */
export function valueName(setting: string, value: string | null | undefined): string {
  if (value == null || value === "") return "—";
  const key = `value_${setting.replace(/\./g, "_")}_${value.replace(/[^a-z0-9]/gi, "_")}` as Key;
  if (key in en) return t(key);
  if (value === "on" || value === "off") return t(`value_${value}` as Key);
  if (/^\d+$/.test(value)) {
    if (setting.endsWith("_pct") || setting === "battery.charge_limit") return t("value_percent", { v: value });
    if (setting === "audio.power_save") return t("value_seconds", { v: value });
  }
  return value;
}

// Plain-language helpers for the Apps screen.
import type { AppPolicy, AppRule, AppUse, Profile } from "./backend";
import { t } from "./i18n";

/** "org.kde.dolphin" → "Dolphin", "firefox" → "Firefox". App IDs stay
 * available behind the Details switch in Advanced; people see names. */
export function appName(id: string): string {
  if (id === "System") return t("apps_system");
  const last = id.includes(".") ? id.split(".").pop() || id : id;
  const words = last.replace(/[-_]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export type Share = "normal" | "never" | "always";

export function shareOf(policy: AppPolicy, app: string): Share {
  if (policy.never_slow.includes(app)) return "never";
  if (policy.always_slow.includes(app)) return "always";
  return "normal";
}

/** The policy with one app's share changed (each app is in at most one list). */
export function withShare(policy: AppPolicy, app: string, share: Share): AppPolicy {
  const never = policy.never_slow.filter((a) => a !== app);
  const always = policy.always_slow.filter((a) => a !== app);
  if (share === "never") never.push(app);
  if (share === "always") always.push(app);
  return { ...policy, never_slow: never, always_slow: always };
}

/** The rules with one app's mode set (or removed with `null`). */
export function withRule(rules: AppRule[], app: string, profile: Profile | null): AppRule[] {
  const rest = rules.filter((r) => r.app !== app);
  return profile ? [...rest, { app, profile }] : rest;
}

/** Bar length, 0–100, relative to the biggest user. Energy when there is any,
 * otherwise processor time (plugged in, there are no watts to share out). */
export function bars(rows: AppUse[]): { row: AppUse; pct: number; byEnergy: boolean }[] {
  const byEnergy = rows.some((r) => r.wh > 0.005);
  const value = (r: AppUse) => (byEnergy ? r.wh : r.cpu_s);
  const top = Math.max(0, ...rows.map(value));
  return [...rows]
    .sort((a, b) => value(b) - value(a))
    .map((row) => ({ row, pct: top > 0 ? Math.round((value(row) / top) * 100) : 0, byEnergy }));
}

// Shapes of what the Dynamis Power service sends (see daemon/crates/dynamisd/src/api.rs).

export type Profile = "power-saver" | "balanced" | "performance";

export interface Status {
  version: string;
  profile: Profile;
  profiles: Profile[];
  holds: { profile: Profile; app: string; reason: string }[];
  auto: { on: boolean; workload: string | null; reason: string };
  learning: {
    on: boolean;
    status: string;
    tests: number;
    tested_settings: number;
    kept: { setting: string; label: string; value: string; baseline: string; saves_w: number; reason: string }[];
    saves_w: number;
  };
  power: {
    on_ac: boolean;
    has_battery: boolean;
    battery_pct: number | null;
    charging: boolean;
    watts: number | null;
    hours_left: number | null;
    hours_to_full: number | null;
    health_pct: number | null;
    cycles: number | null;
    design_wh: number | null;
    full_wh: number | null;
    charge_limit: number | null;
  };
  safe_mode: string | null;
  pending: { id: string; label: string; to: string; trial: boolean } | null;
  display: { w_per_pct: number; base_w: number; r2: number; samples: number } | null;
}

export interface JournalEntry {
  v: number;
  time: number;
  event: string;
  id: string;
  label: string;
  from?: string;
  to?: string;
  reason?: string;
}

export interface SettingRow {
  id: string;
  label: string;
  risk: "safe" | "usually_safe" | "risky" | "experimental" | "advisory";
  risk_label: string;
  available: boolean;
  allowed: string;
  current: string | null;
  original: string | null;
  blocked: string | null;
  not_allowed: [string[], string][];
}

export interface Record {
  setting: string;
  value: string;
  baseline: string;
  verdict: "keep" | "reject" | "inconclusive";
  reason: string;
  effect: { mean: number; ci_low: number; ci_high: number; confidence: number };
  stall: { mean: number };
  time: number;
}

export interface Learned {
  records: Record[];
  learned: Record[];
  display: Status["display"];
}

/** Everything the app can ask of the service. */
export interface Backend {
  status(): Promise<Status>;
  journal(n: number): Promise<JournalEntry[]>;
  settings(): Promise<SettingRow[]>;
  inventory(): Promise<unknown>;
  learned(): Promise<Learned>;
  setProfile(p: Profile): Promise<void>;
  setAuto(on: boolean): Promise<void>;
  setLearning(on: boolean): Promise<void>;
  undo(setting: string): Promise<string>;
  restore(): Promise<{ id: string; label: string; value: string; ok: boolean }[]>;
  setSetting(setting: string, value: string, allowRisky: boolean): Promise<void>;
  setSafeMode(on: boolean): Promise<string>;
  forget(setting: string): Promise<boolean>;
  /** Save an exported file; returns where it went. */
  saveFile(name: string, contents: string): Promise<string>;
}

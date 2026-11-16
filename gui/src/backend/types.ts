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
    /** How far learning has got. Null if it could not be worked out. */
    progress?: LearningProgress | null;
    /** The main reason it waited over about the last day, and its share. */
    waiting?: { reason: WaitReason; share_pct: number } | null;
    /** How often the battery's power reading changes, in seconds. */
    power_update_s?: number | null;
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
  /** Settings set by hand that are applied again after restarts and sleep. */
  kept?: {
    settings: { id: string; label: string; value: string; trusted: boolean }[];
    /** Not applied again after an unclean shutdown (set too recently). */
    held_back: string[];
    /** The battery charged past its limit: the firmware ignores it. */
    charge_limit_ignored: boolean;
  };
  /** Modes this laptop cannot offer, and why (plain language). */
  missing_modes?: { profile: Profile; why: string }[];
  /** Developer: changes are journaled as "would" and not made. */
  dry_run?: boolean;
  pending: { id: string; label: string; to: string; trial: boolean } | null;
  display: { w_per_pct: number; base_w: number; r2: number; samples: number } | null;
  /** "Last until", when set. Times are Unix seconds. */
  last_until?: { until: number; level: Level; reason: string | null; on_track: boolean } | null;
}

export interface LearningProgress {
  /** Whole percent; 100 only when every test is done. */
  pct: number;
  done: number;
  left: number;
  stage: "single" | "together" | "done";
  /** Quiet time on battery the tests left need, roughly. */
  hours_left: number;
}

/** How hard "Last until" is saving. */
export type Level = "normal" | "no_boost" | "save" | "save_more";

export interface Predict {
  /** When the battery runs out at your usual use (Unix seconds). */
  forecast_end: number | null;
  typical_w: number | null;
  /** When you usually unplug next. */
  usual_unplug: number | null;
  unplugs_seen: number;
  target: {
    until: number;
    level: Level;
    budget_w?: number | null;
    recent_w?: number | null;
    expected_end?: number | null;
    on_track?: boolean;
    reason?: string;
  } | null;
  target_ended: string | null;
  adaptive_charging: boolean;
  charge: { limit_pct: number; until: number | null; reason: string } | null;
  charge_supported: boolean;
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
  /** Phase 5: the other processor settings (and the power mode's) while
   * this one was tested together with them. */
  group?: { [setting: string]: string };
}

export interface Learned {
  records: Record[];
  learned: Record[];
  display: Status["display"];
}

export interface Conflict {
  unit: string;
  name: string;
  active: boolean;
  state: string;
}

export interface AppRule {
  app: string;
  profile: Profile;
}

export interface AutoPrefs {
  on_ac: Profile;
  on_battery: Profile;
  boost_on_ac: boolean;
  low_battery_pct: number;
  critical_battery_pct: number;
  app_rules?: AppRule[];
  [other: string]: unknown;
}

export interface AppPolicy {
  v?: number;
  /** Count energy per app. */
  track: boolean;
  /** On battery, give apps not in front a smaller share of the processor. */
  slow_background: boolean;
  /** Give the app in front a bigger share. */
  boost_front: boolean;
  never_slow: string[];
  always_slow: string[];
}

export interface AppUse {
  app: string;
  cpu_s: number;
  /** Estimated watt-hours on battery. */
  wh: number;
}

export interface AppsInfo {
  policy: AppPolicy;
  rules: AppRule[];
  front: string | null;
  running: string[];
  now: { app: string; cpu_pct: number; watts: number | null; share: number | null }[];
  today: AppUse[];
  week: AppUse[];
}

export interface ProfileSettings {
  profiles: {
    profile: Profile;
    settings: { id: string; label: string; choices: string[]; value: string | null; default: string | null }[];
  }[];
  overrides: ProfileOverrides;
}

export interface ProfileOverrides {
  v: number;
  profiles: { [profile: string]: { [setting: string]: string } };
}

export interface Sensors {
  time: number;
  on_ac: boolean;
  battery_w: number | null;
  battery_pct: number | null;
  rapl: { id: string; name: string; energy_uj: number | null; max_energy_range_uj: number | null }[];
  pressure: { cpu: number | null; memory: number | null; io: number | null };
  thermal: { name: string; kind: string; temp_c: number }[];
  cpu_mhz: number | null;
  idle: { cpus: number; states: { name: string; index: number; time_us: number }[] } | null;
  brightness_pct: number | null;
}

export interface SystemInfo {
  version: string;
  kernel: string | null;
  os: string | null;
  uptime_s: number;
  cpu_s: number | null;
  cpu_pct: number | null;
  memory_mb: number | null;
  state_dir: string;
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
  /** Keep a setting's current value after restarts and sleep, or stop. */
  setKept(setting: string, keep: boolean): Promise<void>;
  forget(setting: string): Promise<boolean>;
  conflicts(): Promise<Conflict[]>;
  switchOffConflict(unit: string): Promise<void>;
  autoPrefs(): Promise<AutoPrefs>;
  setAutoPrefs(p: AutoPrefs): Promise<void>;
  apps(): Promise<AppsInfo>;
  setAppPolicy(p: AppPolicy): Promise<void>;
  forgetAppUsage(): Promise<void>;
  predict(): Promise<Predict>;
  /** Unix seconds; 0 ends "Last until". */
  setTarget(until: number): Promise<void>;
  setAdaptiveCharging(on: boolean): Promise<void>;
  /** The panel icon (shown by the session helper). */
  trayShown(): Promise<boolean>;
  setTray(on: boolean): Promise<void>;
  profiles(): Promise<ProfileSettings>;
  setProfileOverrides(o: ProfileOverrides): Promise<void>;
  sensors(): Promise<Sensors>;
  system(): Promise<SystemInfo>;
  /** "tlp" or "tuned": text only, nothing is written. */
  advise(format: string): Promise<string>;
  setDryRun(on: boolean): Promise<void>;
  /** Save an exported file; returns where it went. */
  saveFile(name: string, contents: string): Promise<string>;
}

export type WaitReason =
  | "plugged_in"
  | "low_battery"
  | "power_coming"
  | "no_power"
  | "warm"
  | "busy"
  | "app_hold"
  | "safe_mode"
  | "trial"
  | "dry_run";

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
    /** The hottest temperature sensor, °C. */
    temp_c?: number | null;
  };
  safe_mode: string | null;
  /** The last unclean shutdown (freeze, crash, power cut) and safe mode as a test. */
  unclean?: Unclean;
  /** "Charge fully once". */
  full_charge?: { supported: boolean; active: boolean; restore_to: string | null; ended: string | null };
  /** A rule offered from the modes picked by hand; nothing changes before a yes. */
  suggestion?: Suggestion | null;
  /** The laptop runs hotter at light use than it used to (dust, dried paste). */
  hotter?: { by_c: number; since: string } | null;
  /** "It feels slow" is in force until this time. */
  relief?: number | null;
  learning_strict?: boolean;
  /** Parts of the system the app needs that are missing. */
  missing_parts?: ("systemd" | "polkit")[];
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
  /** Another tool that is running manages this setting, so it is left alone. */
  managed_by?: string | null;
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

export type Suggestion =
  | { kind: "app"; id: string; app: string; profile: Profile; count: number }
  | { kind: "time"; id: string; from_hour: number; to_hour: number; profile: Profile; days: number };

/** A mode for part of the day (hours on a 24-hour clock; `to` may be past midnight). */
export interface TimeRule {
  from_hour: number;
  to_hour: number;
  profile: Profile;
}

/** One sleep: what woke the laptop and the battery it used. Times are Unix seconds. */
export interface SleepRecord {
  from: number;
  to: number;
  woke_by: { name: string; kind: WakeKind }[];
  battery_before: number | null;
  battery_after: number | null;
  on_ac: boolean;
  pct_per_hour: number | null;
}

export type WakeKind = "lid" | "power_button" | "charger" | "battery" | "clock" | "usb" | "network" | "keyboard" | "other";

export interface AutoPrefs {
  on_ac: Profile;
  on_battery: Profile;
  boost_on_ac: boolean;
  low_battery_pct: number;
  critical_battery_pct: number;
  app_rules?: AppRule[];
  /** A mode for part of the day. */
  time_rules?: TimeRule[];
  /** What to believe about the charger (for chargers that report wrongly). */
  power_source?: "detect" | "mains" | "battery";
  /** The mode to start with; null is the mode last used. */
  startup_mode?: Profile | null;
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
  /** Apps whose average power while running on battery changed by a quarter or more since last month. */
  trends?: { app: string; last_w: number; this_w: number; change_pct: number }[];
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
  /** Radios and the rules that switch them off. */
  radios(): Promise<RadiosInfo>;
  setRadioPolicy(p: RadioPolicy): Promise<void>;
  /** "It feels slow": undo what costs speed for the rest of the day. */
  feelsSlow(lasting: boolean): Promise<FeelsSlowResult>;
  endRelief(): Promise<void>;
  /** Yes or no to `Status.suggestion`. Yes adds the rule and switches automatic choice on. */
  answerSuggestion(id: string, yes: boolean): Promise<void>;
  /** The last sleeps, newest first. */
  sleeps(): Promise<{ sleeps: SleepRecord[] }>;
  /** Battery recalibration: run down to 15% while plugged in, then charge fully. */
  recalibration(): Promise<RecalInfo>;
  startRecalibration(): Promise<void>;
  stopRecalibration(): Promise<void>;
  /** What importing TLP's configuration would change, line by line. */
  tlpImport(): Promise<TlpImport>;
  /** Apply the preview's lines at these positions. */
  applyTlpImport(lines: number[]): Promise<{ applied: number; failed: { tlp: string; error: string }[] }>;
  /** "Learn now": a session that runs every test back to back while the
   * laptop is left alone. */
  learnSession(): Promise<LearnSession>;
  startLearnSession(withWorkload: boolean): Promise<void>;
  stopLearnSession(): Promise<void>;
  /** The guided sleep test. */
  sleepTest(): Promise<SleepTestInfo>;
  startSleepTest(minutes: number): Promise<void>;
  stopSleepTest(): Promise<void>;
  /** The full diagnostic report in plain text (`dynamis stat`). */
  diagnostics(): Promise<string>;
  /** Devices that can sleep when idle (USB, internal), and the two switches. */
  devices(): Promise<DevicesInfo>;
  setDevicePolicy(p: DevicePolicy): Promise<void>;
  /** The screen brightness chosen for each mode. */
  brightness(): Promise<BrightnessInfo>;
  setBrightnessPrefs(p: BrightnessPrefs): Promise<void>;
  predict(): Promise<Predict>;
  /** Unix seconds; 0 ends "Last until". */
  setTarget(until: number): Promise<void>;
  setAdaptiveCharging(on: boolean): Promise<void>;
  /** Lift the charge limit until the battery is full (or stop). */
  chargeFullyOnce(on: boolean): Promise<void>;
  /** The panel icon (shown by the session helper). */
  trayShown(): Promise<boolean>;
  setTray(on: boolean): Promise<void>;
  /** Whether the desktop shows panel icons at all (GNOME needs an extension). */
  trayHost(): Promise<boolean>;
  profiles(): Promise<ProfileSettings>;
  setProfileOverrides(o: ProfileOverrides): Promise<void>;
  sensors(): Promise<Sensors>;
  system(): Promise<SystemInfo>;
  /** "tlp" or "tuned": text only, nothing is written. */
  advise(format: string): Promise<string>;
  setDryRun(on: boolean): Promise<void>;
  /** Open the project's new-issue page, filled in, in the browser. */
  openIssue(title: string, body: string): Promise<void>;
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
  | "unsteady"
  | "app_hold"
  | "safe_mode"
  | "trial"
  | "dry_run";

export type Labelled = { id: string; label: string };
export type Unclean = {
  last: {
    found_at: number;
    ended_at: number | null;
    kernel: "sleeping" | "waking" | "driver" | "nothing" | "unknown";
    detail: string | null;
    in_progress: Labelled | null;
    recent: Labelled[];
    in_safe_mode: boolean;
  } | null;
  /** Unclean shutdowns while safe mode was on, since it was last off. */
  in_safe_mode: number;
  /** Settings changed shortly before the freezes that led to safe mode. */
  suspects: Labelled[];
};

export type RadioKind = "bluetooth" | "wlan" | "wwan";
export type RadioPolicy = { off_on_battery: RadioKind[]; wifi_off_when_wired: boolean; off_at_startup: RadioKind[]; off_when_docked?: RadioKind[] };
export type RadiosInfo = {
  policy: RadioPolicy;
  radios: { id: string; kind: RadioKind; name: string; off: boolean; hard_off: boolean; managed: boolean }[];
  /** A network cable is plugged in now. */
  wired: boolean;
  /** The laptop is in a dock now. */
  docked?: boolean;
};

export type FeelsSlowResult = {
  mode_was: Profile;
  mode_now: Profile;
  /** Learned speed-related settings put back to the mode's own values. */
  learned: string[];
  apps_slowed: boolean;
  until: number;
  lasting: boolean;
};

export type RecalInfo = {
  supported: boolean;
  phase: "running_down" | "charging" | null;
  battery_pct: number | null;
  floor_pct: number;
  min_start_pct: number;
  /** How it last ended: "done" or why it stopped. */
  ended: string | null;
  full_before_wh: number | null;
  full_after_wh: number | null;
};

export type TlpLine = {
  /** The TLP line(s) it comes from. */
  tlp: string;
  setting: string | null;
  label: string | null;
  value: string | null;
  /** Where it goes: a setting kept after restarts, or one mode's choice. */
  target: "setting" | "power-saver" | "performance" | null;
  /** Why it is not imported. */
  skipped: string | null;
  note: string | null;
};
export type TlpImport = { found: boolean; lines: TlpLine[] };

export type BrightnessPrefs = {
  /** Percent for each mode; a mode left out is left alone. */
  levels: { [P in Profile]?: number };
};
export type BrightnessInfo = BrightnessPrefs & {
  /** Whether this screen's brightness can be set. */
  supported: boolean;
  now_pct: number | null;
  min_pct: number;
};

export type DevicePolicy = {
  usb: boolean;
  pci: boolean;
  /** Device models the person keeps awake (`usb:046d:c52b`). */
  never?: string[];
};
export type DeviceInfo = {
  id: string;
  kind: "usb" | "pci";
  /** The device's own name, if it gives one. */
  name: string | null;
  /** What kind of device (translated as dev_what_*). */
  what: string;
  /** Its address on the bus. */
  address: string;
  /** Which model it is, the same in any port; null if it does not say. */
  model: string | null;
  /** The person keeps this model awake. */
  kept_awake: boolean;
  /** The device may sleep when idle now. */
  sleeps: boolean;
  /** Dynamis Power changed it (and will put it back). */
  managed: boolean;
  /** Why it is always kept awake, if so. */
  excluded: string | null;
};
export type DevicesInfo = {
  policy: DevicePolicy;
  /** Why a kind is blocked on this laptop, if it is. */
  blocked: { usb: string | null; pci: string | null };
  devices: DeviceInfo[];
};

export type SleepTypeResult = {
  kind: string;
  minutes: number;
  wh_per_h: number | null;
  pct_per_h: number | null;
  clean: boolean;
  problems: string[];
  invalid: string | null;
};
export type SleepTestInfo = {
  phase: "idle" | "ready" | "sleeping" | "settling" | "done";
  current: string | null;
  left: string[];
  minutes: number;
  results: SleepTypeResult[];
  recommended: string | null;
  original: string | null;
  note: string | null;
  offered: string[];
  active: string | null;
  /** The laptop can wake itself (it has a wake-up alarm). */
  alarm: boolean;
  /** Whether "sleep, then hibernate" could be set up here (advice only). */
  hibernate?: { ready: boolean; missing: ("kernel" | "lockdown" | "swap" | "resume")[]; locked_down: boolean };
};

export type LearnSessionResult = {
  setting: string;
  label: string;
  value: string;
  verdict: "keep" | "reject" | "unclear";
  reason: string;
  saves_w: number | null;
  under_load: boolean;
};
export type LearnSession = {
  phase: "idle" | "waiting_screen" | "testing" | "done" | "stopped";
  total: number;
  done: number;
  current: { setting: string; label: string; value: string; under_load: boolean } | null;
  minutes_left: number | null;
  results: LearnSessionResult[];
  stopped_reason: string | null;
  saves_w: number;
  /** The session helper is not running, so the screen turning off cannot be seen. */
  agent_missing?: boolean;
  with_workload?: boolean;
};

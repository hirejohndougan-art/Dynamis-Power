// Advanced: Profiles, Sensors, System and Developer tabs.
import { useEffect, useRef, useState } from "react";
import { backend, type ProfileOverrides, type Sensors, type Status } from "../backend";
import { Choice, Sparkline, Switch } from "../components";
import { useLoad } from "../hooks";
import { duration, profileName, t, valueName } from "../i18n";
import { RestoreDialog } from "./Home";
import { Conflicts } from "./Welcome";

type Run = (a: () => Promise<string | void>) => void;

// --- Profiles -------------------------------------------------------------

export function ProfilesTab(props: { locked: boolean; run: Run }) {
  const data = useLoad(() => backend.profiles(), []);
  const [draft, setDraft] = useState<ProfileOverrides | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (data.data) setDraft(data.data.overrides);
  }, [data.data]);
  if (!data.data || !draft) return <p className="muted">{data.error ?? t("loading")}</p>;

  const set = (profile: string, id: string, value: string, isDefault: boolean) => {
    const profiles = { ...draft.profiles, [profile]: { ...(draft.profiles[profile] ?? {}) } };
    if (isDefault) delete profiles[profile][id];
    else profiles[profile][id] = value;
    setDraft({ ...draft, profiles });
  };
  const save = (o: ProfileOverrides) =>
    props.run(async () => {
      await backend.setProfileOverrides(o);
      data.reload();
      return t("profiles_saved");
    });

  return (
    <div className="stack">
      <p className="muted">{t("profiles_intro")}</p>
      {data.data.profiles.map((p) => (
        <fieldset key={p.profile} className="field">
          <legend>{profileName(p.profile)}</legend>
          {p.settings.length === 0 && <p className="muted small">{t("profiles_nothing")}</p>}
          {p.settings.map((s) => {
            const value = draft.profiles[p.profile]?.[s.id] ?? s.default ?? "";
            return (
              <Choice
                key={s.id}
                name={`${p.profile}-${s.id}`}
                label={s.label}
                value={value}
                disabled={props.locked}
                options={s.choices.map((c): [string, string] => [c, `${valueName(s.id, c)}${c === s.default ? ` ${t("profiles_default_mark")}` : ""}`])}
                onChange={(v) => set(p.profile, s.id, v, v === s.default)}
              />
            );
          })}
        </fieldset>
      ))}
      <div className="row">
        <button type="button" className="btn btn-primary" disabled={props.locked} onClick={() => save(draft)}>{t("modes_save")}</button>
        <button type="button" className="btn" disabled={props.locked} onClick={() => save({ v: 1, profiles: {} })}>{t("profiles_reset")}</button>
        <button
          type="button"
          className="btn"
          onClick={() => props.run(() => backend.saveFile("dynamis-profiles.json", JSON.stringify(draft, null, 2)).then((where) => t("export_saved", { where })))}
        >
          {t("profiles_export")}
        </button>
        <button type="button" className="btn" disabled={props.locked} onClick={() => fileInput.current?.click()}>{t("profiles_import")}</button>
        <input
          ref={fileInput}
          type="file"
          accept="application/json,.json"
          hidden
          aria-label={t("profiles_import")}
          onChange={async (e) => {
            const f = e.target.files?.[0];
            e.target.value = "";
            if (!f) return;
            const text = await f.text();
            props.run(async () => {
              let o: ProfileOverrides;
              try {
                o = JSON.parse(text);
              } catch {
                throw new Error(t("profiles_bad_file"));
              }
              if (typeof o !== "object" || o === null || typeof o.profiles !== "object") throw new Error(t("profiles_bad_file"));
              await backend.setProfileOverrides(o);
              data.reload();
              return t("profiles_saved");
            });
          }}
        />
      </div>
    </div>
  );
}

// --- Sensors ----------------------------------------------------------------

/** Rates from two readings: CPU package watts and the share of idle time
 * spent in the deepest sleep state. */
export function rates(a: Sensors, b: Sensors): { cpu_w: number | null; deep_pct: number | null } {
  const dt = b.time - a.time;
  if (dt <= 0) return { cpu_w: null, deep_pct: null };
  const pa = a.rapl.find((d) => d.name.startsWith("package"));
  const pb = b.rapl.find((d) => d.id === pa?.id);
  let cpu_w: number | null = null;
  if (pa?.energy_uj != null && pb?.energy_uj != null) {
    let de = pb.energy_uj - pa.energy_uj;
    if (de < 0 && pb.max_energy_range_uj) de += pb.max_energy_range_uj;
    cpu_w = de / 1e6 / dt;
  }
  let deep_pct: number | null = null;
  if (a.idle && b.idle && b.idle.states.length > 0) {
    const deepest = b.idle.states[b.idle.states.length - 1];
    const before = a.idle.states.find((s) => s.index === deepest.index);
    if (before) deep_pct = Math.min(100, Math.max(0, ((deepest.time_us - before.time_us) / 1e6 / (dt * b.idle.cpus)) * 100));
  }
  return { cpu_w, deep_pct };
}

export function SensorsTab(props: { intervalMs?: number }) {
  const [history, setHistory] = useState<Sensors[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const read = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const s = await backend.sensors();
        if (live) setHistory((h) => [...h.slice(-89), s]);
      } catch (e) {
        if (live) setError(String((e as Error)?.message ?? e));
      }
    };
    read();
    const id = window.setInterval(read, props.intervalMs ?? 2000);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, [props.intervalMs]);

  const last = history[history.length - 1];
  if (!last) return <p className="muted">{error ?? t("loading")}</p>;
  const pairs = history.slice(1).map((s, i) => rates(history[i], s));
  const cpuW = pairs.map((p) => p.cpu_w).filter((v): v is number => v != null);
  const deep = pairs.map((p) => p.deep_pct).filter((v): v is number => v != null);
  const batW = history.map((s) => s.battery_w).filter((v): v is number => v != null);
  const row = (label: string, value: string, chart?: number[]) => (
    <div className="stack" style={{ gap: 4 }}>
      <div className="row spread">
        <span className="muted">{label}</span>
        <strong>{value}</strong>
      </div>
      {chart && chart.length > 1 && <Sparkline values={chart} label={label} />}
    </div>
  );
  const pct = (v: number | null) => (v == null ? t("unknown") : `${v.toFixed(1)}%`);
  return (
    <div className="stack">
      <p className="muted small">{t("sensors_intro")}</p>
      {row(t("sensors_battery_w"), last.battery_w != null ? `${last.battery_w.toFixed(2)} W` : last.on_ac ? t("est_plugged") : t("unknown"), batW)}
      {row(t("sensors_cpu_w"), cpuW.length ? `${cpuW[cpuW.length - 1].toFixed(2)} W` : t("sensors_no_rapl"), cpuW)}
      {row(t("sensors_deep_sleep"), deep.length ? `${deep[deep.length - 1].toFixed(0)}%` : t("unknown"), deep)}
      {row(t("sensors_cpu_mhz"), last.cpu_mhz != null ? `${last.cpu_mhz} MHz` : t("unknown"))}
      {last.brightness_pct != null && row(t("sensors_brightness"), `${last.brightness_pct.toFixed(0)}%`)}
      <h3>{t("sensors_pressure")}</h3>
      <p className="muted small">{t("sensors_pressure_desc")}</p>
      {row(t("sensors_pressure_cpu"), pct(last.pressure.cpu))}
      {row(t("sensors_pressure_memory"), pct(last.pressure.memory))}
      {row(t("sensors_pressure_io"), pct(last.pressure.io))}
      <h3>{t("sensors_temperatures")}</h3>
      {last.thermal.length === 0 && <p className="muted">{t("unknown")}</p>}
      {last.thermal.map((z) => (
        <div key={z.name}>{row(z.kind || z.name, `${z.temp_c.toFixed(0)} °C`)}</div>
      ))}
    </div>
  );
}

// --- System -----------------------------------------------------------------

export function SystemTab() {
  const info = useLoad(() => backend.system(), []);
  const conflicts = useLoad(() => backend.conflicts(), []);
  const i = info.data;
  const row = (label: string, value: string) => (
    <div className="row spread">
      <dt className="muted">{label}</dt>
      <dd style={{ margin: 0, fontWeight: 600 }}>{value}</dd>
    </div>
  );
  return (
    <div className="stack">
      {!i ? (
        <p className="muted">{info.error ?? t("loading")}</p>
      ) : (
        <dl className="stack" style={{ margin: 0 }}>
          {row(t("system_version"), i.version)}
          {row(t("system_os"), i.os ?? t("unknown"))}
          {row(t("system_kernel"), i.kernel ?? t("unknown"))}
          {row(t("system_running_for"), duration(i.uptime_s / 3600))}
          {row(t("system_memory"), i.memory_mb != null ? `${i.memory_mb.toFixed(1)} MB` : t("unknown"))}
          {row(t("system_cpu"), i.cpu_pct != null ? `${i.cpu_pct.toFixed(2)}%` : t("unknown"))}
        </dl>
      )}
      <p className="muted small">{t("system_budget")}</p>
      <h3>{t("system_conflicts")}</h3>
      <Conflicts list={conflicts.data} reload={conflicts.reload} />
      <h3>{t("system_desktop")}</h3>
      <p>{t("system_desktop_desc")}</p>
      <h3>{t("system_updates")}</h3>
      <p>{t("system_updates_desc")}</p>
    </div>
  );
}

// --- Developer --------------------------------------------------------------

const SOURCES = ["status", "settings", "learned", "apps", "predict", "profiles", "sensors", "system", "journal"] as const;

export function DeveloperTab(props: { status: Status; run: Run; refresh: () => void }) {
  const [advice, setAdvice] = useState<{ format: string; text: string } | null>(null);
  const [source, setSource] = useState<(typeof SOURCES)[number]>("status");
  const [raw, setRaw] = useState<string>("");
  const [restoring, setRestoring] = useState(false);
  const load = async (which: (typeof SOURCES)[number]) => {
    setSource(which);
    try {
      const v = which === "journal" ? await backend.journal(50) : await (backend[which] as () => Promise<unknown>)();
      setRaw(JSON.stringify(v, null, 2));
    } catch (e) {
      setRaw(String((e as Error)?.message ?? e));
    }
  };
  useEffect(() => {
    load("status");
  }, []);

  return (
    <div className="stack">
      <h3>{t("dev_dry_run")}</h3>
      <Switch checked={!!props.status.dry_run} label={t("dev_dry_run_switch")} onChange={(on) => props.run(() => backend.setDryRun(on))} />
      <p className="muted small">{t("dev_dry_run_desc")}</p>

      <h3>{t("dev_advisor")}</h3>
      <p className="muted small">{t("dev_advisor_desc")}</p>
      <div className="row">
        {(["tlp", "tuned"] as const).map((f) => (
          <button key={f} type="button" className="btn btn-small" onClick={() => props.run(async () => setAdvice({ format: f, text: await backend.advise(f) }))}>
            {t(f === "tlp" ? "dev_advisor_tlp" : "dev_advisor_tuned")}
          </button>
        ))}
      </div>
      {advice && (
        <>
          <pre className="code" aria-label={t("dev_advisor")}>{advice.text}</pre>
          <div>
            <button
              type="button"
              className="btn btn-small"
              onClick={() =>
                props.run(() =>
                  backend
                    .saveFile(advice.format === "tlp" ? "50-dynamis-power.conf" : "tuned.conf", advice.text)
                    .then((where) => t("export_saved", { where })),
                )
              }
            >
              {t("dev_advisor_save")}
            </button>
          </div>
        </>
      )}

      <h3>{t("dev_inspector")}</h3>
      <p className="muted small">{t("dev_inspector_desc")}</p>
      <Choice
        name="dev-source"
        label={t("dev_inspector_source")}
        value={source}
        options={SOURCES.map((s): [(typeof SOURCES)[number], string] => [s, s])}
        onChange={(v) => load(v)}
      />
      <pre className="code" aria-label={t("dev_inspector")}>{raw}</pre>

      <h3>{t("home_restore")}</h3>
      <div>
        <button type="button" className="btn" onClick={() => setRestoring(true)}>{t("home_restore")}</button>
      </div>
      <RestoreDialog
        open={restoring}
        onClose={() => setRestoring(false)}
        onDone={() => {
          setRestoring(false);
          props.refresh();
        }}
      />
    </div>
  );
}

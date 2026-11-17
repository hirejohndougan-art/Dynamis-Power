import { useEffect, useState } from "react";
import { backend, type AutoPrefs, type Profile, type Status, type TimeRule } from "../backend";
import { Choice, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { hourName, profileName, t } from "../i18n";

const HOURS = Array.from({ length: 24 }, (_, h) => h);

/** Modes for parts of the day; saved with the other automatic choices. */
function TimeRules(props: { rules: TimeRule[]; profiles: Profile[]; onChange: (r: TimeRule[]) => void }) {
  const [from, setFrom] = useState(21);
  const [to, setTo] = useState(23);
  const [mode, setMode] = useState<Profile>("power-saver");
  const hourSelect = (label: string, value: number, onChange: (h: number) => void) => (
    <label className="field">
      <span className="field-label">{label}</span>
      <select value={value} onChange={(e) => onChange(Number(e.target.value))}>
        {HOURS.map((h) => (
          <option key={h} value={h}>{hourName(h)}</option>
        ))}
      </select>
    </label>
  );
  return (
    <fieldset className="field">
      <legend className="field-label">{t("time_rules_heading")}</legend>
      <p className="muted small">{t("time_rules_desc")}</p>
      {props.rules.length === 0 ? (
        <p className="muted small">{t("time_rules_none")}</p>
      ) : (
        <ul className="stack plain-list">
          {props.rules.map((r, i) => (
            <li key={`${r.from_hour}-${r.to_hour}-${r.profile}`} className="row spread">
              <span>{t("time_rule", { from: hourName(r.from_hour), to: hourName(r.to_hour), mode: profileName(r.profile) })}</span>
              <button type="button" className="btn btn-small" onClick={() => props.onChange(props.rules.filter((_, j) => j !== i))}>
                {t("time_rule_remove")}
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="row" style={{ alignItems: "flex-end" }}>
        {hourSelect(t("time_rule_from"), from, setFrom)}
        {hourSelect(t("time_rule_to"), to, setTo)}
        <label className="field">
          <span className="field-label">{t("time_rule_mode")}</span>
          <select value={mode} onChange={(e) => setMode(e.target.value as Profile)}>
            {props.profiles.map((p) => (
              <option key={p} value={p}>{profileName(p)}</option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="btn"
          disabled={from === to}
          onClick={() => props.onChange([...props.rules, { from_hour: from, to_hour: to, profile: mode }])}
        >
          {t("time_rule_add")}
        </button>
      </div>
    </fieldset>
  );
}

export function Modes(props: { status: Status; refresh: () => void }) {
  const s = props.status;
  const loaded = useLoad(() => backend.autoPrefs(), []);
  const [prefs, setPrefs] = useState<AutoPrefs | null>(null);
  const tray = useLoad(() => backend.trayShown(), []);
  const host = useLoad(() => backend.trayHost(), []);
  const { run, toast } = useAction(() => {
    props.refresh();
    tray.reload();
  });
  useEffect(() => {
    if (loaded.data) setPrefs(loaded.data);
  }, [loaded.data]);
  const set = <K extends keyof AutoPrefs>(k: K, v: AutoPrefs[K]) => prefs && setPrefs({ ...prefs, [k]: v });

  const select = (k: "on_ac" | "on_battery", label: string) =>
    prefs && (
      <Choice
        name={k}
        label={label}
        value={prefs[k]}
        options={s.profiles.map((p): [Profile, string] => [p, profileName(p)])}
        onChange={(v) => set(k, v)}
      />
    );

  return (
    <>
      <header className="page-head">
        <h1>{t("modes_title")}</h1>
      </header>

      <div className="grid">
        {s.profiles.map((p) => (
          <section key={p} className="card" aria-labelledby={`mode-${p}`}>
            <div className="row spread">
              <h2 id={`mode-${p}`}>{profileName(p)}</h2>
              {s.profile === p && <span className="badge badge-safe">{t("modes_current")}</span>}
            </div>
            <p className="muted">{t(`profile_${p.replace("-", "_")}_desc` as "profile_balanced_desc")}</p>
            {s.profile !== p && (
              <div>
                <button type="button" className="btn btn-small" disabled={!!s.safe_mode} onClick={() => run(() => backend.setProfile(p))}>
                  {t("modes_use")}
                </button>
              </div>
            )}
          </section>
        ))}
      </div>

      <p className="muted small">
        {t("modes_bright_moved")} <a className="btn-link" href="#screen">{t("nav_screen")}</a>
      </p>

      {(s.missing_modes ?? []).length > 0 && (
        <div className="stack">
          {(s.missing_modes ?? []).map((m) => (
            <p key={m.profile} className="notice notice-info">
              {t("modes_missing", {
                mode: profileName(m.profile),
                why: m.why.includes("no controls") ? t("modes_why_no_controls") : t("modes_why_same"),
              })}
            </p>
          ))}
        </div>
      )}

      <section className="card" aria-labelledby="auto-heading">
        <div className="row spread">
          <h2 id="auto-heading">{t("modes_auto_heading")}</h2>
          <Switch checked={s.auto.on} label={t("mode_auto")} disabled={!!s.safe_mode} onChange={(on) => run(() => backend.setAuto(on))} />
        </div>
        <p className="muted">{t("modes_auto_intro")}</p>
        {prefs && (
          <>
            <div className="row">
              {select("on_ac", s.power.has_battery ? t("modes_on_ac") : t("modes_on_mains_only"))}
              {s.power.has_battery && select("on_battery", t("modes_on_battery"))}
            </div>
            {s.power.has_battery && (
              <label className="field">
                <span className="field-label">{t("modes_low_battery")}: {prefs.low_battery_pct}%</span>
                <input
                  type="range"
                  min={Math.max(10, prefs.critical_battery_pct)}
                  max={60}
                  step={5}
                  value={prefs.low_battery_pct}
                  onChange={(e) => set("low_battery_pct", Number(e.target.value))}
                  aria-label={t("modes_low_battery")}
                />
              </label>
            )}
            <Switch checked={prefs.boost_on_ac} onChange={(v) => set("boost_on_ac", v)} label={t("modes_boost")} />
            <Choice
              name="startup_mode"
              label={t("modes_startup")}
              value={prefs.startup_mode ?? "last"}
              options={[["last", t("modes_startup_last")] as ["last", string], ...s.profiles.map((p): [Profile, string] => [p, profileName(p)])]}
              onChange={(v) => set("startup_mode", v === "last" ? null : (v as Profile))}
            />
            <TimeRules rules={prefs.time_rules ?? []} profiles={s.profiles} onChange={(r) => set("time_rules", r)} />
            <div>
              <button type="button" className="btn btn-primary" onClick={() => run(() => backend.setAutoPrefs(prefs).then(() => t("modes_saved")))}>
                {t("modes_save")}
              </button>
            </div>
          </>
        )}
      </section>
      <section className="card" aria-labelledby="panel-heading">
        <h2 id="panel-heading">{t("modes_panel_heading")}</h2>
        {tray.error ? (
          <p className="muted">{tray.error}</p>
        ) : (
          <Switch checked={tray.data ?? false} disabled={tray.data == null} label={t("modes_panel")} onChange={(on) => run(() => backend.setTray(on))} />
        )}
        <p className="muted">{t("modes_panel_desc")}</p>
        {tray.data && host.data === false && <p className="notice notice-info">{t("modes_panel_no_host")}</p>}
      </section>
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

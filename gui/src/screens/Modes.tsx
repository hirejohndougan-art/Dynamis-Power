import { useEffect, useRef, useState } from "react";
import { backend, type AutoPrefs, type BrightnessPrefs, type Profile, type Status } from "../backend";
import { Choice, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { profileName, t } from "../i18n";
import { brightnessMinutes } from "../power";

type Levels = BrightnessPrefs["levels"];

/** The brightness chosen for each mode, saved shortly after the last move. */
function useBrightness(onSaved: () => void) {
  const info = useLoad(() => backend.brightness(), []);
  const [levels, setLevels] = useState<Levels | null>(null);
  const [error, setError] = useState<string | null>(null);
  const dirty = useRef(false);
  useEffect(() => {
    if (info.data && !dirty.current) setLevels(info.data.levels);
  }, [info.data]);
  useEffect(() => {
    if (!levels || !dirty.current) return;
    const id = window.setTimeout(() => {
      dirty.current = false;
      backend.setBrightnessPrefs({ levels }).then(
        () => {
          setError(null);
          info.reload();
          onSaved();
        },
        (e) => setError(String(e?.message ?? e)),
      );
    }, 500);
    return () => window.clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [levels]);
  const change = (next: Levels) => {
    dirty.current = true;
    setLevels(next);
  };
  const set = (p: Profile, v: number | null) => {
    if (!levels) return;
    const next = { ...levels };
    if (v == null) delete next[p];
    else next[p] = v;
    change(next);
  };
  return { info: info.data, levels, set, change, error };
}

/** Where a mode's slider starts: the brightness now, but no brighter than
 * 40% for Longer battery. */
function startLevel(p: Profile, nowPct: number | null, min: number) {
  const now = Math.round((nowPct ?? 60) / 5) * 5;
  return Math.max(min, p === "power-saver" ? Math.min(now, 40) : now);
}

function ModeBrightness(props: { profile: Profile; status: Status; bright: ReturnType<typeof useBrightness> }) {
  const { info, levels, set } = props.bright;
  if (!info?.supported || !levels) return null;
  const p = props.profile;
  const level = levels[p];
  const d = props.status.display;
  const id = `bright-${p}`;
  let estimate: string;
  if (level == null) estimate = t("bright_left_alone");
  else if (!d) estimate = t("bright_learning");
  else {
    const mins = brightnessMinutes(level, info.now_pct, d, props.status.power);
    const cost = t("bright_cost", { w: (d.w_per_pct * level).toFixed(1) });
    const diff =
      mins == null ? "" : Math.abs(mins) < 3 ? t("bright_same") : mins > 0 ? t("bright_more", { min: Math.round(mins) }) : t("bright_less", { min: Math.round(-mins) });
    estimate = diff ? `${cost} ${diff}` : cost;
  }
  return (
    <div className="stack mode-bright" style={{ gap: 6 }}>
      <Switch
        checked={level != null}
        disabled={!!props.status.safe_mode}
        label={t("bright_set")}
        onChange={(on) => set(p, on ? startLevel(p, info.now_pct, info.min_pct) : null)}
      />
      {level != null && (
        <label className="field" htmlFor={id}>
          <span className="field-label">
            {t("bright_label")}: {level}%
          </span>
          <input
            id={id}
            type="range"
            min={info.min_pct}
            max={100}
            step={5}
            value={level}
            disabled={!!props.status.safe_mode}
            onChange={(e) => set(p, Number(e.target.value))}
            aria-label={`${t("bright_label")}, ${profileName(p)}`}
          />
        </label>
      )}
      <p className="muted small" aria-live="polite">{estimate}</p>
    </div>
  );
}

export function Modes(props: { status: Status; refresh: () => void }) {
  const s = props.status;
  const loaded = useLoad(() => backend.autoPrefs(), []);
  const [prefs, setPrefs] = useState<AutoPrefs | null>(null);
  const tray = useLoad(() => backend.trayShown(), []);
  const bright = useBrightness(props.refresh);
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
            <ModeBrightness profile={p} status={s} bright={bright} />
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

      {bright.info?.supported && bright.levels && (
        <div className="stack" style={{ gap: 8 }}>
          <p className="muted small">{t("bright_how")}</p>
          {Object.keys(bright.levels).length === 0 && (
            <div className="row">
              <button
                type="button"
                className="btn btn-small"
                disabled={!!s.safe_mode}
                onClick={() => {
                  const i = bright.info;
                  if (i) bright.change({ "power-saver": startLevel("power-saver", i.now_pct, i.min_pct), balanced: startLevel("balanced", i.now_pct, i.min_pct) });
                }}
              >
                {t("bright_suggest")}
              </button>
              <span className="muted small">{t("bright_suggest_desc")}</span>
            </div>
          )}
          {bright.error && <p className="notice" role="alert">{bright.error}</p>}
        </div>
      )}

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
              {select("on_ac", t("modes_on_ac"))}
              {select("on_battery", t("modes_on_battery"))}
            </div>
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
            <Switch checked={prefs.boost_on_ac} onChange={(v) => set("boost_on_ac", v)} label={t("modes_boost")} />
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
      </section>
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

import { useEffect, useState } from "react";
import { backend, type AutoPrefs, type Profile, type Status } from "../backend";
import { Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { profileName, t } from "../i18n";

export function Modes(props: { status: Status; refresh: () => void }) {
  const s = props.status;
  const loaded = useLoad(() => backend.autoPrefs(), []);
  const [prefs, setPrefs] = useState<AutoPrefs | null>(null);
  const tray = useLoad(() => backend.trayShown(), []);
  const { run, toast } = useAction(() => {
    props.refresh();
    tray.reload();
  });
  useEffect(() => {
    if (loaded.data) setPrefs(loaded.data);
  }, [loaded.data]);
  const set = <K extends keyof AutoPrefs>(k: K, v: AutoPrefs[K]) => prefs && setPrefs({ ...prefs, [k]: v });

  const select = (k: "on_ac" | "on_battery", label: string) => (
    <label className="field">
      <span className="field-label">{label}</span>
      <select value={prefs?.[k]} onChange={(e) => set(k, e.target.value as Profile)}>
        {s.profiles.map((p) => (
          <option key={p} value={p}>{profileName(p)}</option>
        ))}
      </select>
    </label>
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

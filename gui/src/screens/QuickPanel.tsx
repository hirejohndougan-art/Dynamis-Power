import { useEffect, useRef, useState } from "react";
import { backend, type Profile, type Status } from "../backend";
import { Logo, Switch } from "../components";
import { clock, duration, profileName, t } from "../i18n";

const MODES: Profile[] = ["power-saver", "balanced", "performance"];

/** The quick panel: a left click on the panel icon. The battery, the modes,
 * automatic choice and the screen brightness, with a gear for the full app.
 * Escape closes it; so does clicking elsewhere (the window does that). */
export function QuickPanel(props: { status: Status | null; refresh: () => void }) {
  const s = props.status;
  const [bright, setBright] = useState<number | null>(null);
  const [asking, setAsking] = useState<Profile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    void backend.screenBrightness().then(setBright);
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") void backend.closeQuick();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);

  const act = (f: () => Promise<unknown>) =>
    f()
      .then(() => {
        setError(null);
        props.refresh();
      })
      .catch((e) => setError(String((e as Error)?.message ?? e)));

  // The brightness follows the slider at once, but is sent at most every
  // 120 ms while it is dragged.
  const slide = (v: number) => {
    setBright(v);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => act(() => backend.setScreenBrightness(v)), 120);
  };

  const pick = (p: Profile) => {
    if (!s || s.profile === p) return;
    if (s.last_until && p !== "power-saver") {
      setAsking(p);
      return;
    }
    act(() => backend.setProfile(p));
  };

  const locked = !!s?.safe_mode;
  const battery = s?.power.has_battery
    ? s.power.on_ac
      ? `${Math.round(s.power.battery_pct ?? 0)}% · ${t("est_plugged")}`
      : `${Math.round(s.power.battery_pct ?? 0)}%${s.power.hours_left != null ? ` · ${t("est_left", { time: duration(s.power.hours_left) })}` : ""}`
    : null;

  return (
    <div className="quick" role="dialog" aria-label={t("quick_title")}>
      <div className="row spread">
        <div className="row" style={{ gap: 8 }}>
          <Logo />
          <strong>{t("app_name")}</strong>
        </div>
        <button type="button" className="icon-btn" aria-label={t("quick_settings")} title={t("quick_settings")} onClick={() => act(() => backend.openFull())}>
          <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M19.4 13a7.6 7.6 0 0 0 0-2l2.1-1.6a.5.5 0 0 0 .1-.6l-2-3.5a.5.5 0 0 0-.6-.2l-2.5 1a7.4 7.4 0 0 0-1.7-1l-.4-2.6a.5.5 0 0 0-.5-.4h-4a.5.5 0 0 0-.5.4l-.4 2.6a7.4 7.4 0 0 0-1.7 1l-2.5-1a.5.5 0 0 0-.6.2l-2 3.5a.5.5 0 0 0 .1.6L4.6 11a7.6 7.6 0 0 0 0 2l-2.1 1.6a.5.5 0 0 0-.1.6l2 3.5c.1.2.4.3.6.2l2.5-1c.5.4 1.1.7 1.7 1l.4 2.6c0 .2.2.4.5.4h4c.3 0 .5-.2.5-.4l.4-2.6c.6-.3 1.2-.6 1.7-1l2.5 1c.2.1.5 0 .6-.2l2-3.5a.5.5 0 0 0-.1-.6zM12 15.5A3.5 3.5 0 1 1 12 8.5a3.5 3.5 0 0 1 0 7z"
            />
          </svg>
        </button>
      </div>
      {!s ? (
        <p className="muted" role="status">{t("loading")}</p>
      ) : (
        <>
          {battery && <p className="quick-battery">{battery}</p>}
          {locked && <p className="notice notice-info small">{t("tray_safe")}</p>}
          <div role="radiogroup" aria-label={t("quick_mode")} className="quick-modes">
            {MODES.filter((p) => s.profiles.includes(p)).map((p) => (
              <button key={p} type="button" role="radio" aria-checked={s.profile === p} className="segment" disabled={locked} onClick={() => pick(p)}>
                {profileName(p)}
              </button>
            ))}
          </div>
          {asking && s.last_until && (
            <div className="notice notice-info small" role="alertdialog" aria-label={t("lu_stop_title")} style={{ flexDirection: "column", alignItems: "flex-start", gap: 6 }}>
              <span>{t("lu_stop_desc", { time: clock(s.last_until.until), mode: profileName(asking) })}</span>
              <div className="row" style={{ gap: 6 }}>
                <button type="button" className="btn btn-small" onClick={() => setAsking(null)}>{t("lu_stop_keep")}</button>
                <button
                  type="button"
                  className="btn btn-small btn-primary"
                  onClick={() => {
                    const p = asking;
                    setAsking(null);
                    act(async () => {
                      await backend.setTarget(0);
                      await backend.setProfile(p);
                    });
                  }}
                >
                  {t("lu_stop_switch", { mode: profileName(asking) })}
                </button>
              </div>
            </div>
          )}
          <Switch checked={s.auto.on} disabled={locked} label={t("tray_auto_switch")} onChange={(on) => act(() => backend.setAuto(on))} />
          {bright != null && (
            <label className="quick-slider">
              <span>{t("quick_brightness", { pct: bright })}</span>
              <input type="range" min={5} max={100} step={1} value={bright} aria-label={t("quick_brightness_label")} onChange={(e) => slide(Number(e.target.value))} />
            </label>
          )}
          {error && <p className="small" role="alert">{error}</p>}
        </>
      )}
    </div>
  );
}

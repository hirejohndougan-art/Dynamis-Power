import { useEffect, useRef, useState } from "react";
import { backend, type BrightnessPrefs, type DesktopPolicy, type Profile, type Status } from "../backend";
import { Switch } from "../components";
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

/** Screen-off times offered, in seconds; null leaves the desktop's own. */
const OFF_TIMES: (number | null)[] = [null, 30, 60, 120, 180, 300, 600, 900, 1800, 3600];

function offLabel(s: number | null): string {
  if (s == null) return t("screen_off_desktop");
  if (s < 60) return t("screen_off_s", { n: s });
  if (s < 3600) return t("screen_off_min", { n: s / 60 });
  return t("screen_off_hour");
}

/** Screen: everything about the screen in one place. The brightness now,
 * the brightness for each mode, when the screen turns off (with animated
 * dim), and the refresh rate and dark style on battery. */
export function Screen(props: { status: Status; refresh: () => void }) {
  const s = props.status;
  const { run, toast } = useAction(props.refresh);
  const bright = useBrightness(props.refresh);
  const [now, setNow] = useState<number | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    void backend.screenBrightness().then(setNow);
  }, []);
  const slide = (v: number) => {
    setNow(v);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void backend.setScreenBrightness(v).catch(() => undefined), 120);
  };
  const d = s.desktop;
  const f = d?.found;
  const setDesk = (change: Partial<DesktopPolicy>) => d && run(() => backend.setDesktopPolicy({ ...d.policy, ...change }));
  const locked = !!s.safe_mode;
  const fast = f?.refresh_hz != null && f.low_hz != null && f.refresh_hz > f.low_hz + 1;
  const canAct = !!f && f.desktop !== "other";
  const select = (key: "screen_off_battery_s" | "screen_off_ac_s", label: string) =>
    d && (
      <label className="field">
        <span className="field-label">{label}</span>
        <select
          value={d.policy[key] == null ? "" : String(d.policy[key])}
          disabled={!canAct}
          onChange={(e) => setDesk({ [key]: e.target.value === "" ? null : Number(e.target.value) })}
        >
          {OFF_TIMES.map((v) => (
            <option key={String(v)} value={v == null ? "" : String(v)}>
              {offLabel(v)}
            </option>
          ))}
        </select>
      </label>
    );

  return (
    <>
      <header className="page-head">
        <h1>{t("screen_title")}</h1>
      </header>

      <section className="card" aria-labelledby="scr-now-heading">
        <h2 id="scr-now-heading">{t("screen_now_heading")}</h2>
        {now == null ? (
          <p className="muted">{t("screen_no_backlight")}</p>
        ) : (
          <label className="quick-slider">
            <span>{t("quick_brightness", { pct: now })}</span>
            <input type="range" min={5} max={100} step={1} value={now} aria-label={t("quick_brightness_label")} onChange={(e) => slide(Number(e.target.value))} />
          </label>
        )}
      </section>

      {bright.info?.supported && bright.levels && (
        <section className="card" aria-labelledby="scr-modes-heading">
          <h2 id="scr-modes-heading">{t("screen_modes_heading")}</h2>
          <div className="grid">
            {s.profiles.map((p) => (
              <div key={p} className="stack" style={{ gap: 4 }}>
                <strong>{profileName(p)}</strong>
                <ModeBrightness profile={p} status={s} bright={bright} />
              </div>
            ))}
          </div>
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
        </section>
      )}

      {d && (
        <section className="card" aria-labelledby="scr-off-heading">
          <h2 id="scr-off-heading">{t("screen_off_heading")}</h2>
          <p className="muted">{t("screen_off_intro")}</p>
          {!f ? (
            <p className="notice notice-info">{t("desk_no_helper")}</p>
          ) : f.desktop === "other" ? (
            <p className="notice notice-info">{t("desk_other")}</p>
          ) : null}
          <div className="grid">
            {s.power.has_battery && select("screen_off_battery_s", t("screen_off_battery"))}
            {select("screen_off_ac_s", s.power.has_battery ? t("screen_off_ac") : t("screen_off_always"))}
          </div>
          <Switch checked={d.policy.animated_dim} disabled={!canAct} label={t("screen_animated_dim")} onChange={(v) => setDesk({ animated_dim: v })} />
          <p className="muted small">{t("screen_animated_dim_desc")}</p>
        </section>
      )}

      {d && s.power.has_battery && (fast || f?.can_style) && (
        <section className="card" aria-labelledby="scr-bat-heading">
          <h2 id="scr-bat-heading">{t("screen_battery_heading")}</h2>
          <p className="muted">{t("desk_intro")}</p>
          {fast && (
            <>
              <Switch
                checked={d.policy.lower_refresh}
                disabled={locked}
                label={t("desk_refresh", { from: Math.round(f!.refresh_hz!), to: Math.round(f!.low_hz!) })}
                onChange={(v) => setDesk({ lower_refresh: v })}
              />
              <p className="muted small">{t("desk_refresh_desc")}</p>
            </>
          )}
          {f?.can_style && (
            <>
              <Switch checked={d.policy.dark_style} disabled={locked} label={t("desk_dark")} onChange={(v) => setDesk({ dark_style: v })} />
              <p className="muted small">{t(d.oled ? "desk_dark_oled" : "desk_dark_desc")}</p>
            </>
          )}
        </section>
      )}
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

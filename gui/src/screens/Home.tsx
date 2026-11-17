import { useState } from "react";
import { backend, type FeelsSlowResult, type Profile, type Status } from "../backend";
import { Dialog, PowerChart, ProgressBar, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { clock, duration, formatTime, hourName, profileName, t, tn } from "../i18n";
import { describe } from "../activity";
import { useTrouble } from "./Trouble";

function Estimate({ s }: { s: Status }) {
  const p = s.power;
  let text: string;
  if (!p.has_battery) text = t("est_no_battery");
  else if (p.charging && p.hours_to_full != null) text = t("est_to_full", { time: duration(p.hours_to_full) });
  else if (p.on_ac) text = p.battery_pct === 100 ? t("est_full") : t("est_plugged");
  else if (p.hours_left != null) text = t("est_left", { time: duration(p.hours_left) });
  else text = t("est_unknown");
  return (
    <div className="stack">
      <p className="big">{text}</p>
      {p.battery_pct != null && <BatteryMeter pct={p.battery_pct} charging={!!p.charging} />}
    </div>
  );
}

/** A battery-shaped meter across the card with the percentage inside. The
 * label is drawn twice (dark over the fill, normal text over the empty part)
 * so it stays readable wherever the fill ends. */
function BatteryMeter({ pct, charging }: { pct: number; charging: boolean }) {
  const v = Math.max(0, Math.min(100, Math.round(pct)));
  const label = charging ? t("meter_charging", { pct: v }) : t("meter_pct", { pct: v });
  return (
    <div
      className={`meter${v <= 20 ? " meter-low" : ""}`}
      role="meter"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={v}
      aria-valuetext={label}
      aria-label={t("meter_label")}
    >
      <div className="meter-body">
        <div className="meter-fill" style={{ width: `${v}%` }} />
        <span className="meter-text" aria-hidden="true">{label}</span>
        <span className="meter-text meter-text-on" aria-hidden="true" style={{ clipPath: `inset(0 ${100 - v}% 0 0)` }}>
          {label}
        </span>
      </div>
      <div className="meter-cap" aria-hidden="true" />
    </div>
  );
}

const DISMISSED = "dynamis.unclean-seen";

/** After an unclean shutdown: what was happening, in plain words. */
function UncleanNotice({ s }: { s: Status }) {
  const last = s.unclean?.last;
  const [seen, setSeen] = useState<number | null>(() => {
    try {
      return Number(localStorage.getItem(DISMISSED)) || null;
    } catch {
      return null;
    }
  });
  if (!last || seen === last.found_at || Date.now() / 1000 - last.found_at > 3 * 86400) return null;
  const names = (l: { label: string; id: string }[]) => l.map((x) => x.label || x.id).join(", ");
  const dismiss = () => {
    try {
      localStorage.setItem(DISMISSED, String(last.found_at));
    } catch {
      /* per-viewer convenience only */
    }
    setSeen(last.found_at);
  };
  return (
    <div className="notice notice-info" role="status" style={{ flexDirection: "column", alignItems: "flex-start", gap: 4 }}>
      <strong>{t("unclean_heading")}</strong>
      <span>{t(`unclean_kernel_${last.kernel}` as "unclean_kernel_nothing")}</span>
      {last.in_safe_mode ? (
        <span>{t("unclean_in_safe_mode")}</span>
      ) : (
        <>
          {last.in_progress && <span>{t("unclean_in_progress", { label: last.in_progress.label || last.in_progress.id })}</span>}
          {last.recent.length > 0 ? (
            <span>{t("unclean_recent", { labels: names(last.recent) })}</span>
          ) : (
            last.ended_at != null && !last.in_progress && <span>{t("unclean_none_recent")}</span>
          )}
        </>
      )}
      {last.detail && (
        <details>
          <summary>{t("details")}</summary>
          <code className="small">{last.detail}</code>
        </details>
      )}
      <button type="button" className="btn btn-small" onClick={dismiss}>
        {t("got_it")}
      </button>
    </div>
  );
}

/** Power use now, with a chart of the last half hour (readings the app has
 * seen since it was opened, every 5 seconds). */
function PowerCard(props: { s: Status; watts: number[]; temps?: number[] }) {
  const { s, watts } = props;
  const now = s.power.on_ac ? null : s.power.watts;
  const avg = watts.length ? watts.reduce((a, b) => a + b, 0) / watts.length : null;
  const minutes = Math.max(1, Math.round(((watts.length - 1) * 5) / 60));
  return (
    <section className="card" aria-labelledby="draw-heading">
      <h2 id="draw-heading">{t("draw_heading")}</h2>
      <div className="stat-row">
        {now != null ? <p className="big">{t("draw_watts", { w: now.toFixed(1) })}</p> : <p className="muted">{t("draw_none")}</p>}
        {now != null && avg != null && watts.length > 1 && (
          <p className="muted">{t("draw_avg", { w: avg.toFixed(1), m: minutes })}</p>
        )}
        {s.power.temp_c != null && <p className="muted">{t("draw_temp", { c: Math.round(s.power.temp_c) })}</p>}
      </div>
      <PowerChart
        values={s.power.on_ac ? [] : watts}
        stepS={5}
        second={
          !s.power.on_ac && props.temps && props.temps.some((c) => !Number.isNaN(c))
            ? { values: props.temps, unit: "°C", label: t("draw_temp_line") }
            : undefined
        }
        label={
          watts.length > 1
            ? t("draw_chart_label", { m: minutes, min: Math.min(...watts).toFixed(1), max: Math.max(...watts).toFixed(1), now: (now ?? watts[watts.length - 1]).toFixed(1) })
            : t("draw_chart_empty")
        }
        empty={s.power.on_ac ? t("draw_chart_plugged") : t("draw_chart_empty")}
      />
    </section>
  );
}

export function Home(props: { status: Status; watts: number[]; temps?: number[]; refresh: () => void; go: (screen: string) => void }) {
  const s = props.status;
  const journal = useLoad(() => backend.journal(30), [s.profile, s.learning.tests]);
  const { run, toast } = useAction(() => {
    props.refresh();
    journal.reload();
  });
  const [restoring, setRestoring] = useState(false);
  // A faster mode while "Make it last" is on: ask before stopping it.
  const [stopLasting, setStopLasting] = useState<Profile | null>(null);
  const trouble = useTrouble(s, run);

  const last = (journal.data ?? []).map(describe).find((i) => i !== null) ?? null;
  const held = s.holds[0];
  const nowLine = s.last_until
    ? t("lu_home", { time: clock(s.last_until.until), reason: s.last_until.reason ?? "" })
    : held
    ? t("mode_held", { app: held.app, mode: profileName(held.profile), reason: held.reason })
    : s.auto.on
      ? t("mode_now_auto", { reason: s.auto.reason })
      : t("mode_now_manual", { mode: profileName(s.profile) });

  const savedMinutes =
    s.learning.saves_w > 0 && s.power.watts && s.power.hours_left
      ? Math.round(s.power.hours_left * 60 * (s.learning.saves_w / s.power.watts))
      : 0;

  return (
    <>
      <header className="page-head">
        <h1>{t("home_title")}</h1>
        <div className="row">
          {trouble.button}
          <button type="button" className="btn" onClick={() => setRestoring(true)}>
            {t("home_restore")}
          </button>
        </div>
      </header>
      {trouble.notice}
      {trouble.dialog}

      <UncleanNotice s={s} />
      {(s.missing_parts ?? []).length > 0 && (
        <p className="notice" role="alert">
          {(s.missing_parts ?? []).map((m) => t(m === "polkit" ? "missing_polkit" : "missing_systemd")).join(" ")}
        </p>
      )}
      {s.safe_mode && (
        <div className="notice" role="alert">
          <div className="stack" style={{ gap: 4 }}>
            <span>{t("safe_mode_on", { why: s.safe_mode })}</span>
            {(s.unclean?.in_safe_mode ?? 0) > 0 ? (
              <span>
                {s.unclean!.in_safe_mode >= 2
                  ? t("safe_mode_still_froze", { n: s.unclean!.in_safe_mode })
                  : t("safe_mode_froze_once")}
              </span>
            ) : (
              (s.unclean?.suspects ?? []).length > 0 && (
                <span>{t("safe_mode_suspects", { labels: s.unclean!.suspects.map((x) => x.label || x.id).join(", ") })}</span>
              )
            )}
          </div>
          <button type="button" className="btn btn-small" onClick={() => run(async () => { await backend.setSafeMode(false); })}>
            {t("safe_mode_off_button")}
          </button>
        </div>
      )}
      <SuggestionNotice s={s} run={run} />
      {s.hotter && (
        <p className="notice notice-info" role="status">
          {t("hotter", { by: Math.round(s.hotter.by_c), since: monthName(s.hotter.since) })}
        </p>
      )}
      {s.pending?.trial && <div className="notice notice-info">{t("trial_running", { label: s.pending.label })}</div>}
      {(s.kept?.held_back ?? []).length > 0 && (
        <div className="notice">
          {t("kept_held_back", { labels: (s.kept?.held_back ?? []).map((id) => s.kept?.settings.find((k) => k.id === id)?.label || id).join(", ") })}
        </div>
      )}
      {s.dry_run && (
        <div className="notice notice-info">
          {t("dry_run_on")} <a href="#advanced:developer">{t("tab_developer")}</a>
        </div>
      )}

      <PowerCard s={s} watts={props.watts} temps={props.temps} />

      <section className="card" aria-label={t("battery_title")}>
        <Estimate s={s} />
        <p>{nowLine}</p>
      </section>

      <section className="card" aria-labelledby="mode-heading">
        <div className="row spread">
          <h2 id="mode-heading">{t("mode_heading")}</h2>
          <Switch checked={s.auto.on} label={t("mode_auto")} disabled={!!s.safe_mode} onChange={(on) => run(() => backend.setAuto(on))} />
        </div>
        <div className="segmented" role="radiogroup" aria-labelledby="mode-heading">
          {s.profiles.map((p: Profile) => (
            <button
              key={p}
              type="button"
              role="radio"
              aria-checked={s.profile === p}
              className="segment"
              disabled={!!s.safe_mode}
              onClick={() => (s.last_until && p !== "power-saver" && p !== s.profile ? setStopLasting(p) : run(() => backend.setProfile(p)))}
            >
              <span className="segment-title">{profileName(p)}</span>
              <span className="muted small">{t(`profile_${p.replace("-", "_")}_desc` as "profile_balanced_desc")}</span>
            </button>
          ))}
        </div>
        <p className="muted small">{t("mode_auto_desc")}</p>
        <FeelsSlow status={s} run={run} />
        {stopLasting && s.last_until && (
          <Dialog
            open
            onClose={() => setStopLasting(null)}
            title={t("lu_stop_title")}
            actions={
              <>
                <button type="button" className="btn" onClick={() => setStopLasting(null)}>{t("lu_stop_keep")}</button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => {
                    const p = stopLasting;
                    setStopLasting(null);
                    run(async () => {
                      await backend.setTarget(0);
                      await backend.setProfile(p);
                      return t("lu_stopped", { mode: profileName(p) });
                    });
                  }}
                >
                  {t("lu_stop_switch", { mode: profileName(stopLasting) })}
                </button>
              </>
            }
          >
            <p>{t("lu_stop_desc", { time: clock(s.last_until.until), mode: profileName(stopLasting) })}</p>
          </Dialog>
        )}
      </section>

      <div className="grid">
        <section className="card" aria-labelledby="learn-heading">
          <div className="row spread">
            <h2 id="learn-heading">{t("learn_heading")}</h2>
            <Switch checked={s.learning.on} label={t("learn_switch")} onChange={(on) => run(() => backend.setLearning(on))} />
          </div>
          {s.learning.progress && (
            <>
              <ProgressBar pct={s.learning.progress.pct} />
              <p className="small">{t("learn_pct", { pct: s.learning.progress.pct })}</p>
            </>
          )}
          {s.learning.on ? (
            <>
              <p>{tn("learn_progress", s.learning.tested_settings, { checked: s.learning.tested_settings, kept: s.learning.kept.length })}</p>
              {s.learning.saves_w > 0 && <p>{t("learn_saves", { w: s.learning.saves_w.toFixed(2), min: savedMinutes })}</p>}
              <p className="muted small">{s.learning.status}</p>
            </>
          ) : (
            <p className="muted">{t("learn_off")}</p>
          )}
          <div className="row">
            <button type="button" className="btn-link" onClick={() => props.go("learning")}>
              {t("learn_details")}
            </button>
            {s.learning.progress && s.learning.progress.left > 0 && (
              <button type="button" className="btn-link" onClick={() => props.go("learning")}>
                {t("learn_now_link")}
              </button>
            )}
          </div>
        </section>
      </div>

      <section className="card" aria-labelledby="last-heading">
        <h2 id="last-heading">{t("last_heading")}</h2>
        {last ? (
          <div className="row spread">
            <div className="stack">
              <p>{last.title}</p>
              <p className="muted small">
                {formatTime(last.entry.time)}
                {last.why && ` · ${last.why}`}
              </p>
            </div>
            {last.undoable && (
              <button type="button" className="btn btn-small" onClick={() => run(() => backend.undo(last.entry.id).then(() => t("undone", { label: last.entry.label })))}>
                {t("undo")}
              </button>
            )}
          </div>
        ) : (
          <p className="muted">{t("last_none")}</p>
        )}
      </section>

      <RestoreDialog open={restoring} onClose={() => setRestoring(false)} onDone={(m) => run(async () => m)} />
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

export function RestoreDialog(props: { open: boolean; onClose: () => void; onDone: (msg: string) => void }) {
  return (
    <Dialog
      open={props.open}
      onClose={props.onClose}
      title={t("restore_title")}
      actions={
        <>
          <button type="button" className="btn" onClick={props.onClose}>{t("cancel")}</button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={async () => {
              props.onClose();
              try {
                const done = await backend.restore();
                props.onDone(done.length ? tn("restore_done", done.length) : t("restore_nothing"));
              } catch (e) {
                props.onDone(String((e as Error)?.message ?? e));
              }
            }}
          >
            {t("restore_confirm")}
          </button>
        </>
      }
    >
      <p>{t("restore_body")}</p>
    </Dialog>
  );
}

/** "It feels slow" (everyday problem §3): undo what costs speed for the
 * rest of the day, say what that was, and offer to keep it that way. */
function FeelsSlow(props: { status: Status; run: (a: () => Promise<string | void>) => void }) {
  const [asking, setAsking] = useState(false);
  const [lasting, setLasting] = useState(false);
  const [result, setResult] = useState<FeelsSlowResult | null>(null);
  const s = props.status;
  if (s.relief) {
    return (
      <div className="notice notice-info" role="status">
        <span>
          {t("slow_on", { time: clock(s.relief) })}
          {result && ` ${slowSummary(result)}`}
        </span>
        <button type="button" className="btn btn-small" onClick={() => props.run(() => backend.endRelief())}>
          {t("slow_end")}
        </button>
      </div>
    );
  }
  return (
    <>
      <div>
        <button type="button" className="btn-link" disabled={!!s.safe_mode} onClick={() => setAsking(true)}>
          {t("slow_button")}
        </button>
      </div>
      {asking && (
      <Dialog
        open={asking}
        onClose={() => setAsking(false)}
        title={t("slow_title")}
        actions={
          <>
            <button type="button" className="btn" onClick={() => setAsking(false)}>{t("cancel")}</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => {
                setAsking(false);
                props.run(async () => {
                  const r = await backend.feelsSlow(lasting);
                  setResult(r);
                  return slowSummary(r);
                });
              }}
            >
              {t("slow_confirm")}
            </button>
          </>
        }
      >
        <p>{t("slow_desc")}</p>
        <label className="check">
          <input type="checkbox" checked={lasting} onChange={(e) => setLasting(e.target.checked)} />
          {t("slow_lasting")}
        </label>
        <p className="muted small">{t("slow_lasting_desc")}</p>
      </Dialog>
      )}
    </>
  );
}

/** A rule offered from the modes picked by hand. Nothing changes before a yes. */
function SuggestionNotice(props: { s: Status; run: (a: () => Promise<string | void>) => void }) {
  const g = props.s.suggestion;
  if (!g || props.s.safe_mode) return null;
  const mode = profileName(g.profile);
  const text =
    g.kind === "app"
      ? t("suggest_app", { mode, count: g.count, app: g.app })
      : t("suggest_time", { mode, from: hourName(g.from_hour), to: hourName(g.to_hour), days: g.days });
  return (
    <div className="notice notice-info" role="status">
      <span>{text}</span>
      <div className="row">
        <button
          type="button"
          className="btn btn-small btn-primary"
          onClick={() => props.run(async () => { await backend.answerSuggestion(g.id, true); return t("suggest_done"); })}
        >
          {t("suggest_yes")}
        </button>
        <button type="button" className="btn btn-small" onClick={() => props.run(() => backend.answerSuggestion(g.id, false))}>
          {t("suggest_no")}
        </button>
      </div>
    </div>
  );
}

/** "2026-03" as "March 2026". */
function monthName(key: string): string {
  const [y, m] = key.split("-").map(Number);
  if (!y || !m) return key;
  return new Date(y, m - 1, 1).toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

function slowSummary(r: FeelsSlowResult): string {
  const parts: string[] = [];
  if (r.mode_was !== r.mode_now) parts.push(t("slow_did_mode", { from: profileName(r.mode_was), to: profileName(r.mode_now) }));
  if (r.learned.length > 0) parts.push(t("slow_did_learned"));
  if (r.apps_slowed) parts.push(t("slow_did_apps"));
  return parts.length ? t("slow_did", { list: parts.join("; ") }) : t("slow_did_nothing");
}

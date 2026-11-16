import { useState } from "react";
import { backend, type Predict, type Status } from "../backend";
import { Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { clock, t } from "../i18n";

export function Battery(props: { status: Status; refresh: () => void }) {
  const p = props.status.power;
  const settings = useLoad(() => backend.settings(), []);
  const limitRow = settings.data?.find((r) => r.id === "battery.charge_limit");
  const [limit, setLimit] = useState<number>(p.charge_limit ?? 80);
  const predict = useLoad(() => backend.predict(), []);
  const { run, toast } = useAction(() => {
    props.refresh();
    predict.reload();
  });
  const d = props.status.display;
  const pr = predict.data;

  return (
    <>
      <header className="page-head">
        <h1>{t("battery_title")}</h1>
      </header>
      <section className="card">
        <dl className="stack" style={{ margin: 0 }}>
          <Row term={t("battery_health")} value={p.health_pct != null ? t("battery_health_value", { pct: Math.round(p.health_pct) }) : t("unknown")} />
          <Row
            term={t("battery_capacity")}
            value={p.full_wh != null && p.design_wh != null ? t("battery_capacity_value", { full: p.full_wh.toFixed(1), design: p.design_wh.toFixed(1) }) : t("unknown")}
          />
          <Row term={t("battery_cycles")} value={p.cycles != null ? String(p.cycles) : t("unknown")} />
        </dl>
      </section>
      {pr && (
        <section className="card" aria-labelledby="forecast-heading">
          <h2 id="forecast-heading">{t("battery_forecast")}</h2>
          <p>{pr.forecast_end ? t("battery_forecast_value", { time: clock(pr.forecast_end) }) : t("battery_forecast_learning")}</p>
          {pr.usual_unplug && <p className="muted">{t("battery_usual_unplug", { time: clock(pr.usual_unplug) })}</p>}
        </section>
      )}
      {pr && p.has_battery && <LastUntil predict={pr} locked={!!props.status.safe_mode} run={run} />}
      {pr?.charge_supported && (
        <section className="card" aria-labelledby="charge-heading">
          <h2 id="charge-heading">{t("charge_heading")}</h2>
          <Switch
            checked={pr.adaptive_charging}
            disabled={!!props.status.safe_mode}
            label={t("charge_follow")}
            onChange={(on) => run(() => backend.setAdaptiveCharging(on))}
          />
          <p className="muted">{t("charge_follow_desc")}</p>
          {pr.charge && (
            <p>{pr.charge.until ? t("charge_until", { reason: pr.charge.reason, time: clock(pr.charge.until) }) : pr.charge.reason}</p>
          )}
        </section>
      )}
      <section className="card" aria-labelledby="limit-heading">
        <h2 id="limit-heading">{t("battery_limit")}</h2>
        {limitRow?.available && pr?.adaptive_charging ? (
          <p className="muted">{t("charge_limit_by_routine")}</p>
        ) : limitRow?.available ? (
          <>
            <p className="muted">{t("battery_limit_desc")}</p>
            {props.status.kept?.settings.some((k) => k.id === "battery.charge_limit") && <p className="muted small">{t("battery_limit_kept")}</p>}
            {props.status.kept?.charge_limit_ignored && <p className="notice">{t("battery_limit_ignored")}</p>}
            <div className="row">
              <label className="field" style={{ flex: 1 }}>
                <span className="field-label">{limit}%</span>
                <input type="range" min={50} max={100} step={5} value={limit} onChange={(e) => setLimit(Number(e.target.value))} aria-label={t("battery_limit")} />
              </label>
              <button type="button" className="btn btn-primary" onClick={() => run(() => backend.setSetting("battery.charge_limit", String(limit), false))}>
                {t("change_apply")}
              </button>
            </div>
          </>
        ) : (
          <p className="muted">{t("battery_limit_none")}</p>
        )}
      </section>
      <section className="card" aria-labelledby="screen-heading">
        <h2 id="screen-heading">{t("battery_screen")}</h2>
        <p>{d ? t("battery_screen_value", { w: (d.w_per_pct * 10).toFixed(2) }) : t("battery_screen_learning")}</p>
      </section>
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

function Row(props: { term: string; value: string }) {
  return (
    <div className="row spread">
      <dt className="muted">{props.term}</dt>
      <dd style={{ margin: 0, fontWeight: 600 }}>{props.value}</dd>
    </div>
  );
}

function LastUntil(props: { predict: Predict; locked: boolean; run: (a: () => Promise<string | void>) => void }) {
  const target = props.predict.target;
  const [time, setTime] = useState("");
  const inHours = (h: number) => Date.now() / 1000 + h * 3600;
  /** "17:30" today, or tomorrow if that time has passed. */
  const fromClock = (hhmm: string) => {
    const [h, m] = hhmm.split(":").map(Number);
    const d = new Date();
    d.setHours(h, m, 0, 0);
    if (d.getTime() < Date.now()) d.setDate(d.getDate() + 1);
    return d.getTime() / 1000;
  };
  return (
    <section className="card" aria-labelledby="lu-heading">
      <h2 id="lu-heading">{t("lu_heading")}</h2>
      {target ? (
        <>
          <p><strong>{t("lu_active", { time: clock(target.until) })}</strong></p>
          {target.reason && <p>{target.reason}. {t(`lu_level_${target.level}` as "lu_level_normal")}</p>}
          {target.budget_w != null && target.recent_w != null && (
            <p className="muted small">{t("lu_budget", { budget: target.budget_w.toFixed(1), recent: target.recent_w.toFixed(1) })}</p>
          )}
          <div>
            <button type="button" className="btn" onClick={() => props.run(() => backend.setTarget(0))}>{t("lu_stop")}</button>
          </div>
        </>
      ) : (
        <>
          <p className="muted">{t("lu_desc")}</p>
          {props.predict.target_ended && <p className="muted small">{t("lu_ended", { why: props.predict.target_ended })}</p>}
          <div className="chips" role="group" aria-label={t("lu_heading")}>
            {[2, 4, 6, 8].map((h) => (
              <button key={h} type="button" className="chip" disabled={props.locked} onClick={() => props.run(() => backend.setTarget(inHours(h)))}>
                {t("lu_for_hours", { h })}
              </button>
            ))}
          </div>
          <div className="row">
            <label className="field" style={{ marginTop: 0 }}>
              <span className="field-label">{t("lu_until_label")}</span>
              <input type="time" value={time} onChange={(e) => setTime(e.target.value)} />
            </label>
            <button
              type="button"
              className="btn btn-primary"
              disabled={!time || props.locked}
              style={{ alignSelf: "flex-end" }}
              onClick={() => props.run(() => backend.setTarget(fromClock(time)))}
            >
              {t("lu_start")}
            </button>
          </div>
        </>
      )}
    </section>
  );
}

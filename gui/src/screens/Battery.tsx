import { useEffect, useState } from "react";
import { backend, type Predict, type Sensors, type Status } from "../backend";
import { Dialog, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { clock, t } from "../i18n";
import { powerSplit } from "../power";

export function Battery(props: { status: Status; refresh: () => void; go?: (screen: string) => void }) {
  const p = props.status.power;
  const settings = useLoad(() => backend.settings(), []);
  const limitRow = settings.data?.find((r) => r.id === "battery.charge_limit");
  const [limit, setLimit] = useState<number>(p.charge_limit ?? 80);
  const predict = useLoad(() => backend.predict(), []);
  const { run, toast } = useAction(() => {
    props.refresh();
    predict.reload();
  });
  const fc = props.status.full_charge;
  const pr = predict.data;

  return (
    <>
      <header className="page-head">
        <h1>{t("battery_title")}</h1>
      </header>
      <div className="grid grid-2">
      <section className="card" aria-labelledby="health-heading">
        <h2 id="health-heading">{t("battery_health_heading")}</h2>
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
      </div>
      <section className="card" aria-labelledby="limit-heading">
        <h2 id="limit-heading">{t("battery_limit")}</h2>
        {limitRow?.managed_by ? (
          <p className="muted">{t("battery_limit_elsewhere", { tool: limitRow.managed_by })}</p>
        ) : limitRow?.available && pr?.adaptive_charging ? (
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
        {limitRow?.available && !limitRow.managed_by && (
          <>
            {fc?.active ? (
              <div className="notice notice-info">
                <span>{t("full_charge_on", { pct: fc.restore_to ?? "" })}</span>
                <button type="button" className="btn btn-small" onClick={() => run(() => backend.chargeFullyOnce(false))}>
                  {t("full_charge_stop")}
                </button>
              </div>
            ) : (
              fc?.supported &&
              p.charge_limit != null &&
              p.charge_limit < 100 && (
                <div className="row">
                  <button type="button" className="btn" disabled={!!props.status.safe_mode} onClick={() => run(() => backend.chargeFullyOnce(true))}>
                    {t("full_charge_button")}
                  </button>
                  <span className="muted small">{t("full_charge_desc")}</span>
                </div>
              )
            )}
            {!fc?.active && fc?.ended && <p className="muted small">{t("full_charge_ended", { why: fc.ended })}</p>}
          </>
        )}
      </section>
      <Recalibrate status={props.status} run={run} />
      <PowerUse status={props.status} go={props.go} />
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

/** What uses the battery: screen, processor, graphics and the rest,
 * measured while this page is open. */
function PowerUse(props: { status: Status; go?: (screen: string) => void }) {
  const [reads, setReads] = useState<Sensors[]>([]);
  useEffect(() => {
    let live = true;
    const read = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const s = await backend.sensors();
        if (live) setReads((r) => [...r.slice(-1), s]);
      } catch {
        // Shown as still measuring.
      }
    };
    read();
    const id = window.setInterval(read, 3000);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, []);
  const d = props.status.display;
  const [a, b] = reads;
  const split = a && b ? powerSplit(a, b, d) : null;
  const top = split ? Math.max(split.total ?? 0, ...split.parts.map((x) => x.w)) : 0;
  const desc = (key: string) =>
    key === "screen" && d && b?.brightness_pct != null
      ? t("use_screen_desc", { pct: Math.round(b.brightness_pct), w: (d.w_per_pct * 10).toFixed(2) })
      : t(`use_${key}_desc` as "use_rest_desc");
  return (
    <section className="card" aria-labelledby="use-heading">
      <div className="row spread">
        <h2 id="use-heading">{t("use_heading")}</h2>
        {split?.total != null && <strong>{t("use_total", { w: split.total.toFixed(1) })}</strong>}
      </div>
      <p className="muted small">{t("use_desc")}</p>
      {!split ? (
        <p className="muted" role="status">{t("use_measuring")}</p>
      ) : (
        <ul className="use-list">
          {split.parts.map((x) => (
            <li key={x.key}>
              <div className="row spread">
                <span>
                  <strong>{t(`use_${x.key}` as "use_rest")}</strong>
                  <span className="muted small use-desc">{desc(x.key)}</span>
                </span>
                <span className="use-w">
                  {t("use_watts", { w: x.w.toFixed(2) })}
                  {split.total != null && <span className="muted small"> · {t("use_share", { pct: Math.round((x.w / split.total) * 100) })}</span>}
                </span>
              </div>
              <div className="bar" aria-hidden="true">
                <span style={{ width: `${top > 0 ? Math.min(100, (x.w / top) * 100) : 0}%` }} />
              </div>
            </li>
          ))}
        </ul>
      )}
      {!d && <p className="muted small">{t("use_screen_learning")}</p>}
      {b?.on_ac && <p className="muted small">{t("use_plugged")}</p>}
      {props.go && (
        <div>
          <button type="button" className="btn-link" onClick={() => props.go?.("modes")}>
            {t("use_brightness_link")}
          </button>
        </div>
      )}
    </section>
  );
}

/** Battery recalibration (decision 0019): only where the battery can run
 * the laptop while plugged in. */
function Recalibrate(props: { status: Status; run: (a: () => Promise<string | void>) => void }) {
  const info = useLoad(() => backend.recalibration(), []);
  const [asking, setAsking] = useState(false);
  const r = info.data;
  useEffect(() => {
    if (!r?.phase) return;
    const id = window.setInterval(info.reload, 3000);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [r?.phase]);
  if (!r?.supported) return null;
  const p = props.status.power;
  const ready = p.on_ac && (p.battery_pct ?? 0) > r.min_start_pct && !props.status.safe_mode;
  const act = (a: () => Promise<void>) =>
    props.run(async () => {
      await a();
      info.reload();
    });
  return (
    <section className="card" aria-labelledby="recal-heading">
      <h2 id="recal-heading">{t("recal_heading")}</h2>
      <p className="muted">{t("recal_desc", { floor: r.floor_pct })}</p>
      {r.phase ? (
        <div className="notice notice-info" role="status">
          <span>
            {r.phase === "running_down"
              ? t("recal_down", { pct: r.battery_pct ?? "?", floor: r.floor_pct })
              : t("recal_charging", { pct: r.battery_pct ?? "?" })}
          </span>
          <button type="button" className="btn btn-small" onClick={() => act(() => backend.stopRecalibration())}>
            {t("recal_stop")}
          </button>
        </div>
      ) : (
        <>
          {r.ended === "done" &&
            (r.full_before_wh != null && r.full_after_wh != null ? (
              <p>{t("recal_done", { before: r.full_before_wh.toFixed(1), after: r.full_after_wh.toFixed(1) })}</p>
            ) : (
              <p>{t("recal_done_plain")}</p>
            ))}
          {r.ended && r.ended !== "done" && <p className="notice">{t("recal_ended", { why: r.ended })}</p>}
          <div className="row">
            <button type="button" className="btn" disabled={!ready} onClick={() => setAsking(true)}>
              {t("recal_start")}
            </button>
            {!ready && <span className="muted small">{t("recal_needs", { pct: r.min_start_pct })}</span>}
          </div>
        </>
      )}
      <Dialog
        open={asking}
        onClose={() => setAsking(false)}
        title={t("recal_confirm_title")}
        actions={
          <>
            <button type="button" className="btn" onClick={() => setAsking(false)}>{t("cancel")}</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => {
                setAsking(false);
                act(() => backend.startRecalibration());
              }}
            >
              {t("recal_confirm")}
            </button>
          </>
        }
      >
        <p>{t("recal_confirm_desc", { floor: r.floor_pct })}</p>
      </Dialog>
    </section>
  );
}

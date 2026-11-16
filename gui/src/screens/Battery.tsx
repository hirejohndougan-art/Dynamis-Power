import { useState } from "react";
import { backend, type Status } from "../backend";
import { useAction, useLoad } from "../hooks";
import { t } from "../i18n";

export function Battery(props: { status: Status; refresh: () => void }) {
  const p = props.status.power;
  const settings = useLoad(() => backend.settings(), []);
  const limitRow = settings.data?.find((r) => r.id === "battery.charge_limit");
  const [limit, setLimit] = useState<number>(p.charge_limit ?? 80);
  const { run, toast } = useAction(props.refresh);
  const d = props.status.display;

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
      <section className="card" aria-labelledby="limit-heading">
        <h2 id="limit-heading">{t("battery_limit")}</h2>
        {limitRow?.available ? (
          <>
            <p className="muted">{t("battery_limit_desc")}</p>
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

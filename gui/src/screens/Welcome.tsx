// First run: four steps, and nothing changes until Start.
import { useEffect, useRef, useState } from "react";
import { backend, type Conflict, type SettingRow, type Status } from "../backend";
import { Logo, Switch } from "../components";
import { useLoad } from "../hooks";
import { t, tn } from "../i18n";

const KEY = "dynamis.welcomed";

/** Whether the welcome has been completed (per person, in this app). */
export function welcomed(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return true; // storage unavailable: never block the app behind the welcome
  }
}

function markWelcomed() {
  try {
    localStorage.setItem(KEY, "1");
  } catch {
    /* nothing to do */
  }
}

export function Welcome(props: { status: Status; onDone: () => void }) {
  const [step, setStep] = useState(1);
  const [auto, setAuto] = useState(true);
  const [learn, setLearn] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const settings = useLoad(() => backend.settings(), []);
  const inventory = useLoad(() => backend.inventory() as Promise<{ machine?: { vendor?: string; model?: string } }>, []);
  const conflicts = useLoad(() => backend.conflicts(), []);
  const total = 4;

  useEffect(() => heading.current?.focus(), [step]);

  const finish = async (apply: boolean) => {
    setError(null);
    try {
      if (apply) {
        await backend.setAuto(auto);
        await backend.setLearning(learn);
      }
      markWelcomed();
      props.onDone();
    } catch (e) {
      setError(String((e as Error)?.message ?? e));
    }
  };

  return (
    <div className="welcome">
      <div className="card welcome-card">
        <div className="row spread">
          <div className="row">
            <Logo />
            <span className="muted small">{t("welcome_step", { n: step, total })}</span>
          </div>
          {step < total && (
            <button type="button" className="btn-link" onClick={() => finish(false)}>
              {t("skip")}
            </button>
          )}
        </div>
        <h1 ref={heading} tabIndex={-1}>{t(`welcome_title_${step}` as "welcome_title_1")}</h1>

        {step === 1 && (
          <div className="stack">
            <p>{t("welcome_body_1a")}</p>
            <p>{t("welcome_body_1b")}</p>
            <p>{t("welcome_body_1c")}</p>
            <p className="muted">{t("welcome_nothing_yet")}</p>
          </div>
        )}
        {step === 2 && <Check status={props.status} rows={settings.data ?? []} model={inventory.data?.machine} />}
        {step === 3 && <Conflicts list={conflicts.data} reload={conflicts.reload} />}
        {step === 4 && (
          <div className="stack">
            <Switch checked={auto} onChange={setAuto} label={t("mode_auto")} />
            <p className="muted small">{t("welcome_auto_desc")}</p>
            <Switch checked={learn} onChange={setLearn} label={t("learn_switch")} />
            <p className="muted small">{t("welcome_learn_desc")}</p>
            <p className="muted small">{t("welcome_change_later")}</p>
          </div>
        )}

        {error && <p role="alert" className="notice">{error}</p>}
        <div className="dialog-actions">
          {step > 1 && (
            <button type="button" className="btn" onClick={() => setStep(step - 1)}>
              {t("back")}
            </button>
          )}
          {step < total ? (
            <button type="button" className="btn btn-primary" onClick={() => setStep(step + 1)}>
              {t("next")}
            </button>
          ) : (
            <button type="button" className="btn btn-primary" onClick={() => finish(true)}>
              {t("start")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function Check(props: { status: Status; rows: SettingRow[]; model?: { vendor?: string; model?: string } }) {
  const p = props.status.power;
  const available = props.rows.filter((r) => r.available).length;
  const restricted = props.rows.filter((r) => r.not_allowed.length > 0).length;
  return (
    <div className="stack">
      <dl className="stack" style={{ margin: 0 }}>
        <div className="row spread">
          <dt className="muted">{t("welcome_model")}</dt>
          <dd style={{ margin: 0, fontWeight: 600 }}>{[props.model?.vendor, props.model?.model].filter(Boolean).join(" ") || t("unknown")}</dd>
        </div>
        {p.has_battery && (
          <div className="row spread">
            <dt className="muted">{t("welcome_battery")}</dt>
            <dd style={{ margin: 0, fontWeight: 600 }}>
              {p.health_pct != null ? t("welcome_battery_value", { pct: Math.round(p.health_pct) }) : t("unknown")}
            </dd>
          </div>
        )}
        <div className="row spread">
          <dt className="muted">{t("welcome_settings")}</dt>
          <dd style={{ margin: 0, fontWeight: 600 }}>{t("welcome_settings_value", { n: available, total: props.rows.length })}</dd>
        </div>
      </dl>
      {!p.has_battery && <p className="notice">{t("welcome_no_battery")}</p>}
      {restricted > 0 && <p className="muted small">{tn("welcome_restricted", restricted)}</p>}
    </div>
  );
}

function Conflicts(props: { list: Conflict[] | null; reload: () => void }) {
  const [error, setError] = useState<string | null>(null);
  if (props.list === null) return <p className="muted">{t("loading")}</p>;
  if (props.list.length === 0) return <p>{t("welcome_conflicts_none")}</p>;
  return (
    <div className="stack">
      <p>{t("welcome_conflicts_intro")}</p>
      <ul className="entries">
        {props.list.map((c) => (
          <li key={c.unit} className="entry">
            <div className="stack">
              <span>{c.name}</span>
              <span className="muted small">{c.active ? t("welcome_conflict_running") : t("welcome_conflict_enabled")}</span>
            </div>
            <button
              type="button"
              className="btn btn-small"
              aria-label={`${t("welcome_switch_off")}: ${c.name}`}
              onClick={async () => {
                setError(null);
                try {
                  await backend.switchOffConflict(c.unit);
                  props.reload();
                } catch (e) {
                  setError(String((e as Error)?.message ?? e));
                }
              }}
            >
              {t("welcome_switch_off")}
            </button>
          </li>
        ))}
      </ul>
      <p className="muted small">{t("welcome_switch_off_note")}</p>
      {error && <p role="alert" className="notice">{error}</p>}
    </div>
  );
}

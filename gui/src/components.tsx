// Small accessible building blocks shared by the screens.
import { useEffect, useRef, type ReactNode } from "react";
import { t } from "./i18n";

export function Switch(props: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      className="switch"
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
    >
      <span className="switch-track" aria-hidden="true">
        <span className="switch-thumb" />
      </span>
      <span>{props.label}</span>
    </button>
  );
}

/** A dialog that opens as a modal, traps focus (natively) and closes on Escape. */
export function Dialog(props: { open: boolean; onClose: () => void; title: string; children: ReactNode; actions: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (props.open && !d.open) d.showModal?.() ?? d.setAttribute("open", "");
    if (!props.open && d.open) d.close?.() ?? d.removeAttribute("open");
  }, [props.open]);
  return (
    <dialog ref={ref} aria-labelledby="dialog-title" onClose={props.onClose} onCancel={props.onClose}>
      {props.open && (
        <>
          <h2 id="dialog-title">{props.title}</h2>
          {props.children}
          <div className="dialog-actions">{props.actions}</div>
        </>
      )}
    </dialog>
  );
}

export function Toast(props: { message: string | null }) {
  return (
    <div role="status" aria-live="polite">
      {props.message && <div className="toast">{props.message}</div>}
    </div>
  );
}

/** A tiny line chart. Purely visual; the numbers are also given as text. */
export function Sparkline(props: { values: number[]; label: string }) {
  const v = props.values;
  if (v.length < 2) return null;
  const w = 240, h = 48, pad = 3;
  const min = Math.min(...v), max = Math.max(...v);
  const span = max - min || 1;
  const pts = v.map((y, i) => `${pad + (i / (v.length - 1)) * (w - 2 * pad)},${h - pad - ((y - min) / span) * (h - 2 * pad)}`).join(" ");
  return (
    <svg width="100%" height={h} viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" role="img" aria-label={props.label}>
      <polyline points={pts} fill="none" stroke="var(--accent)" strokeWidth="2" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
    </svg>
  );
}

/** A step for the chart's watt lines: 0.5, 1, 2 or 5 times a power of ten,
 * giving about four lines. */
function niceStep(max: number) {
  const raw = max / 4;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  return [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
}

/** Power use over time, with grid lines: watts up the side (from zero, so
 * the height is honest) and minutes along the bottom. `values` are readings
 * `stepS` seconds apart, oldest first. */
export function PowerChart(props: { values: number[]; stepS: number; label: string; empty?: string }) {
  const v = props.values;
  const max = Math.max(1, ...v) * 1.1;
  const step = niceStep(max);
  const top = Math.ceil(max / step) * step;
  const lines: number[] = [];
  for (let y = 0; y <= top + 1e-9; y += step) lines.push(Math.round(y * 100) / 100);
  const W = 1000;
  const H = 180;
  const spanS = Math.max((v.length - 1) * props.stepS, 0);
  const x = (i: number) => (v.length < 2 ? W : (i / (v.length - 1)) * W);
  const y = (w: number) => H - (w / top) * H;
  const pts = v.map((w, i) => `${x(i).toFixed(1)},${y(w).toFixed(1)}`).join(" ");
  // Minute marks along the bottom: every 1, 2, 5 or 10 minutes.
  const spanMin = spanS / 60;
  const every = [1, 2, 5, 10, 15, 30].find((m) => spanMin / m <= 6) ?? 30;
  const ticks: number[] = [];
  for (let m = every; m < spanMin - every * 0.3; m += every) ticks.push(m);
  const unit = step < 1 ? 1 : 0;
  return (
    <div className="chart">
      <div className="chart-y" aria-hidden="true">
        {lines.map((w) => (
          <span key={w} style={{ top: `${(y(w) / H) * 100}%` }}>
            {t("chart_w", { w: w.toFixed(unit) })}
          </span>
        ))}
      </div>
      <div className="chart-plot" role="img" aria-label={props.label}>
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
          <defs>
            <linearGradient id="chart-fade" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" className="chart-fade-top" />
              <stop offset="100%" className="chart-fade-bottom" />
            </linearGradient>
          </defs>
          {lines.slice(1).map((w) => (
            <line key={w} className="chart-grid" x1={0} x2={W} y1={y(w)} y2={y(w)} vectorEffect="non-scaling-stroke" />
          ))}
          {ticks.map((m) => {
            const xx = W - (m * 60 / spanS) * W;
            return <line key={m} className="chart-grid" x1={xx} x2={xx} y1={0} y2={H} vectorEffect="non-scaling-stroke" />;
          })}
          {v.length > 1 && (
            <>
              <polygon className="chart-area" fill="url(#chart-fade)" points={`0,${H} ${pts} ${W},${H}`} />
              <polyline className="chart-line" points={pts} vectorEffect="non-scaling-stroke" />
            </>
          )}
        </svg>
        {v.length < 2 && props.empty && <div className="chart-empty">{props.empty}</div>}
      </div>
      <div className="chart-x" aria-hidden="true">
        {v.length > 1 && <span style={{ left: "0%" }}>{t("chart_ago", { m: Math.max(1, Math.round(spanMin)) })}</span>}
        {ticks
          .filter((m) => m < spanMin * 0.85)
          .map((m) => (
            <span key={m} style={{ left: `${100 - (m * 60 * 100) / spanS}%` }}>
              {t("chart_ago", { m })}
            </span>
          ))}
        <span style={{ left: "100%" }}>{t("chart_now")}</span>
      </div>
    </div>
  );
}

/** Pick one of a few values. Every choice is visible at once, as buttons
 * (real radio inputs underneath, so keyboards and screen readers work).
 * Used instead of drop-down lists, which some embedded views cannot open. */
export function Choice<T extends string | number>(props: {
  label: string;
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  disabled?: boolean;
  name: string;
}) {
  return (
    <fieldset className="field choice">
      <legend className="field-label">{props.label}</legend>
      <div className="chips">
        {props.options.map(([v, text]) => (
          <label key={String(v)} className={`chip choice-chip${props.value === v ? " on" : ""}`}>
            <input
              type="radio"
              name={props.name}
              value={String(v)}
              checked={props.value === v}
              disabled={props.disabled}
              onChange={() => props.onChange(v)}
            />
            {text}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** A progress bar with its value for screen readers. */
export function ProgressBar(props: { pct: number; label?: string }) {
  const pct = Math.max(0, Math.min(100, props.pct));
  return (
    <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={props.label ?? t("learn_bar")}>
      <span style={{ width: `${pct}%` }} />
    </div>
  );
}

export function Logo() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true">
      <rect x="1" y="1" width="26" height="26" rx="8" fill="var(--accent)" />
      <path d="M15.5 6 9 15.5h5l-1.5 6.5L19 12.5h-5z" fill="var(--on-accent)" />
    </svg>
  );
}

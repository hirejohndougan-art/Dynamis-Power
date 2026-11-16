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

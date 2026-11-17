// "Something stopped working" (everyday problem §1): undo the last hour,
// then ask what stopped working and block only what could cause it.
import { useEffect, useState } from "react";
import { backend, type Status, type TroubleKind } from "../backend";
import { Choice, Dialog } from "../components";
import { t, tn, type Key } from "../i18n";

type Result = { kind: string; blocked: { id: string; label: string }[] };

export function useTrouble(s: Status, run: (a: () => Promise<string | void>) => void) {
  // Closed by hand ("Later") while the question is still open.
  const [later, setLater] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  // A new question (from the panel menu, or the button) opens the dialog.
  useEffect(() => {
    if (s.trouble?.at) setLater(false);
  }, [s.trouble?.at]);
  const start = () => run(async () => { setResult(null); await backend.somethingBroke(); });
  const dialog = (!!s.trouble && !later) || result !== null;
  return {
    button: (
      <button type="button" className="btn" onClick={start} disabled={!!s.trouble}>
        {t("trouble_button")}
      </button>
    ),
    notice: s.trouble && later && (
      <div className="notice" role="status">
        <span>{t("trouble_waiting")}</span>
        <button type="button" className="btn btn-small" onClick={() => setLater(false)}>
          {t("trouble_answer")}
        </button>
      </div>
    ),
    dialog: dialog && (
      <TroubleDialog
        s={s}
        result={result}
        onLater={() => setLater(true)}
        onClose={() => setResult(null)}
        answer={(kind) =>
          run(async () => {
            const r = await backend.whatBroke(kind);
            if (kind !== "none") setResult(r);
            return kind === "none" ? t("trouble_none_done") : undefined;
          })
        }
      />
    ),
  };
}

function TroubleDialog(props: {
  s: Status;
  result: Result | null;
  onLater: () => void;
  onClose: () => void;
  answer: (kind: TroubleKind | "none") => void;
}) {
  const [kind, setKind] = useState<TroubleKind | null>(null);
  const tr = props.s.trouble;
  if (props.result) {
    const r = props.result;
    const what = t(`trouble_kind_${r.kind}` as Key);
    return (
      <Dialog
        open
        onClose={props.onClose}
        title={t("trouble_result_title")}
        actions={<button type="button" className="btn btn-primary" onClick={props.onClose}>{t("close")}</button>}
      >
        {r.blocked.length > 0 ? (
          <>
            <p>{t("trouble_blocked", { what })}</p>
            <ul>
              {r.blocked.map((b) => <li key={b.id}>{b.label}</li>)}
            </ul>
            <p className="muted small">{t("trouble_blocked_after")}</p>
          </>
        ) : (
          <p>{t("trouble_not_ours", { what })}</p>
        )}
      </Dialog>
    );
  }
  if (!tr) return null;
  return (
    <Dialog
      open
      onClose={props.onLater}
      title={t("trouble_title")}
      actions={
        <>
          <button type="button" className="btn" onClick={() => props.answer("none")}>{t("trouble_fine")}</button>
          <button type="button" className="btn" onClick={props.onLater}>{t("trouble_later")}</button>
          <button type="button" className="btn btn-primary" disabled={!kind} onClick={() => kind && props.answer(kind)}>
            {t("trouble_send")}
          </button>
        </>
      }
    >
      {tr.undone.length > 0 ? (
        <>
          <p>{tn("trouble_undone", tr.undone.length)}</p>
          <ul className="small">
            {tr.undone.map((u) => <li key={u.id}>{u.label}</li>)}
          </ul>
        </>
      ) : (
        <p>{t("trouble_undone_none")}</p>
      )}
      <Choice
        name="trouble-kind"
        label={t("trouble_question")}
        value={kind ?? ("" as TroubleKind)}
        options={tr.kinds.map((k): [TroubleKind, string] => [k, t(`trouble_kind_${k}` as Key)])}
        onChange={setKind}
      />
      <p className="muted small">{t("trouble_hold")}</p>
    </Dialog>
  );
}

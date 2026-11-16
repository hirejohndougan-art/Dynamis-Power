import { backend, type DeviceInfo, type DevicesInfo, type Status } from "../backend";
import { Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { t, tn } from "../i18n";

/** Devices: which USB devices and built-in hardware may sleep when idle on
 * battery, every device listed with its name and why some stay awake. */
export function Devices(props: { status: Status; refresh: () => void }) {
  const { run, toast } = useAction(props.refresh);
  return (
    <>
      <header className="page-head">
        <h1>{t("dev_heading")}</h1>
      </header>
      <p className="muted">{t("dev_intro")}</p>
      <DeviceLists locked={!!props.status.safe_mode} run={run} />
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

/** Letting idle USB devices and internal hardware sleep on battery. */
function DeviceLists(props: { locked: boolean; run: (a: () => Promise<string | void>) => void }) {
  const info = useLoad<DevicesInfo>(() => backend.devices(), []);
  const d = info.data;
  if (!d || d.devices.length === 0) return null;
  const set = (p: DevicesInfo["policy"]) =>
    props.run(async () => {
      await backend.setDevicePolicy(p);
      info.reload();
    });
  const what = (x: DeviceInfo) => t(`dev_what_${x.what}` as "dev_what_other");
  const name = (x: DeviceInfo) => x.name || what(x);
  const usb = d.devices.filter((x) => x.kind === "usb");
  const pci = d.devices.filter((x) => x.kind === "pci");
  const kind = (k: "usb" | "pci", list: DeviceInfo[]) => {
    const blocked = d.blocked[k];
    return (
      <section className="card" aria-labelledby={`dev-${k}-heading`}>
        <h2 id={`dev-${k}-heading`}>{t(k === "usb" ? "dev_usb_heading" : "dev_pci_heading")}</h2>
        <Switch
          checked={d.policy[k]}
          disabled={props.locked || !!blocked}
          label={t(k === "usb" ? "dev_usb_switch" : "dev_pci_switch")}
          onChange={(on) => set({ ...d.policy, [k]: on })}
        />
        <p className="muted small">{blocked ? t("dev_blocked", { why: blocked }) : t(k === "usb" ? "dev_usb_desc" : "dev_pci_desc")}</p>
        <ul className="entries" aria-label={tn("dev_count", list.length, { n: list.length })}>
            {list.map((x) => (
              <li key={x.id} className="entry">
                <div className="stack" style={{ gap: 2 }}>
                  <span>{name(x)}</span>
                  <span className="muted small">
                    {x.name && x.kind === "pci" ? `${what(x)} · ` : ""}
                    {x.address}
                    {" · "}
                    {x.excluded
                      ? t(`dev_keep_${x.excluded}` as "dev_keep_input")
                      : x.kept_awake
                        ? t("dev_kept_awake")
                        : x.sleeps
                          ? t("dev_sleeps")
                          : t("dev_awake")}
                  </span>
                </div>
                <div className="row" style={{ gap: 8 }}>
                  {x.managed && <span className="badge badge-safe">{t("dev_managed")}</span>}
                  {!x.excluded && x.model && (
                    <button
                      type="button"
                      className="btn btn-small"
                      aria-pressed={x.kept_awake}
                      aria-label={`${t("dev_keep_awake")}: ${name(x)}`}
                      disabled={props.locked}
                      onClick={() => {
                        const never = new Set(d.policy.never ?? []);
                        if (x.kept_awake) never.delete(x.model!);
                        else never.add(x.model!);
                        set({ ...d.policy, never: [...never] });
                      }}
                    >
                      {t("dev_keep_awake")}
                    </button>
                  )}
                </div>
              </li>
            ))}
        </ul>
      </section>
    );
  };
  return (
    <>
      {usb.length > 0 && kind("usb", usb)}
      {pci.length > 0 && kind("pci", pci)}
    </>
  );
}


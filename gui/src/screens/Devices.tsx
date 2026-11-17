import { backend, type DeviceInfo, type DevicesInfo, type RadioKind, type RadiosInfo, type Status } from "../backend";
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
      <Radios locked={!!props.status.safe_mode} hasBattery={props.status.power.has_battery} run={run} />
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


/** Radios switched off by rule (TLP's DEVICES_TO_DISABLE_ON_*). Rules act
 * when the situation changes; switching a radio back on yourself is
 * respected until the next change. */
function Radios(props: { locked: boolean; hasBattery: boolean; run: (a: () => Promise<string | void>) => void }) {
  const info = useLoad<RadiosInfo>(() => backend.radios(), []);
  const d = info.data;
  if (!d || (d.radios.length === 0 && !d.keyboard_lights)) return null;
  const kinds = Array.from(new Set(d.radios.map((r) => r.kind)));
  const set = (p: RadiosInfo["policy"]) =>
    props.run(async () => {
      await backend.setRadioPolicy(p);
      info.reload();
    });
  const toggle = (list: RadioKind[], k: RadioKind, on: boolean) => (on ? [...list.filter((x) => x !== k), k] : list.filter((x) => x !== k));
  const kindName = (k: RadioKind) => t(`radio_${k}`);
  return (
    <section className="card" aria-labelledby="radios-heading">
      <h2 id="radios-heading">{t("radio_heading")}</h2>
      <p className="muted">{t("radio_intro")}</p>
      <div className="stack" style={{ gap: 8 }}>
        {props.hasBattery &&
          kinds
            .filter((k) => k !== "wlan")
            .map((k) => (
              <Switch
                key={`bat-${k}`}
                checked={d.policy.off_on_battery.includes(k)}
                disabled={props.locked}
                label={t("radio_off_on_battery", { radio: kindName(k) })}
                onChange={(on) => set({ ...d.policy, off_on_battery: toggle(d.policy.off_on_battery, k, on) })}
              />
            ))}
        {props.hasBattery && (d.keyboard_lights ?? 0) > 0 && (
          <Switch
            checked={!!d.policy.keyboard_light_off_on_battery}
            disabled={props.locked}
            label={t("radio_keyboard_off_on_battery")}
            onChange={(on) => set({ ...d.policy, keyboard_light_off_on_battery: on })}
          />
        )}
        {kinds.includes("wlan") && (
          <Switch
            checked={d.policy.wifi_off_when_wired}
            disabled={props.locked}
            label={t("radio_wifi_wired")}
            onChange={(on) => set({ ...d.policy, wifi_off_when_wired: on })}
          />
        )}
        {kinds.map((k) => (
          <Switch
            key={`dock-${k}`}
            checked={(d.policy.off_when_docked ?? []).includes(k)}
            disabled={props.locked}
            label={t("radio_off_when_docked", { radio: kindName(k) })}
            onChange={(on) => set({ ...d.policy, off_when_docked: toggle(d.policy.off_when_docked ?? [], k, on) })}
          />
        ))}
        {kinds
          .filter((k) => k !== "wlan")
          .map((k) => (
            <Switch
              key={`start-${k}`}
              checked={d.policy.off_at_startup.includes(k)}
              disabled={props.locked}
              label={t("radio_off_at_startup", { radio: kindName(k) })}
              onChange={(on) => set({ ...d.policy, off_at_startup: toggle(d.policy.off_at_startup, k, on) })}
            />
          ))}
      </div>
      {d.docked && <p className="muted small">{t("radio_docked_now")}</p>}
      <ul className="entries" aria-label={t("radio_now")}>
        {d.radios.map((r) => (
          <li key={r.id} className="entry">
            <span>{kindName(r.kind)}</span>
            <span className="muted small">
              {r.hard_off ? t("radio_hard_off") : r.off ? t("radio_off") : t("radio_on")}
              {r.managed && ` · ${t("radio_managed")}`}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

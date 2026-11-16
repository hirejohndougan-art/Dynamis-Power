// Dynamis Power: tells the session helper (dynamis-agent) which window is in
// front, as GNOME does not tell other programs (decision 0024). The same
// call the KWin script makes on KDE Plasma: net.dynamis.Agent1.FocusChanged
// with the process ID, on the session bus. Nothing else is sent, and nothing
// leaves the computer. GNOME Shell 45 and later (ES modules).
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const NAME = 'net.dynamis.Agent';
const PATH = '/net/dynamis/Agent';
const IFACE = 'net.dynamis.Agent1';

export default class DynamisFocus extends Extension {
    enable() {
        this._last = -1;
        this._handler = global.display.connect('notify::focus-window', () => this._report());
        this._report();
    }

    disable() {
        if (this._handler) {
            global.display.disconnect(this._handler);
            this._handler = null;
        }
        this._last = -1;
    }

    _report() {
        const w = global.display.focus_window;
        const pid = w ? w.get_pid() : 0;
        const value = pid > 0 ? pid : 0;
        if (value === this._last)
            return;
        this._last = value;
        // The helper may not be running: no answer is fine.
        Gio.DBus.session.call(
            NAME, PATH, IFACE, 'FocusChanged',
            new GLib.Variant('(u)', [value]),
            null, Gio.DBusCallFlags.NO_AUTO_START, 1000, null, null);
    }
}

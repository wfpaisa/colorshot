// Publica en D-Bus el marco de las ventanas visibles para «Capture Window»: GNOME no deja
// que otras apps lean esa lista (org.gnome.Shell.Introspect está restringido).
// Además, mientras el selector está abierto, la barra superior pasa a estar debajo de las
// ventanas (para marcar encima de ella y que salga en la captura), las esquinas activas no
// abren la vista general y el selector puede capturar solo la zona elegida.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const IFACE = `<node>
  <interface name="com.wfelipe.ColorShot">
    <method name="Windows">
      <arg type="a(iiii)" direction="out" name="frames"/>
    </method>
    <method name="Area">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="i" direction="in" name="width"/>
      <arg type="i" direction="in" name="height"/>
      <arg type="s" direction="in" name="path"/>
    </method>
  </interface>
</node>`;

const KINDS = [
    Meta.WindowType.NORMAL,
    Meta.WindowType.DIALOG,
    Meta.WindowType.MODAL_DIALOG,
    Meta.WindowType.UTILITY,
];

// El selector es una ventana «colorshot» de tipo dock por monitor.
function isSelector(window) {
    return window.get_window_type() === Meta.WindowType.DOCK &&
        window.get_title() === 'colorshot';
}

// Proceso dueño de una conexión de D-Bus.
function callerPid(sender) {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
            new GLib.Variant('(s)', [sender]), new GLib.VariantType('(u)'),
            Gio.DBusCallFlags.NONE, -1, null, (connection, result) => {
                try {
                    resolve(connection.call_finish(result).deepUnpack()[0]);
                } catch (e) {
                    reject(e);
                }
            });
    });
}

function screenshotArea(x, y, width, height, stream) {
    return new Promise((resolve, reject) => {
        new Shell.Screenshot().screenshot_area(x, y, width, height, stream,
            (shooter, result) => {
                try {
                    resolve(shooter.screenshot_area_finish(result));
                } catch (e) {
                    reject(e);
                }
            });
    });
}

export default class ColorShot extends Extension {
    enable() {
        this._object = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._object.export(Gio.DBus.session, '/com/wfelipe/ColorShot');
        this._name = Gio.bus_own_name(Gio.BusType.SESSION, 'com.wfelipe.ColorShot',
            Gio.BusNameOwnerFlags.NONE, null, null, null);

        this._selectors = new Set();
        this._reactive = null;
        global.display.connectObject('window-created', (_display, window) => {
            if (!isSelector(window))
                return;
            this._selectors.add(window);
            window.connectObject('unmanaged', () => {
                this._selectors.delete(window);
                this._update();
            }, this);
            this._update();
        }, this);
    }

    _update() {
        const selecting = this._selectors.size > 0;
        if (selecting === (this._reactive !== null))
            return;
        const layout = Main.layoutManager;
        const panel = layout.panelBox;
        if (selecting) {
            // gnome-shell pinta la barra sobre todas las ventanas: se oculta y una copia queda
            // con el fondo, bajo las ventanas. Un actor no reactivo no se elige al
            // apuntar, así que el puntero llega al selector.
            this._reactive = [];
            const walk = actor => {
                if (actor.reactive) {
                    this._reactive.push(actor);
                    actor.reactive = false;
                }
                actor.get_children().forEach(walk);
            };
            walk(panel);
            this._opacity = panel.opacity;
            panel.opacity = 0;
            // Dentro del grupo del fondo: Mutter reordena window_group y deja encima de las
            // ventanas cualquier otro actor.
            this._clone = new Clutter.Clone({source: panel, x: panel.x, y: panel.y});
            layout._backgroundGroup.add_child(this._clone);
            layout.hotCorners.forEach(corner => corner?.setBarrierSize(0));
        } else {
            this._clone?.destroy();
            this._clone = null;
            panel.opacity = this._opacity;
            this._reactive.forEach(actor => {
                try {
                    actor.reactive = true;
                } catch {
                    // Destruido mientras tanto.
                }
            });
            this._reactive = null;
            layout._updateHotCorners();
        }
    }

    disable() {
        global.display.disconnectObject(this);
        this._selectors.forEach(window => window.disconnectObject(this));
        this._selectors.clear();
        this._update();
        this._object?.unexport();
        this._object = null;
        if (this._name)
            Gio.bus_unown_name(this._name);
        this._name = 0;
    }

    // Guarda en `path` (que no debe existir) un PNG de la zona, en coordenadas del escenario.
    // El portal codifica el escritorio entero; esto solo la zona. Como lo restringido de
    // org.gnome.Shell.Screenshot, solo atiende al proceso con el selector abierto.
    async AreaAsync([x, y, width, height, path], invocation) {
        try {
            const pid = await callerPid(invocation.get_sender());
            if (![...this._selectors].some(window => window.get_pid() === pid))
                throw new Error('Only the open colorshot selector can capture');
            const stream = Gio.File.new_for_path(path)
                .create(Gio.FileCreateFlags.PRIVATE, null);
            try {
                await screenshotArea(x, y, width, height, stream);
            } finally {
                stream.close(null);
            }
            invocation.return_value(null);
        } catch (e) {
            invocation.return_error_literal(Gio.DBusError, Gio.DBusError.FAILED, e.message);
        }
    }

    // Marcos en coordenadas del escenario (lógicas), de la ventana de arriba a la de abajo.
    Windows() {
        const workspace = global.workspace_manager.get_active_workspace();
        return global.get_window_actors()
            .map(actor => actor.meta_window)
            .filter(w => w && KINDS.includes(w.get_window_type()) && !w.minimized &&
                w.showing_on_its_workspace() &&
                (w.is_on_all_workspaces() || w.get_workspace() === workspace))
            .reverse()
            .map(w => {
                const r = w.get_frame_rect();
                return [r.x, r.y, r.width, r.height];
            });
    }
}

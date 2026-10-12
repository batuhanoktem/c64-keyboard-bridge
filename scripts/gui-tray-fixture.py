#!/usr/bin/env python3
"""StatusNotifierWatcher fixture for a harness-owned D-Bus session only."""
import json
from pathlib import Path
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

WATCHER = "org.kde.StatusNotifierWatcher"
DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName(WATCHER, bus)
report = Path(sys.argv[1])
items = []


class Watcher(dbus.service.Object):
    @dbus.service.method(WATCHER, in_signature='s', out_signature='')
    def RegisterStatusNotifierItem(self, service):
        items.append(str(service))
        report.write_text(json.dumps({"items_registered": len(items)}))
        self.StatusNotifierItemRegistered(service)

    @dbus.service.method(WATCHER, in_signature='s', out_signature='')
    def RegisterStatusNotifierHost(self, service):
        self.StatusNotifierHostRegistered()

    @dbus.service.method('org.freedesktop.DBus.Properties', in_signature='ss', out_signature='v')
    def Get(self, interface, property_name):
        return self.GetAll(interface)[property_name]

    @dbus.service.method('org.freedesktop.DBus.Properties', in_signature='s', out_signature='a{sv}')
    def GetAll(self, interface):
        if interface != WATCHER:
            return {}
        return {"IsStatusNotifierHostRegistered": dbus.Boolean(True),
                "RegisteredStatusNotifierItems": dbus.Array(items, signature='s'),
                "ProtocolVersion": dbus.Int32(0)}

    @dbus.service.signal(WATCHER, signature='s')
    def StatusNotifierItemRegistered(self, service):
        pass

    @dbus.service.signal(WATCHER, signature='')
    def StatusNotifierHostRegistered(self):
        pass


watcher = Watcher(bus, '/StatusNotifierWatcher')
report.write_text(json.dumps({"ready": True, "items_registered": 0}))
GLib.MainLoop().run()

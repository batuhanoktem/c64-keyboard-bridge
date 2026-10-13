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
CONTROL = "com.flameliongames.c64keyboardbridge.SmokeFixture1"
ITEM = "org.kde.StatusNotifierItem"
MENU = "com.canonical.dbusmenu"
APP_ID = "com.flameliongames.c64keyboardbridge"
DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName(WATCHER, bus)
report = Path(sys.argv[1])
items = []
evidence = {"ready": True, "items_registered": 0, "menu_commands": []}


def save_report():
    temporary = report.with_suffix('.tmp')
    temporary.write_text(json.dumps(evidence))
    temporary.replace(report)


def find_item_menu():
    """Only inspect fixture registrations with the bridge's exact SNI identity."""
    for registration in reversed(items):
        service, separator, suffix = registration.partition('/')
        if not service.startswith('org.kde.StatusNotifierItem-'):
            continue
        path = '/' + suffix if separator else '/StatusNotifierItem'
        item = bus.get_object(service, path, introspect=False)
        properties = dbus.Interface(item, 'org.freedesktop.DBus.Properties')
        values = properties.GetAll(ITEM, timeout=2)
        if str(values.get('Id', '')) != APP_ID:
            continue
        menu_path = str(values.get('Menu', ''))
        if not menu_path.startswith('/'):
            raise dbus.exceptions.DBusException('Bridge fixture tray has no menu')
        return dbus.Interface(bus.get_object(service, menu_path, introspect=False), MENU)
    raise dbus.exceptions.DBusException('No bridge fixture tray item is registered')


def menu_entries(layout):
    identifier, properties, children = layout
    yield int(identifier), properties
    for child in children:
        yield from menu_entries(child)


class Watcher(dbus.service.Object):
    @dbus.service.method(WATCHER, in_signature='s', out_signature='', sender_keyword='sender')
    def RegisterStatusNotifierItem(self, service, sender=None):
        registration = str(service)
        if registration.startswith('/'):
            registration = str(sender) + registration
        if registration not in items:
            items.append(registration)
        evidence['items_registered'] = len(items)
        save_report()
        self.StatusNotifierItemRegistered(registration)

    @dbus.service.method(CONTROL, in_signature='', out_signature='s')
    def ExerciseHiddenMenu(self):
        """The harness invokes this only after the GUI confirms it is hidden.

        Resolve IDs from the real exported dbusmenu on each click. Do not use
        synthetic GUI actions, open a user menu, or post native input.
        """
        if evidence['menu_commands']:
            raise dbus.exceptions.DBusException('Fixture menu commands were already exercised')
        menu = find_item_menu()
        for label in ('Stop bridge', 'Enable input'):
            _, layout = menu.GetLayout(dbus.Int32(0), dbus.Int32(-1),
                                      dbus.Array(['label', 'enabled', 'visible'], signature='s'), timeout=2)
            matches = [identifier for identifier, properties in menu_entries(layout)
                       if str(properties.get('label', '')) == label
                       and bool(properties.get('enabled', True))
                       and bool(properties.get('visible', True))]
            if len(matches) != 1:
                raise dbus.exceptions.DBusException('Expected bridge fixture menu command is absent')
            menu.Event(dbus.Int32(matches[0]), 'clicked', dbus.String('', variant_level=1),
                       dbus.UInt32(0), timeout=2)
            evidence['menu_commands'].append(label)
            save_report()
        return json.dumps({"menu_commands": evidence['menu_commands'],
                           "item_id": APP_ID, "actual_dbusmenu": True})

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
save_report()
GLib.MainLoop().run()

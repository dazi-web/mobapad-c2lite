#!/usr/bin/env python3
"""GATT-Tabelle des Pro Controller 2 für die Jieli-Bibliothek (Format wie gatt_inc_generator: Größe, Flags, Handle,
UUID, Wert) erzeugen: sdk .../modules/c2lite/c2_pro2_profil.h.

Aufbau und Handles: github.com/ndeadly/switch2_controller_research, bluetooth_interface.md („GATT Attributes“, Pro
Controller mit aktualisierter Firmware, Kopfhörer-Attribute 0x002c..0x0032, GAP danach ab 0x0033). Die Konsole nutzt
die Handles direkt, darum prüft das Skript jede Nummer gegen die Doku.
"""
import sys
import uuid as uuidmod

LESEN, OHNE_ANTWORT, SCHREIBEN, MELDEN = 0x02, 0x04, 0x08, 0x10
DYNAMISCH, UUID128 = 0x100, 0x200


def u128(text):
    return uuidmod.UUID(text).bytes[::-1]  # Leitungsreihenfolge (little endian)


def u16(wert):
    return wert.to_bytes(2, "little")


eintraege, handle, namen = [], 0, {}


def attribut(flags, typ, wert=b"", name=None, soll=None):
    global handle
    handle += 1
    if soll is not None and handle != soll:
        sys.exit(f"Handle {name}: {handle:#06x} statt {soll:#06x}")
    if name:
        namen[name] = handle
    daten = u16(handle) + typ + wert
    eintraege.append((handle, name or "", u16(len(daten) + 4) + u16(flags) + daten))
    return handle


def dienst(uid, soll):
    attribut(LESEN, u16(0x2800), u128(uid) if isinstance(uid, str) else u16(uid), soll=soll)


def merkmal(uid, eigenschaften, name, soll_wert, ccc=None, beschreiber=None):
    """Deklaration, Wert, optional CCC (0x2902) und ein herstellereigener Beschreiber (128 Bit)"""
    gross = isinstance(uid, str)
    typ = u128(uid) if gross else u16(uid)
    attribut(LESEN, u16(0x2803), bytes([eigenschaften]) + u16(soll_wert) + typ)
    attribut(eigenschaften | DYNAMISCH | (UUID128 if gross else 0), typ, name=name, soll=soll_wert)
    if ccc:
        attribut(LESEN | SCHREIBEN | DYNAMISCH, u16(0x2902), b"\x00\x00", name=name + "_CCC", soll=soll_wert + 1)
    if beschreiber:
        attribut(LESEN | SCHREIBEN | DYNAMISCH | UUID128, u128(beschreiber), name=name + "_BESCHR",
                 soll=soll_wert + 2)


RATE = "679d5510-5a24-4dee-9557-95df80486ecb"  # „Set Report Rate?“
UNBEK = "b746df8c-f358-495b-9cd2-e3bbeda4f979"

dienst("00c5af5d-1964-4e30-8f51-1956f96bd280", 0x0001)
merkmal("00c5af5d-1964-4e30-8f51-1956f96bd281", LESEN, "INFO_0003", 0x0003)
merkmal("00c5af5d-1964-4e30-8f51-1956f96bd282", SCHREIBEN, "INFO_0005", 0x0005)
merkmal("00c5af5d-1964-4e30-8f51-1956f96bd283", LESEN, "INFO_0007", 0x0007)
dienst("ab7de9be-89fe-49ad-828f-118f09df7fd0", 0x0008)
merkmal("ab7de9be-89fe-49ad-828f-118f09df7fd2", LESEN | MELDEN, "BERICHT_05", 0x000a, ccc=1, beschreiber=RATE)
merkmal("7492866c-ec3e-4619-8258-32755ffcc0f8", LESEN | MELDEN, "BERICHT_09", 0x000e, ccc=1, beschreiber=RATE)
merkmal("cc483f51-9258-427d-a939-630c31f72b05", OHNE_ANTWORT, "VIBRATION", 0x0012)
merkmal("649d4ac9-8eb7-4e6c-af44-1ea54fe5f005", OHNE_ANTWORT, "BEFEHL", 0x0014)
merkmal("3dacbc7e-6955-40b5-8eaf-6f9809e8b379", OHNE_ANTWORT, "VIB_BEFEHL", 0x0016)
merkmal("4147423d-fdae-4df7-a4f7-d23e5df59f8d", OHNE_ANTWORT, "GROSSBEFEHL", 0x0018)
merkmal("c765a961-d9d8-4d36-a20a-5315b111836a", MELDEN, "ANTWORT_1", 0x001a, ccc=1, beschreiber=UNBEK)
merkmal("506d9f7d-4278-4e95-a549-326ba77657e0", MELDEN, "ANTWORT_2", 0x001e, ccc=1, beschreiber=UNBEK)
merkmal("d3bd69d2-841c-4241-ab15-f86f406d2a80", MELDEN, "MELDUNG_22", 0x0022, ccc=1, beschreiber=UNBEK)
merkmal("ab7de9be-89fe-49ad-828f-118f09df7fde", LESEN | MELDEN, "MELDUNG_26", 0x0026, ccc=1, beschreiber=RATE)
merkmal("ab7de9be-89fe-49ad-828f-118f09df7fdf", OHNE_ANTWORT, "AUSGABE_2A", 0x002a)
merkmal("cc483f51-9258-427d-a939-630c31f72b06", OHNE_ANTWORT, "KOPFHOERER_AUS", 0x002c)
merkmal("7492866c-ec3e-4619-8258-32755ffcc0f9", LESEN | MELDEN, "KOPFHOERER_EIN", 0x002e, ccc=1, beschreiber=RATE)
merkmal("3dacbc7e-6955-40b5-8eaf-6f9809e8b380", OHNE_ANTWORT, "AUSGABE_32", 0x0032)
dienst(0x1800, 0x0033)
merkmal(0x2a00, LESEN, "GERAETENAME", 0x0035)
merkmal(0x2a01, LESEN, "ERSCHEINUNG", 0x0037)
dienst(0x1801, 0x0038)

zeilen = [
    "/* Erzeugt von werkzeuge/pro2_profil.py, nicht von Hand ändern. GATT-Tabelle des Pro Controller 2 (Handles wie",
    " * ndeadly/switch2_controller_research, bluetooth_interface.md; Format der Jieli-Bibliothek). */",
    "#ifndef C2_PRO2_PROFIL_H",
    "#define C2_PRO2_PROFIL_H",
    "",
    "static const u8 pro2_profil[] = {",
]
for h, name, b in eintraege:
    zeilen.append(f"    /* {h:#06x} {name} */ " + ", ".join(f"0x{x:02x}" for x in b) + ",")
zeilen += ["    0x00, 0x00,", "};", ""]
zeilen += [f"#define PRO2_H_{n} 0x{h:04x}" for n, h in namen.items()]
zeilen += ["", "#endif", ""]
ziel = sys.argv[1] if len(sys.argv) > 1 else "c2_pro2_profil.h"
open(ziel, "w").write("\n".join(zeilen))
print(f"{ziel}: {handle} Attribute, {sum(len(b) for _, _, b in eintraege) + 2} Byte")

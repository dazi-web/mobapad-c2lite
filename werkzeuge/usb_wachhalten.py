#!/usr/bin/env python3
"""Hält den USB-Verkehr zu einer C2 Lite mit Werks-Firmware wach, damit Home + Y + X den ROM-Download-Modus auslöst.

Die Werks-Firmware sendet im Xbox-Modus nur bei Änderungen. Ohne USB-Interrupts setzt ein Wächter „USB aktiv“
nach etwa 1,1 s zurück, und die Kombination (≈ 2 s halten) greift nie. Dieses Skript schickt alle 250 ms einen
kurzen Rumble-Impuls über evdev (Treiber xpad). Jeder Impuls ist ein USB-Bericht an den Controller.
Siehe docs/WERKS-DOWNLOAD.md.

Aufruf:  python3 werkzeuge/usb_wachhalten.py [sekunden] [/dev/input/eventN]
         Ohne Gerät wird das Eingabegerät von USB 2345:e06e gesucht. Zugriff auf /dev/input/eventN hat der
         angemeldete Benutzer bei Gamepads normalerweise schon (uaccess), sonst root.
Ende:    nach der Zeit oder sobald der Controller verschwindet (er ist dann im Download-Modus, USB 4c4a:2342).
"""
import fcntl
import glob
import os
import struct
import sys
import time

EV_FF, FF_RUMBLE = 0x15, 0x50
EVIOCSFF = 0x40304580  # _IOW('E', 0x80, struct ff_effect); sizeof(struct ff_effect) = 48 auf 64-Bit-Linux
STAERKE = 0x9000       # spürbar, damit man sieht, dass Verkehr läuft
TAKT = 0.25            # Sekunden zwischen zwei Impulsen (deutlich unter 1,1 s)


def finden():
    """Eingabegerät, dessen USB-Elterngerät 2345:e06e ist (C2 Lite im PC-Modus)."""
    for ev in sorted(glob.glob("/sys/class/input/event*")):
        pfad = os.path.realpath(os.path.join(ev, "device"))
        while pfad != "/":
            vid, pid = os.path.join(pfad, "idVendor"), os.path.join(pfad, "idProduct")
            if os.path.exists(vid):
                if open(vid).read().strip() == "2345" and open(pid).read().strip() == "e06e":
                    return "/dev/input/" + os.path.basename(ev)
                break
            pfad = os.path.dirname(pfad)
    return None


def main():
    dauer = float(sys.argv[1]) if len(sys.argv) > 1 else 60.0
    dev = sys.argv[2] if len(sys.argv) > 2 else finden()
    if not dev:
        sys.exit("Kein Controller 2345:e06e gefunden (Kabel gesteckt, Schieber auf PC?)")
    fd = os.open(dev, os.O_RDWR)
    # struct ff_effect: type, id (-1 = neu), direction, trigger{button, interval}, replay{length, delay}, 2 Byte Füllung,
    # union (32 Byte): ff_rumble_effect{strong_magnitude, weak_magnitude}
    eff = bytearray(struct.pack("<HhHHHHH2x", FF_RUMBLE, -1, 0, 0, 0, 50, 0) + struct.pack("<HH", STAERKE, STAERKE) + bytes(28))
    fcntl.ioctl(fd, EVIOCSFF, eff, True)
    eid = struct.unpack_from("<h", eff, 2)[0]
    print(f"{dev}: Impulse alle {TAKT * 1000:.0f} ms für {dauer:.0f} s. Jetzt Home + Y + X etwa 2 s halten.", flush=True)

    def spiel(wert):
        os.write(fd, struct.pack("<qqHHi", 0, 0, EV_FF, eid, wert))  # struct input_event (64 Bit)

    ende, n = time.time() + dauer, 0
    try:
        while time.time() < ende:
            spiel(1)
            n += 1
            time.sleep(TAKT)
    except OSError:
        print(f"Controller nach {n} Impulsen verschwunden: vermutlich im Download-Modus (lsusb: 4c4a:2342).", flush=True)
        return
    spiel(0)
    print(f"Zeit abgelaufen ({n} Impulse), kein Wechsel.", flush=True)


if __name__ == "__main__":
    main()

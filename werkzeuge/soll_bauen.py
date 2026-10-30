#!/usr/bin/env python3
"""Baut aus dem aktuellen Flash-Stand (1 MiB) und einem jl_isd.bin das Soll-Abbild (nur Dateien, keine Hardware):
  0 .. len(jl_isd.bin)        = jl_isd.bin (Kopf, uboot.boot, isd_config, App-Bereich)
  danach .. 0x3F000           = 0xFF (VM 0x2A000.., BTIF 0x3E000.. laut Verzeichnis des neuen Abbilds)
  0x3F000 .. 0x40000          = Kopie von 0xFF000 .. 0x100000 (key_mac: BT-MAC bleibt erhalten)
  0x40000 .. 0x100000         = unverändert
Geschrieben wird nur 0 .. 0x40000 (Datei <ausgabe>.region).

Mit --vm-behalten (Update der eigenen Firmware auf eine neuere): Liegen VM und BTIF im Stand und im neuen Abbild an derselben Stelle,
endet die Region vor dem VM. Kopplungen und Einstellungen im Flash bleiben dann erhalten. Sonst Abbruch.
Mit --dualbank (Abbild mit CONFIG_DOUBLE_BANK_ENABLE, VM fest bei 0xFA000, BTIF bei 0xFC000, key_mac 0xFF000):
  0 .. len(jl_isd.bin)        = jl_isd.bin (Bank A)
  danach .. 0xFA000           = 0xFF (Bank B leer, Reste der Werks-Firmware weg)
  0xFA000 .. 0xFD000          = 0xFF (VM + BTIF neu), mit --vm-behalten unverändert
  0xFD000 .. 0x100000         = unverändert (key_mac bei 0xFF000 = Bluetooth-Adresse, liegt schon an der richtigen Stelle)
Aufruf: soll_bauen.py [--vm-behalten] [--dualbank] <stand.bin> <jl_isd.bin> <ausgabe.bin>"""
import hashlib, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from jlcommon import crypt, enc_block, entry

KEY = 0xA80F


def bereiche(d):
    """VM- und BTIF-Eintrag (off, size) aus dem Verzeichnis des App-Bereichs, None wenn nicht lesbar"""
    for o in range(0x20, 0x200, 0x20):
        h = enc_block(d[o:o + 32], 0xFFFF)
        e = entry(h)
        if not e["hok"]:
            return None
        if e["name"] != "app_dir_head":
            continue
        plain = crypt(d[e["off"]:e["off"] + 0x400], 0, KEY)
        if not entry(plain[0:32])["hok"]:
            return None
        gefunden = {}
        for o2 in range(0x20, 0x200, 0x20):
            h2 = entry(plain[o2:o2 + 32])
            if not h2["hok"]:
                break
            if h2["name"] in ("VM", "BTIF"):
                gefunden[h2["name"]] = (h2["off"], h2["size"])
        return gefunden if len(gefunden) == 2 else None
    return None


args = sys.argv[1:]
vm_behalten = "--vm-behalten" in args
dualbank = "--dualbank" in args
args = [a for a in args if a not in ("--vm-behalten", "--dualbank")]
stand = bytearray(open(args[0], "rb").read())
jl = open(args[1], "rb").read()
assert len(stand) == 0x100000, "Stand muss 1 MiB sein"
# Einzel-Bank: App bis vor das VM bei 0x3E000; Dual-Bank: Bank A bis vor Bank B bei 0x7E000
assert len(jl) % 0x1000 == 0 and len(jl) <= (0x7E000 if dualbank else 0x3E000), hex(len(jl))
soll = bytearray(stand)
soll[0:len(jl)] = jl
if dualbank:
    neu = bereiche(jl)
    if neu != {"VM": (0xFA000, 0x2000), "BTIF": (0xFC000, 0x1000)}:
        sys.exit(f"--dualbank: unerwartetes Layout {neu} (erwartet VM 0xFA000/8K, BTIF 0xFC000/4K)")
    soll[len(jl):0xFA000] = b"\xff" * (0xFA000 - len(jl))
    if not vm_behalten:
        soll[0xFA000:0xFD000] = b"\xff" * 0x3000
    ende = 0xFA000 if vm_behalten else 0xFD000  # VM und BTIF nicht anfassen
elif vm_behalten:
    neu, alt = bereiche(jl), bereiche(bytes(stand))
    if not neu or neu != alt or neu["VM"][0] != len(jl):
        sys.exit(f"--vm-behalten geht nicht: VM/BTIF neu {neu}, im Stand {alt}, Abbild endet bei {len(jl):#x}")
    ende = len(jl)
else:
    soll[len(jl):0x3F000] = b"\xff" * (0x3F000 - len(jl))
    soll[0x3F000:0x40000] = stand[0xFF000:0x100000]
    ende = 0x40000
open(args[2], "wb").write(soll)
open(args[2] + ".region", "wb").write(soll[:ende])
geaendert = sum(1 for a in range(0, 0x100000, 0x1000) if soll[a:a + 0x1000] != stand[a:a + 0x1000])
print(f"Soll {args[2]}: sha256 {hashlib.sha256(soll).hexdigest()[:16]}, {geaendert} Sektoren anders als der Stand, "
      f"Region 0..{ende:#x}{' (VM bleibt)' if vm_behalten else ''}{' (Dual-Bank)' if dualbank else ''}, "
      f"darüber unverändert: {all(soll[a:a+0x1000] == stand[a:a+0x1000] for a in range(ende, 0x100000, 0x1000))}")

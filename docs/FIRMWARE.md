# Eigene Firmware „C2 Lite Custom“ für den Mobapad C2 Lite

Kleines Easter Egg: Am Gerät meldet sie sich als „C2 Lite dazi edition“ (USB) bzw. „C2 Lite dazi“ (Bluetooth).

Freie Ersatz-Firmware für den Mobapad C2 Lite (Jieli AC635N/AC695N, Familie BR23), gebaut aus dem offenen
Jieli-SDK. Stand 2026-10-31: Stufe 13b. Fertige Firmware: `release/`.

> **Auf eigenes Risiko.** Nur an eigener Hardware verwenden. Vor dem ersten Schreiben immer den ganzen Flash
> sichern (siehe unten). Die Werks-Firmware von Mobapad liegt nicht in diesem Repo und wird nicht verteilt.

## Was die Firmware kann

| Funktion | Stand |
|---|---|
| Alle Tasten, Steuerkreuz, ZL/ZR, beide Sticks, Rücktasten, M, Licht | ✓ |
| PC über USB | ✓ als Xbox-Controller (XInput) wie die Werks-Firmware, `2345:e06e` „dazi / C2 Lite dazi edition“; Linux `xpad`, Windows eingebauter Treiber, ohne Kernel-Quirk |
| PC über Bluetooth LE | ✓ „C2 Lite dazi“ als **Xbox Wireless Controller** (045E:02FD, wie Xbox One S), mit Vibration; Schlüssellänge 7..16 (der Werks-Fehler „Key-Size 1“ ist weg), gleichzeitig mit USB. Nach dem Update auf Stufe 13 einmal neu koppeln |
| Switch über USB | ✓ als Pro Controller (057E:2009), Spieler-LEDs und HD-Rumble der Konsole |
| Switch 2 über Bluetooth | ✓ als **Pro Controller 2** (Bluetooth LE): einmal koppeln, verbindet sich selbst wieder, geht im Standby der Konsole aus ([`PRO-CONTROLLER-2.md`](PRO-CONTROLLER-2.md)) |
| Switch 2 aufwecken | ✓ Home oder Einschalten weckt die Konsole, mit der **eigenen** Adresse (ohne Joy-Con-Kennung) |
| Belegung, Turbo, Makros | ✓ wie das Original: Rück 1/2 und M mit Taste, mehreren Tasten oder Makro (bis 80 Schritte), Turbo je Taste |
| Vibration | ✓ aus PC-Spielen über USB (XInput) und Bluetooth LE (ab Stufe 13, Linux: hid-microsoft), an der Switch (HD-Rumble) und per Befehl |
| Licht | ✓ Spieler-LEDs und RGB-Logo als Zustand, Streifen und Seiten mit Effekten |
| Akkustand und Laden | ✓ über die Zellspannung, auch per Bluetooth-Akkudienst |
| Ein/Aus | ✓ Schieber auf Aus ohne USB = aus; am Akku nach 10 min ohne Eingabe aus; Wecken per Schieber, Home oder USB |
| Ladeschlaf | ✓ am Ladegerät nach 10 min Ruhe nur noch gedimmte Ladeanzeige |
| Rückmeldungen | ✓ kurze Vibration beim Verbinden, Lauflicht beim Suchen |
| Updates ohne Download-Modus | ✓ über USB (ca. 36 s) oder Bluetooth (60–90 s), sicher dank Dual-Bank |
| Einstellungen | ✓ im Flash gespeichert, bleiben bei Updates erhalten |
| Werkzeug `c2lite-custom` | ✓ Oberfläche und Kommandozeile (Linux getestet) |

## Failsafes

1. **ROM-Download-Modus (immer da):** Der Chip hat im Masken-ROM einen USB-Lader („BR23 UBOOT1.00“, USB
   `4c4a:2342`), den keine Firmware löschen kann. Das ROM horcht nur etwa 10 ms nach jedem Neustart auf den
   Jieli-USB-Schlüssel (0x16EF). Ein ESP32 sendet ihn über D+/D− (`c6-rettung/`).
2. **Home + Minus 5 s halten (mit USB-Kabel):** Unsere Firmware springt selbst in den ROM-Download-Modus
   (`go_mask_usb_updata()`, ROM-Funktion `nvram_set_boot_state`). Dafür braucht man keinen ESP32. Nur bei
   anliegender USB-Spannung, damit der Controller nicht ohne Host im Download-Modus hängt.
3. **Watchdog:** Hängt die Firmware länger als 4 s, startet der Chip neu.
4. **Absturzbericht und Nachlass:** Nach einem Absturz stehen Register, Stack-Ausschnitt und die letzten
   Protokollzeilen des vorigen Laufs im USB-Protokoll (reservierter RAM 0x2EF80..0x2FF80).
5. **Kaltstart, wenn gar nichts mehr geht:** Akku im Controller abstecken (weißer 3-Pin-Stecker rechts neben dem
   Akku) und USB-Kabel ziehen. Ohne Akku läuft der Controller über USB, ein Kabel-Einstecken ist dann ein echter
   Neustart, den der ESP32 abfängt.

**Regel für eigene Änderungen:** Jede Version braucht einen Weg zurück, ohne Gehäuse und ohne ESP32. Konkret:
USB-Stack an lassen (der Neustart beim ESP32-Signal kommt von ihm) und die Tastenkombination nicht entfernen. Eine
Testversion ohne USB-Stack ließ sich am 29.10. nur noch über den Akku-Kaltstart retten.

## Was man braucht

- Linux-Rechner (getestet: Ubuntu 24.04), Docker (zum Packen mit Jielis Packer unter Wine)
- Für den ersten Einstieg (Controller läuft noch mit Werks-Firmware): ESP32-C6 (getestet: Waveshare
  ESP32-C6-DEV-KIT-NX), USB-C-Durchgangsplatine mit herausgeführten D+/D− (z. B. PENGLIN), drei Steckkabel
- udev-Regel für den Download-Modus, damit kein sudo nötig ist:
  ```
  sudo cp werkzeuge/99-jieli-br23.rules /etc/udev/rules.d/ && sudo udevadm control --reload
  ```

Verkabelung ESP32 → Durchgangsplatine: IO6 → D+ (Takt), IO5 → D− (Daten), GND → GND. **VBUS nie an den ESP32.**

**Einfacher:** Ohne ESP32 installiert `c2lite-custom` (Seite „Installieren“) C2 Lite Custom auf einen Controller mit
Werks-Firmware, nur mit dem Bluetooth des Rechners und einem USB-Kabel (Linux, siehe README). Zurück zur
Werks-Firmware geht es auf derselben Seite (oder `c2lite-custom werk-zurueck`) mit der dabei angelegten Sicherung.

## Bauen

Fremdcode liegt nicht im Repo, sondern lokal unter `sdk-arbeit/`:

```
git clone https://github.com/Jieli-Tech/fw-AC63_BT_SDK sdk-arbeit/jieli-sdk/sdk
git -C sdk-arbeit/jieli-sdk/sdk checkout fdc018de
git -C sdk-arbeit/jieli-sdk/sdk apply "$PWD/sdk-arbeit/sdk-aenderungen.patch"
# Toolchain 20260730.1 von pkgman.jieliapp.com/s/linux-toolchain nach sdk-arbeit/jltc/tc/
werkzeuge/sdk-bauen.sh                 # -> sdk-arbeit/jieli-sdk/sdk/cpu/br23/tools/sdk.elf
werkzeuge/abbild_packen.sh stufe2p     # -> sdk-arbeit/paket/stufe2p-<zeit>/jl_isd.bin
```

Nach dem Wechsel zwischen Testvarianten (`EXT_CFLAGS=-D...`) vorher `apps/hid/board/br23/objs/` löschen; make
erkennt geänderte Flags nicht.

Das Packen braucht zusätzlich eine Schlüsseldatei `sdk-arbeit/schluessel/chipkey_a80f.key` (der Flash-Schlüssel
0xA80F des Controllers im Dateiformat von Jielis Packer). Sie liegt nicht im Repo. Wer nur installieren will, nimmt
die fertigen Dateien aus `release/`.

## Flash-Aufteilung (Dual-Bank)

| Bereich | Adresse | Inhalt |
|---|---|---|
| Bank A / Bank B | 0x3000.. / 0x7E000.. | Firmware; ein Update landet immer in der freien Bank |
| VM | 0xFA000, 8 KB | Kopplungen und Einstellungen, feste Lage |
| BTIF | 0xFC000, 4 KB | Bluetooth-Daten |
| key_mac | 0xFF000 (Adresse bei 0xFFF40) | Bluetooth-Adresse des Geräts, bleibt aus der Werks-Firmware |

OTA-Ablauf (`c2_ota.c`): `ota start <größe>`, Datenzeilen `ota d <hex>` (je 256 Byte, Bestätigung erst nach dem
Schreiben), `ota ende <crc16>`. Erst wenn CRC16 und Jielis Bankprüfung stimmen, wird umgeschaltet; sonst läuft die
alte Firmware weiter. Wichtig: die App braucht den Task `dw_update` in `task_info_table` (`apps/hid/app_main.c`),
sonst startet `dual_bank_passive_update_init` still neu.

## Installieren

Der normale Weg ist die Seite „Installieren“ von `c2lite-custom` (siehe README). Dieser Abschnitt beschreibt den
Weg über den ROM-Download-Modus: für die Rettung nach einem Brick (ESP32-C6 aus `c6-rettung/` sendet den
USB-Schlüssel) oder wenn C2 Lite Custom schon läuft (Home + Minus 5 s halten oder `c2lite-custom download-modus`).
Mit **Werks-Firmware** geht es auch ohne Bluetooth-Brücke und ohne ESP32: Home + Y + X bei laufendem USB-Verkehr,
siehe [`WERKS-DOWNLOAD.md`](WERKS-DOWNLOAD.md). Der Controller meldet sich dann als „BR23 UBOOT1.00“
(USB 4c4a:2342). Zugriff braucht root oder die udev-Regel `werkzeuge/99-jieli-br23.rules`.

1. **Sichern:** `c2lite-custom download lesen sicherung.bin` (ganzer Flash, 1 MB). Gut aufheben, das ist der Rückweg.
2. **Abbild bauen** (nur Dateien): `python3 werkzeuge/soll_bauen.py --dualbank sicherung.bin release/jl_isd.bin soll.bin`.
   Schreibt Bank A, leert Bank B, VM und BTIF; die Bluetooth-Adresse (0xFFF40) bleibt liegen. Mit `--vm-behalten`
   bleiben Kopplungen und Einstellungen erhalten (nur wenn schon C2 Lite Custom drauf ist).
3. **Schreiben und prüfen:** `c2lite-custom download schreiben 0 soll.bin.region`, dann
   `c2lite-custom download lesen zurueck.bin 0 <länge>` und `cmp zurueck.bin soll.bin.region`.
4. **Neu starten:** `c2lite-custom download neustart`.

**Updates danach:** mit `c2lite-custom update release/db_update_data.bin` oder der Seite „Firmware“.

**Zurück zur Werks-Firmware:** Seite „Installieren“ → „Zurück zur Werks-Firmware“ (aus der Sicherung der
Installation). Mit einer eigenen Sicherung aus Schritt 1: sie im Download-Modus zurückschreiben
(`c2lite-custom download schreiben 0 sicherung.bin`).

## Werkzeug c2lite-custom (Rust)

`c2lite-custom/`: bauen mit `cargo build --release`, Programm `target/release/c2lite-custom`. Ohne Befehl öffnet es
eine Oberfläche (Übersicht, Tastentest, Belegung & Turbo, Licht & Vibration, Firmware-Update, Diagnose, Backup).
Es findet den Controller selbst über USB (PC-Modus) oder mit `--bluetooth` per Bluetooth. Unter Linux muss der
Benutzer in der Gruppe `dialout` sein.

```
c2lite-custom                        Oberfläche
c2lite-custom test                   Live-Ansicht: Tasten, Sticks, Akku
c2lite-custom info                   Firmware, Laufzeit, Reset-Quelle, Akku
c2lite-custom licht [modus]          regenbogen | farbwechsel | atmen | statisch | reaktiv | aus
c2lite-custom licht farbe ff8800     (oder rot, blau, lila, türkis, ...); helligkeit / logo / tempo / standard
c2lite-custom belegung | turbo ...   Rücktasten, M, Makros, Turbo (c2lite-custom hilfe)
c2lite-custom wecken [einrichten|lernen|test|zuruecksetzen]   Switch-2-Weckkennung
c2lite-custom update db_update_data.bin   Firmware-Update (auch mit --bluetooth)
c2lite-custom led 1,2 | alle | aus | auto
c2lite-custom vibration 60 60 500
c2lite-custom download-modus         in den ROM-Download-Modus (zum Flashen)
```

## Bedienung am Controller

| Bedienung | Wirkung |
|---|---|
| Schieber | PC, Aus, Switch (Wechsel startet den Controller neu) |
| Licht kurz | nächster Lichtmodus |
| Licht 3 s halten | Tasten aufnehmen, Abschluss mit Rück 1, Rück 2 oder M |
| Licht + Taste | Turbo dieser Taste: beim Halten → automatisch → aus |
| Licht + Rück 1/2/M | Makro einmal ↔ endlos (liegt dort eine Taste: deren Turbo) |
| Licht + linker Stick | links/rechts Turbo-Tempo 5/8/12 Hz, hoch/runter Vibrationsstärke |
| Licht + rechter Stick | hoch/runter Helligkeit |
| Licht + Home | Switch-2-Weckkennung lernen (siehe unten) |
| Home (Switch-Modus, nicht verbunden) | Switch 2 aufwecken und verbinden |
| Home + Minus 5 s (mit USB) | ROM-Download-Modus zum Flashen |

## Licht und Anzeigen

| Zustand | Spieler-LEDs | Logo |
|---|---|---|
| Einschalten | Lauflicht 1→4 | weiß |
| Bluetooth sucht | Lauflicht 1→4 | blau blinkend |
| verbunden (USB oder Bluetooth) | Spieler 1 (Switch: Nummer von der Konsole) | weiß |
| am Ladegerät | alle blinken (voll: aus) | grün, Streifen als Ladebalken |
| Ladeschlaf | aus | schwach grün atmend (voll: aus) |
| Akku schwach | – | rot blinkend |
| Aufnahme | – | türkis blinkend |
| Weckkennung lernen | – | orange blinkend, dann 3× grün (gefunden) oder 2× rot |
| Home + Minus halten | füllen sich, dann schnelles Blinken | lila |
| Download-Modus (danach) | aus | kurz rot, dann alles dunkel, solange das Kabel steckt |

Nach 5 min ohne Eingabe wird das Licht gedimmt. Der PC gilt als verbunden, solange der USB-Rahmenzähler läuft (SOF),
nicht erst, wenn ein Programm den Controller liest. Beim Verbinden vibriert der Controller kurz (nicht bei
Vibration 0 %).

Hardware (am Gerät geprüft): Spieler-LEDs 1..4 = PC5/PC4/PC3/PC2 gegen die Sammelleitung PA7. RGB wie WS2812 per
Bit-Bang: PA7 Logo + 7 LEDs Streifen (R,G,B), PA8 2 LEDs an den Seiten (G,R,B). PA7/PA8 sind zugleich LED- bzw.
Tastenleitung, daher läuft alles im Wechsel in der Abfragerunde (10 ms).

## Switch 2

Ab Stufe 11 ist der C2 Lite mit Schieber auf Switch ein **Pro Controller 2** per Bluetooth LE: koppeln, spielen,
wecken ohne Joy-Con. Alles dazu, auch das Wissen für eigene Nachbauten: [`PRO-CONTROLLER-2.md`](PRO-CONTROLLER-2.md).

Der frühere Weg (Switch-1-Pro-Controller über klassisches Bluetooth) ist mit `pro2 klassisch` wählbar:

1. **Registrieren:** Schieber auf Switch, per USB-Kabel an die Switch 2 (obere Buchse, „Kabelgebundene
   Pro-Controller-Kommunikation“ an), in „Griffhaltung/Reihenfolge ändern“ bestätigen. Danach geht es kabellos.
2. **Aufwecken einrichten (einmal):** Die Switch 2 wacht nur bei Controllern auf, die sie selbst per Bluetooth LE
   gekoppelt hat (Joy-Con 2, Pro Controller 2). Der C2 Lite meldet sich als Switch-1-Pro-Controller und leiht sich
   deshalb die Kennung eines Joy-Con 2. 8BitDo und GameSir machen es offenbar genauso (ihre Einrichtung
   verlangt ebenfalls einen Joy-Con; offiziell erklärt ist es nicht). Switch in den Standby, am C2 Lite
   Licht + Home, dann einen Joy-Con 2 abnehmen und eine Taste daran drücken. Alternativ über `c2lite-custom`
   (Übersicht → „Switch 2 aufwecken“).
3. **Aufwecken:** Home am C2 Lite.

## Protokoll und Befehle (USB-seriell)

Mitlesen mit einem Terminalprogramm, z. B. `picocom /dev/ttyACM0` (Schieber auf PC). Befehle enden mit `\r`.
Nur ein Programm gleichzeitig am Port, sonst teilen sich beide die Zeilen.

| Befehl | Wirkung |
|---|---|
| `info`, `status`, `einstellungen` | Zustand (maschinenlesbar für Werkzeuge) |
| `setze <anzeige\|vibration\|totzone\|licht\|hell\|logo\|tempo\|farbe> <wert>` | Einstellung ändern und speichern |
| `led <maske\|auto>`, `motor <l%> <r%> [ms]` | Spieler-LEDs, Vibration |
| `rgb alle\|a\|b ...`, `rgb auto`, `rgb zeit`, `rgb messen` | RGB direkt (Test), Zeitmessung |
| `neustart`, `haenger`, `download-modus`, `hilfe` | Neustart, Watchdog-Test, ROM-Download-Modus |
| `belegung ...`, `turbo ...` | Rücktasten, M, Makros, Turbo |
| `switch diag`, `switch weck [setzen] [pid] [adresse]`, `switch weck lernen` | Switch-Zustand, Weckkennung |
| `ota start\|d\|ende\|abbruch` | Update in die freie Bank (benutzt `c2lite-custom update`) |
| `log [zeichen]`, `btlog [ab]`, `ruhe <s>` | Protokollende, Bluetooth-Mitschnitt, Ruhezeit vortäuschen (Test) |

Reset-Quelle (P3_RST_SRC, aus `cpu.a`/`power_hw.c`): 0x01 VDDIO POR, 0x02 VDDIO LVD, 0x04 WDT, 0x08 VCM,
0x10 PPINR, 0x20 SYS RESET, 0x40 SOFT RESET.

## Bekannte Fallstricke

- USB-seriell: Beim Öffnen/Schließen des Ports kein Endpunkt neu einrichten (Daten-Toggle), Senden bei DTR 0
  sperren (sonst blockiert die Firmware bis zum Watchdog). Siehe `cdc.c` (C2 Lite).
- Keine langen Schleifen beim Start: der Watchdog (4 s) startet sonst in einer Schleife neu (Stufe 2w).
- USB-Gamepad-Bericht nur senden, wenn der vorige abgeholt ist: `usb_g_intr_write` der Bibliothek wartet sonst aktiv
  bis zu 20 ms, solange kein Programm das Gamepad liest. Bei Stickbewegung (Senden alle 10 ms) verhungerte so der
  Watchdog, Neustart nach ~3,5 s (Stufe 13a, behoben in 13b: `c2_ep_tx_belegt` in `hid.c` prüft TXCSR vorher).

- **Systemtakt mindestens 48 MHz**, sobald USB läuft. Mit 24 MHz (wie die Werks-Firmware) startet der Chip beim
  Bluetooth-Verbinden still neu (SOFT RESET, kein Absturzbericht). Hinweis nur in der Community-Registerdoku
  (`USB_CON0.LOWP_MD`), nicht im SDK.
- Der Exception-Handler der Bibliothek gibt seine Analyse nur auf UART0 aus; unsere Firmware ersetzt
  `exception_analyze_release()` und sichert die Werte im RAM.
- Abbilder nie in Mobapads alten Rahmen umpacken (so ist der erste Versuch gebrickt: falsche Ladeadresse).

## Lizenz

Eigener Code in diesem Repo: Apache-2.0 (`LICENSE`). Jieli-SDK, Toolchain, Packer und jl-uboot-tool gehören
ihren Urhebern und sind nicht enthalten.

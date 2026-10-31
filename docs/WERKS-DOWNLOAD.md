# USB-Download-Modus der Werks-Firmware (Home + Y + X)

Die Werks-Firmware 0.04 des Mobapad C2 Lite hat einen eigenen Einstieg in den ROM-Download-Modus: **Home + Y + X
mit USB-Kabel**. Der Controller meldet sich dann als „BR23 UBOOT1.00“ (USB `4c4a:2342`, USB-Massenspeicher). Damit
lässt sich der Flash ohne Bluetooth-Brücke und ohne ESP32 lesen und schreiben. **Am Gerät bestätigt am 31.10.2026**:
Lesen, Schreiben und Prüfen des ganzen Flashs über diesen Einstieg.

English version: [`WERKS-DOWNLOAD.en.md`](WERKS-DOWNLOAD.en.md).

## Benutzen

**Im Programm:** Seite „Installieren“, Weg „USB-Kabel“, oder `c2lite-custom installieren --usb
[--probelauf]`. Das Programm setzt den nötigen USB-Quirk (siehe unten), schickt die Impulse, wartet auf den
Download-Modus, sichert, schreibt und prüft. Im Probelauf schreibt es nichts und startet danach die Werks-Firmware neu.

**Von Hand:**

1. Unter Linux den Quirk setzen, falls er fehlt (gilt bis zum Neustart):
   `echo 2345:e06e:i | sudo tee /sys/module/usbcore/parameters/quirks`. Ohne ihn fragt Linux den
   Device_Qualifier ab, daran hängt sich die Werks-Firmware auf, und dieselbe Anfrage sperrt Home + Y + X.
2. Controller mit Werks-Firmware **ohne Kabel** einschalten (Schieber auf PC, Home).
3. USB-Kabel an den Rechner stecken. Der Controller erscheint als Xbox-Controller (`2345:e06e`).
4. USB-Verkehr starten: `python3 werkzeuge/usb_wachhalten.py` (leichtes Brummen alle 250 ms).
5. **Home + Y + X etwa 2 s halten.** Das Brummen hört auf, `lsusb` zeigt `4c4a:2342 BR23UBOOT1.00`.
6. Weiter wie unter „Installieren“ in [`FIRMWARE.md`](FIRMWARE.md): `c2lite-custom download lesen sicherung.bin`
   (Rückweg aufheben), schreiben, prüfen, `c2lite-custom download neustart`.

**Ohne Schritt 4 klappt es nicht** (siehe unten). Das Hersteller-Programm der Vorlage hält den Verkehr vermutlich
selbst aufrecht. Andere Jieli-Controller nutzen dasselbe Muster, z. B. GameSir Tarantula Pro: „Logo + X + Y drücken,
bis das Gerät im Update-Programm online ist“ ([Anleitung](https://gamesir.com/de/pages/tutorial-how-to-upgrade-the-dongle-firmware-of-the-tarantula-pro)).
Daraus folgt die **Vermutung**, dass die Kombination aus Jielis Gamepad-Vorlage stammt.

## Bedingung (aus der Werks-Firmware, objdump)

Die Firmware prüft die Tastenkombinationen in der Tastenabfrage. Diese läuft alle **6 ms**: Sie wird mit 400 ms
eingetragen, stellt ihren Zeitgeber aber nach jedem Durchlauf mit `sys_timer_modify(id, 6)` um. Der Sprung in den
Download-Modus kommt, wenn bei **333 Durchläufen hintereinander (≈ 2 s)** alles zugleich gilt:

| Bedingung | Bedeutung |
|---|---|
| genau drei Tasten gedrückt, nämlich Home, Y und X | Zählung über alle Tastenbytes ohne Schieber und unbelegte Bits |
| „USB aktiv“ gesetzt | der Rechner hat in den letzten ≈ 1,1 s mit dem Controller gesprochen |
| kein Ummelden durch die Switch-Konsole | Merker, den der Switch-USB-Handshake und bestimmte Deskriptor-Anfragen setzen |
| Normalbetrieb | nicht im Lade- bzw. Aufwach-Zustand (siehe unten) |

Danach schreibt die Firmware `usb_FW_update` ins Protokoll und springt wie `go_mask_usb_updata()` aus dem Jieli-SDK
(`nvram_set_boot_state(2)`, Neustart) in den Masken-ROM-Lader.

**Warum Stillhalten nicht reicht:** Im Xbox-Modus schickt die Werks-Firmware Berichte nur, wenn sich etwas ändert.
Ohne USB-Interrupt löscht ein 100-ms-Wächter „USB aktiv“ nach etwa 1,1 s (Protokolltext `USB_Config_Flag1`). Der
Zähler kommt so nie auf 2 s. Jeder Bericht vom Rechner an den Controller, zum Beispiel ein Rumble-Befehl, löst
einen USB-Interrupt aus und hält den Merker gesetzt. Belegt durch einen usbmon-Mitschnitt (nach dem Anmelden kamen
in 35 s keine Berichte mehr) und durch den Versuch am Gerät.

**Startzustand (aus dem Code, am Gerät nicht einzeln geprüft):** Beim Start liest die Firmware Home (PB1) und die
USB-Spannung (PB5). Startet der Controller mit Kabel und ohne gedrückte Home-Taste, bleibt er im Lade-Zustand, und
die Kombination wird nicht geprüft, solange USB anliegt. Deshalb in Schritt 1 ohne Kabel einschalten.

## Wächter (Watchdog)

Die Werks-Firmware stellt den Watchdog auf 4 s (`WDT_4S`) und schaltet ihn vor dem Sprung **nicht** ab. Am Gerät
gemessen stört das nicht: 1 MB lesen in 2,7 s, zweimal hintereinander (5,5 s Dauerbetrieb), 1 MB schreiben in
8,2 s, über 3 min Leerlauf, ohne Neustart. Der Lader hält den Watchdog offenbar selbst zurück.

## Weitere Kombinationen im selben Code

Aus dem Code gelesen, am Gerät **nicht** geprüft. Zeiten bei 6 ms je Durchlauf:

| Kombination | Bedingung | Wirkung | Zeit |
|---|---|---|---|
| Home + Plus + A | Funktest freigeschaltet | Funk-Testmodus (Träger auf festen Kanälen, Wahl mit Plus + Steuerkreuz) | ≈ 2 s |
| Home | über Bluetooth verbunden | Ausschalten | ≈ 5 s |
| Home | kein USB, nicht verbunden | gespeicherte Kopplung löschen | ≈ 3 s |
| Home + eine Taste | beim Aufwachen | Verbindungsmodus wählen | kurz |

Ein dritter Weg in den Download-Modus ist ein Bluetooth-Befehl (Byte 1..4 = `53 10 01 01`, Protokolltext `ISP`,
Sprung nach 100 ms). Er ist nicht erprobt.

## Belege

Adressen sind Offsets in der App der Werks-Firmware 0.04 (Laufzeit = Offset + 0x1E000C0).

- Kombinationen: Funktion bei 0x14bb6. USB-Update-Prüfung 0x14e84..0x14ec2, Sprung 0x15248 → 0x14ac (= SDK
  `go_mask_usb_updata`).
- Takt: Eintrag `sys_timer_add(tasten, 400)` bei 0x981c, Umstellung `sys_timer_modify(id, 6)` bei 0x1a804.
- USB-Wächter: Zeitgeber-Funktion 0x1b908 (100 ms), Aktivitätsmerker am Ende der USB-Interrupt-Routine (0x6f78).
- Startzustand: App-Start bei 0x908a..0x90cc und 0x98b4..0x98ca.
- ROM-Lader: Warteschleife mit `wdt_clr` in `usb_slave_mode` (SDK `cpu/br23/tools/rom.lst`).

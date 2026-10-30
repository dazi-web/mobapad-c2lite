# C2 Lite Custom

Eigene Firmware für den Mobapad C2 Lite (Jieli BR23) und das Programm `c2lite-custom` zum Installieren und Einstellen.

English version: [`README.en.md`](README.en.md)

## Warum

Mit der Werks-Firmware lief der Controller unter Linux weder per USB (Firmware-Fehler, nur mit dem Kernel-Parameter
`usbcore.quirks=2345:e06e:i`) noch per Bluetooth (Kopplung wird abgelehnt). An der Switch 2 meldet er sich als alter
Switch-1-Controller und kann die Konsole nicht wecken.

Mit dieser Firmware:

- PC: USB als Xbox-Controller mit Vibration, Bluetooth LE als Gamepad
- Switch 2: per Bluetooth als Pro Controller 2; Home weckt die Konsole, im Standby geht der Controller nach 30 s aus
- Switch kabelgebunden als Pro Controller
- Rücktasten, Turbo, Makros, Licht, Auto-Aus wie bisher, eingestellt über `c2lite-custom` statt die App
- Updates per USB oder Bluetooth

## Installieren (Linux)

Voraussetzungen: Bluetooth am Rechner, ein USB-Datenkabel, Internet für den ersten Lauf, die Firmware-Datei
[`release/jl_isd.bin`](release/) (Prüfsummen in `release/SHA256SUMS`; selbst bauen siehe [`docs/FIRMWARE.md`](docs/FIRMWARE.md))
und `c2lite-custom` (`cargo build --release` in [`c2lite-custom/`](c2lite-custom/)).

1. `c2lite-custom` starten, Seite „Installieren“.
2. Probelauf: koppelt einmal mit dem Controller, ändert nichts.
3. Installieren: Controller ohne Kabel in den Kopplungsmodus. Das Programm überträgt über den offiziellen Update-Weg
   eine kleine Brücke (4 bis 6 Minuten).
4. Auf Aufforderung das USB-Kabel einstecken. Das Programm sichert den gesamten Speicher nach `~/c2lite-sicherung/`,
   schreibt die Firmware, liest sie zurück und startet neu.

Danach: Schieber auf PC für den Rechner, auf Switch für die Switch 2. An der Switch 2 einmal unter
„Griff/Reihenfolge ändern“ koppeln.

Unter Windows ist das Installieren nicht möglich. Der Controller sollte dort als Xbox-Controller ohne Treiber laufen,
getestet ist das nicht.

## Risiken

Beim Firmware-Tausch kann der Controller unbrauchbar werden. Das ist während der Entwicklung einmal passiert.
Vorkehrungen:

- Vor dem ersten Schreiben wird der komplette Speicher gesichert. Damit geht es zurück zur Werks-Firmware
  (Seite „Installieren“, „Zurück zur Werks-Firmware“; am Gerät getestet).
- Alles Geschriebene wird zurückgelesen und verglichen; heruntergeladene Dateien werden per Prüfsumme kontrolliert.
- Updates landen in der zweiten Speicherbank. Bricht ein Update ab, startet die alte Firmware.
- Der Chip hat einen Lademodus im ROM, den keine Firmware überschreiben kann. Mit dieser Firmware: Home und Minus 5 s
  halten, USB-Kabel gesteckt.
- Startet gar nichts mehr, hilft nur ein ESP32-C6 als Rettungssender ([`c6-rettung/`](c6-rettung/)); dafür muss der
  Controller geöffnet werden.

Beim Schreiben das Kabel nicht ziehen und den Controller nicht ausschalten. Die Sicherung aus `~/c2lite-sicherung/`
aufheben, sie passt nur zu diesem einen Controller. Die Garantie ist danach vermutlich weg. Nutzung auf eigenes Risiko.

## Entstehung

Das Projekt ist in wenigen Tagen zusammen mit Claude (Anthropic, über Claude Code) als Co-Autor entstanden. Getestet wurde an einem
Controller und einer Switch 2. Code lesen, bevor man ihm vertraut; Fehler gern melden.

## Quellcode und Lizenz

Eigener Code (Firmware-Module, SDK-Änderungen, `c2lite-custom`, Werkzeuge): Apache-2.0, siehe [`LICENSE`](LICENSE).
Die Firmware baut auf dem öffentlichen [Jieli-SDK](https://github.com/Jieli-Tech/fw-AC63_BT_SDK) auf (ebenfalls
Apache-2.0); den Bluetooth-Stack liefert Jieli nur vorkompiliert. Mobapad-Firmware ist nicht im Repo, die Werkzeuge
laden sie bei Bedarf vom Mobapad-Server. Schriften in `c2lite-custom/assets/fonts/`: Geist, SIL Open Font License.

## Dokumentation

- [`docs/FIRMWARE.md`](docs/FIRMWARE.md): Funktionen, Bedienung, Bauen, Befehle ([English](docs/FIRMWARE.en.md))
- [`docs/PRO-CONTROLLER-2.md`](docs/PRO-CONTROLLER-2.md): Switch 2 als Pro Controller 2 und was man dafür wissen muss
  ([English](docs/PRO-CONTROLLER-2.en.md))

Ordner: `c2lite-custom/` Programm, `sdk-arbeit/sdk-aenderungen.patch` Firmware (Änderungen am Jieli-SDK),
`release/` fertige Firmware, `werkzeuge/` Bauen, Packen, Download-Modus, `c6-rettung/` Rettungssender.

Danke an [kagaimiq](https://github.com/kagaimiq) (jl-uboot-tool und Jieli-Doku),
[ndeadly](https://github.com/ndeadly/switch2_controller_research) (Switch-2-Protokoll) und die in
[`docs/PRO-CONTROLLER-2.md`](docs/PRO-CONTROLLER-2.md) genannten ESP32-Projekte.

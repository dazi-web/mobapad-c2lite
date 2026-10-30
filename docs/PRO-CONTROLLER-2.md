# C2 Lite als Pro Controller 2 (Switch 2, Bluetooth LE)

Ab Stufe 11 meldet sich der C2 Lite mit dem Schieber auf **Switch** als echter **Pro Controller 2** per Bluetooth LE.
Damit funktionieren Koppeln, Spielen, Wiederverbinden und das **Wecken der Switch 2 aus dem Standby mit der
eigenen Adresse**, ohne geliehene Joy-Con-Kennung und ohne Einrichten. Am Gerät bestätigt am 30.10.2026 mit einer
Switch 2 (Firmware-Stand der Konsole vom Oktober 2026).

English version: [`PRO-CONTROLLER-2.en.md`](PRO-CONTROLLER-2.en.md). Code: `sdk-arbeit/jieli-sdk/.../modules/c2lite/c2_pro2.c`
(über `sdk-arbeit/sdk-aenderungen.patch`), GATT-Tabelle aus `werkzeuge/pro2_profil.py`.

## Benutzen

| Schritt | Was tun |
|---|---|
| Koppeln (einmal) | Schieber auf **Switch**, Switch 2 einschalten, Controller → „Griff/Reihenfolge ändern“ öffnen. Der C2 Lite wirbt als Pro Controller 2; die Switch koppelt von selbst (Vibration, Spieler-LED). |
| Spielen | Schieber auf Switch, einschalten: verbindet sich selbst wieder (verschlüsselt mit dem gespeicherten Schlüssel). |
| Switch wecken | **Home** drücken (oder den Controller einschalten): 10 s Wecksignal mit der eigenen Adresse. |
| Standby | Schläft die Switch, sucht der Controller noch 30 s und geht dann aus (am Ladekabel: Ladeschlaf). |
| Per Kabel an der Switch | bleibt der kabelgebundene (klassische) Pro Controller; Bluetooth schweigt solange. |
| Alter Weg | `c2lite-custom roh pro2 klassisch`: Switch-1-Pro-Controller über klassisches Bluetooth, Wecken mit geliehener Joy-Con-Kennung (`docs/FIRMWARE.md`). Zurück mit `pro2 standard`. |

Befehle (USB-seriell, Schieber auf PC, oder `c2lite-custom roh …`):

| Befehl | Wirkung |
|---|---|
| `pro2 status` | gewählt, gekoppelt, Konsole, Verbindung, Verschlüsselung, MTU, Zähler |
| `pro2 standard` / `pro2 klassisch` | Schieber auf Switch: Pro Controller 2 (Vorgabe) / klassischer Pro Controller |
| `pro2 vergessen` | gespeicherte Kopplung löschen (neu koppeln) |
| `pro2 spur` | die letzten 96 Ereignisse mit Zeit (siehe Fehlersuche) |
| `pro2 trennen` | Verbindung trennen (Test: die Konsole verbindet neu) |
| `pro2 an` / `pro2 einmal` / `pro2 aus` | Entwicklung: auch mit Schieber auf PC als Pro Controller 2 (USB-Protokoll bleibt zum Mitlesen); `einmal` nur für den nächsten Start |
| `pro2 debug an\|aus` | Meldungen von ATT-Server und SM der Bibliothek (nur mit `C2_BLE_DIAGNOSE`) |

Grenzen: Die zwei Unwuchtmotoren können kein HD-Rumble; aus den HD-Daten wird nur die Stärke je Seite übernommen,
die Vibrationsmuster (Verbinden, „Controller suchen“) als kräftige Stöße. Kein Bewegungssensor (Bewegungsdaten = 0),
kein NFC, kein Kopfhöreranschluss. Kabelgebunden bleibt es der Switch-1-Pro-Controller (USB-Modus des Pro Controller 2
nicht nachgebaut).

## Wissensstand (Groundtruth)

### Quellen

- Protokoll: [ndeadly/switch2_controller_research](https://github.com/ndeadly/switch2_controller_research)
  (`bluetooth_interface.md`, `commands.md`, `hid_reports.md`, `memory_layout.md`) und dessen Funk-Mitschnitte
  `captures/nrf52840/*_decrypted.pcapng`. Ohne Lizenz: übernommen sind nur Fakten (Werte, Abläufe).
- Laufende Nachbauten an echter Switch 2 (ESP32):
  [2017fighting/ESP32-BLE5-NSController-Emulator](https://github.com/2017fighting/ESP32-BLE5-NSController-Emulator)
  (MIT; Antwortdaten), [virgil724/playgate-switch2-gateway](https://github.com/virgil724/playgate-switch2-gateway)
  (A/B-Test zum 5-ms-Abstand), [espp PR #765](https://github.com/esp-cpp/espp/pull/765).
- HD-Rumble-Bitlage: SDL `src/joystick/hidapi/SDL_hidapi_switch2.c`, `EncodeHDRumble` (zlib, Valve).
- Jieli: die Bibliotheken `cpu/br23/liba/btctrler.a` und `btstack.a` des SDK sind **LLVM-Bitcode mit Debug-Infos**;
  `llvm-dis-18` macht daraus lesbaren IR-Code mit Funktions- und Variablennamen. Alles Jieli-Spezifische unten ist
  dort nachgelesen, nicht geraten.

### Ablauf einer Kopplung (echter Mitschnitt und unser Controller gleich)

```
Konsole verbindet (15 ms)          LL: LENGTH_REQ, FEATURE_REQ, PHY_REQ -> 2M
ATT MTU_REQ 512  -> MTU_RSP 512    (!) siehe Fallstricke
Read By Type …bd281 -> Handle 0x0003 = 04 00 05 00 01 01 00
Write 0x0005 = 01 00
Read By Type …bd283 -> Handle 0x0007 (8 Byte)
Dienstsuche, Abos 0x001b/0x001f/0x0023
Befehle auf 0x0016 (33 Byte Vibration vorneweg), Antworten auf 0x001e (14 Nullbytes vorneweg):
  07/01, 02/04 (Gerätespeicher 0x13000), 10/01, 16/01,
  15/01 Adressen, 15/04 Schlüssel (B1 fest), 15/02 Bestätigung (AES), 15/03 Abschluss
LL ENC_REQ (EDIV 0, Rand 0) -> LTK = A1 ^ B1
0A/02 Vibrationsmuster 3, 09/07 Spieler-LED, 0C/02 Merkmale 0x27, Gerätespeicher …, 0C/04
Abo 0x000f -> Eingabeberichte 0x09 (63 Byte) auf 0x000e; Vibration auf 0x0012
später alle 10 s 01/0C (NFC)
```

Kopplungsrechnung (geprüft mit `werkzeuge/test_pro2_krypto.c` gegen FIPS-197 und den Mitschnitt):
`LTK = A1 ^ B1` (Leitungsreihenfolge), Antwort `B2 = AES-128(umgedrehter LTK, umgedrehtes A2)`, so gesendet.
`B1 = 5CF6EE792CDF05E1BA2B6325C41A5F10` ist bei allen Controllern gleich; es gibt keine Echtheitsprüfung.

### Werbung

`02 01 06 1B FF 53 05 01 00 03 7E 05 69 20 00 01 [F] [Konsole, 6 Byte, Leitungsreihenfolge] 0F 00×7`, Typ ADV_IND,
öffentliche Adresse. `F` = 0x00 (ohne Konsole: Kopplung; mit Konsole: Wiederverbinden), 0x81 = Wecken. Die Switch 2
wacht nur bei einer Adresse auf, die sie per BLE gekoppelt hat; nach der Kopplung als Pro Controller 2 ist das die
eigene Adresse des C2 Lite (BLE-Adresse mit dem niedrigsten Byte XOR 0x55, damit ein Laptop, der den
C2 Lite als BLE-Gamepad kennt, sich nicht angesprochen fühlt).

### Fallstricke (am Gerät gefunden)

1. **ATT-MTU 512.** Antwortet der Controller auf `MTU_REQ 512` mit weniger (wir hatten 247), schickt die Switch
   danach **gar nichts mehr**: keine Dienstsuche, kein Befehl, die Verbindung bleibt stumm bestehen. Mit 512 wie
   der echte Controller läuft die Kopplung sofort durch.
2. **DLE und 2M-PHY.** Ohne lange Datenpakete und 2M (Jieli: `CONFIG_BLE_HIGH_SPEED 0`, Pakete 27 Byte) stockt die
   Switch schon nach `LL_FEATURE_REQ`. In `board_c2lite_cfg.h` ist `CONFIG_BLE_HIGH_SPEED 1` gesetzt.
3. **5 ms Verbindungsabstand.** Die Konsole schaltet auf 4 × 1,25 ms (unter dem Spec-Minimum 7,5 ms). NimBLE lehnt das
   ab (Patch nötig, siehe playgate); Jielis `ll_slave.c` übernimmt den Abstand ungeprüft. Am Gerät verbindet der
   C2 Lite direkt mit 5 ms.
4. **Kein SMP.** Die Kopplung läuft über Befehl 0x15; der Controller darf keine Sicherheitsanfrage stellen.
5. **Berichte nicht stauen.** Sendet man alle 10 ms ohne Rücksicht auf den Sendepuffer, liegen Tastendrücke
   hunderte Millisekunden in der Schlange („laggy“). Gesendet wird nur, wenn der Puffer leer ist.
6. **Vibrationsmuster.** Bei „Controller suchen“ schickt die Konsole Muster 2 und nach ~150 ms Muster 0, alle ~0,5 s.
   Beim echten Controller ein Ton aus dem Linearaktor; Unwuchtmotoren brauchen ~100 ms zum Anlaufen, darum laufen
   Muster bei uns voll und mindestens 300 ms.
7. **Nach Standby nicht endlos werben.** Sonst sucht der Controller ewig (Lauflicht); nach 30 s aus wie das Original.

### Jieli: was man dafür braucht

- **LTK einspeisen:** `btstack.a`/`sm.c` beantwortet eine LTK-Anfrage mit EDIV = 0 und Rand = 0, zu der es keinen
  eigenen Eintrag gibt, über den schwachen Haken `u8 sm_info_null_set_ltk_reply(u16 conn_handle, u8 *ltk)`. Gibt die
  Firmware 1 zurück, dreht der Stack den Puffer um und ruft `ll_hci_long_term_key_request_reply`. Also den LTK in
  **umgekehrter** Leitungsreihenfolge eintragen. `le_device_db_add` ist bei Jieli nur ein Platzhalter (liefert -1),
  der NimBLE-Weg der ESP32-Nachbauten geht hier nicht.
- **Feste Handles:** Die GATT-Tabelle ist ein Byte-Feld im Format von Jielis `gatt_inc_generator` (Größe, Flags, Handle,
  UUID); die Handles ergeben sich aus der Reihenfolge. `werkzeuge/pro2_profil.py` erzeugt sie und prüft jede Nummer
  gegen die Doku (Pro Controller mit aktueller Firmware: Kopfhörer-Merkmale bis 0x0032, GAP ab 0x0033).
- **Eigener GATT-Steuerblock** statt BLE-HID: `ble_comm_init()` mit eigenem `gatt_ctrl_t` (MTU 512, SM ohne
  Sicherheitsanfrage), `ble_gatt_server_set_profile()`, Werbung über `adv_cfg_t` mit `USE_SET_LOCAL_ADDRESS_TAG`.
  Eingehängt in `ble_hogp.c` (`bt_ble_before_start_init`, `bt_ble_init`).
- **Benachrichtigungen** mit `ble_comm_att_send_data(..., ATT_OP_NOTIFY)`; Puffer prüfen mit
  `ble_comm_cbuffer_vaild_len()` (freier Platz).
- **Klassisches Bluetooth aus**, sonst meldet sich der Controller doppelt (`app_keyboard.c`, `bt_hid_mode`).

## Fehlersuche

- **`pro2 spur`** zeigt die letzten 96 Ereignisse aus dem RAM, unabhängig vom USB-Protokoll:
  `V` verbunden (Handle = Abstand), `A` eingehendes ATT-Paket (Handle = Opcode), `L`/`S` Lesen/Schreiben je Handle,
  `M` MTU, `U` neue Verbindungsparameter, `K` LTK geliefert, `E` Verschlüsselung (0 = Erfolg), `B` erster Bericht,
  `P` Vibrationsmuster, `R` Vibration, `W` Wecken, `T` getrennt (Grund), `Z` Ruhe nach Standby.
  Das ATT-Mitlesen hängt sich beim Verbinden vor den ATT-Server der Bibliothek (`att_global_info`, Feld 1, Zeiger auf
  den Server-Handler; aus dem Bitcode von `att_dispatch.c`).
- **`C2_BLE_DIAGNOSE`** in `board_c2lite_cfg.h`: Steuerpakete der Funkschicht (`[LL_S]Recv - LL_PHY_REQ` …) und mit
  `pro2 debug an` die Meldungen von ATT-Server und SM. Erzeugt viel Ausgabe.
- Nur **ein** Programm am USB-Port: zwei Mitleser teilen sich die Zeilen (verlorene Antworten, abgebrochene Updates).

## Offen

- Bewegungssensor fehlt der Hardware; Spiele mit Bewegungssteuerung bekommen Nullen.
- Firmware-Version gemeldet wie die MIT-Vorlage (2.1.4); ob die Konsole irgendwann ein Controller-Update anbietet,
  ist nicht getestet (Update-Befehle 0x0D werden nicht umgesetzt).
- Gefühlte Verzögerung am Switch-Schieber nicht gemessen.

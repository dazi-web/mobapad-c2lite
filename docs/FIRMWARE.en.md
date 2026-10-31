# Custom firmware "C2 Lite Custom" for the Mobapad C2 Lite

A small easter egg: on the device it shows up as "C2 Lite dazi edition" (USB) or "C2 Lite dazi" (Bluetooth).

Free replacement firmware for the Mobapad C2 Lite (Jieli AC635N/AC695N, BR23 family), built from the open Jieli
SDK. Status 2026-10-31: stage 13a. Prebuilt firmware: `release/`. German version of this document:
`FIRMWARE.md`.

> **At your own risk.** Only use on hardware you own. Always back up the whole flash before the first write (see
> below). Mobapad's factory firmware is not part of this repository and is not distributed.

Command names of the firmware and of `c2lite-custom` are German (e.g. `licht` = light, `belegung` = mapping,
`wecken` = wake). They are listed with an English explanation below.

## Features

| Feature | Status |
|---|---|
| All buttons, d-pad, ZL/ZR, both sticks, back paddles, M, light key | ✓ |
| PC over USB | ✓ as Xbox controller (XInput) like the factory firmware, `2345:e06e` "dazi / C2 Lite dazi edition"; Linux `xpad`, Windows built-in driver, no kernel quirk needed |
| PC over Bluetooth LE | ✓ "C2 Lite dazi" as an **Xbox Wireless Controller** (045E:02FD, like the Xbox One S), with rumble; key size 7..16 (the factory "key size 1" bug is gone), works alongside USB. Pair again once after updating to stage 13 |
| Switch over USB | ✓ as Pro Controller (057E:2009), player LEDs and HD rumble from the console |
| Switch 2 over Bluetooth | ✓ as a **Pro Controller 2** (Bluetooth LE): pair once, reconnects by itself, powers off when the console sleeps ([`PRO-CONTROLLER-2.en.md`](PRO-CONTROLLER-2.en.md)) |
| Wake the Switch 2 | ✓ Home or power-on wakes the console with the controller's **own** address (no Joy-Con identity) |
| Mapping, turbo, macros | ✓ like the original: back 1/2 and M send a button, several buttons or a macro (up to 80 steps); turbo per button |
| Rumble | ✓ from PC games over USB (XInput) and Bluetooth LE (from stage 13, Linux: hid-microsoft), on the Switch (HD rumble) and by command |
| Lights | ✓ player LEDs and RGB logo show the state, strip and sides with effects |
| Battery level and charging | ✓ from the cell voltage, also via the Bluetooth battery service |
| Power on/off | ✓ slider to off without USB = off; on battery off after 10 min without input; wake by slider, Home or USB |
| Charge sleep | ✓ on a charger, after 10 min idle only a dimmed charge indicator remains |
| Feedback | ✓ short rumble on connect, running light while searching |
| Updates without download mode | ✓ via USB (about 36 s) or Bluetooth (60–90 s), safe thanks to dual bank |
| Settings | ✓ stored in flash, kept across updates |
| Tool `c2lite-custom` | ✓ GUI and command line (tested on Linux) |

## Failsafes

1. **ROM download mode (always present):** the chip has a USB loader in mask ROM ("BR23 UBOOT1.00", USB
   `4c4a:2342`) that no firmware can erase. The ROM only listens for about 10 ms after each reset for the Jieli
   USB key (0x16EF). An ESP32 sends it over D+/D− (`c6-rettung/`).
2. **Hold Home + Minus for 5 s (with USB cable):** our firmware jumps into the ROM download mode itself
   (`go_mask_usb_updata()`, ROM function `nvram_set_boot_state`). No ESP32 needed. Only while USB power is
   present, so the controller never sits in download mode without a host.
3. **Watchdog:** if the firmware hangs for more than 4 s, the chip restarts.
4. **Crash report and last log:** after a crash, registers, a stack excerpt and the last log lines of the previous
   run show up in the USB log (reserved RAM 0x2EF80..0x2FF80).
5. **Cold start when nothing else works:** unplug the battery inside the controller (white 3-pin connector right
   of the battery) and the USB cable. Without battery the controller runs from USB, so plugging the cable in is a
   real reset that the ESP32 can catch.

**Rule for your own changes:** every version needs a way back without opening the case and without ESP32. In
practice: keep the USB stack enabled (the restart on the ESP32 signal comes from it) and keep the button
combination. A test build without USB stack could only be recovered on 29 Oct via the battery cold start.

## What you need

- Linux computer (tested: Ubuntu 24.04), Docker (to run Jieli's packer under Wine)
- For the first install (controller still runs the factory firmware): ESP32-C6 (tested: Waveshare
  ESP32-C6-DEV-KIT-NX), USB-C breakout board with D+/D− exposed (e.g. PENGLIN), three jumper wires
- udev rule for the download mode, so no sudo is needed:
  ```
  sudo cp werkzeuge/99-jieli-br23.rules /etc/udev/rules.d/ && sudo udevadm control --reload
  ```

Wiring ESP32 → breakout board: IO6 → D+ (clock), IO5 → D− (data), GND → GND. **Never connect VBUS to the ESP32.**

**Easier:** without ESP32, `c2lite-custom` (page "Installieren") installs C2 Lite Custom on a controller with factory
firmware using only the computer's Bluetooth and a USB cable (Linux, see README). The same page (or
`c2lite-custom werk-zurueck`) goes back to the factory firmware using the backup made during installation.

## Building

Third-party code is not in the repository but locally under `sdk-arbeit/`:

```
git clone https://github.com/Jieli-Tech/fw-AC63_BT_SDK sdk-arbeit/jieli-sdk/sdk
git -C sdk-arbeit/jieli-sdk/sdk checkout fdc018de
git -C sdk-arbeit/jieli-sdk/sdk apply "$PWD/sdk-arbeit/sdk-aenderungen.patch"
# toolchain 20260730.1 from pkgman.jieliapp.com/s/linux-toolchain into sdk-arbeit/jltc/tc/
werkzeuge/sdk-bauen.sh                 # -> sdk-arbeit/jieli-sdk/sdk/cpu/br23/tools/sdk.elf
werkzeuge/abbild_packen.sh stufe2p     # -> sdk-arbeit/paket/stufe2p-<time>/jl_isd.bin and db_update_data.bin
```

After switching between test variants (`EXT_CFLAGS=-D...`) delete `apps/hid/board/br23/objs/` first; make does
not notice changed flags.

Packing also needs a key file `sdk-arbeit/schluessel/chipkey_a80f.key` (the controller's flash key 0xA80F in the file
format of Jieli's packer). It is not in the repository. If you only want to install, use the prebuilt files in
`release/`.

## Flash layout (dual bank)

| Area | Address | Content |
|---|---|---|
| Bank A / bank B | 0x3000.. / 0x7E000.. | firmware; an update always goes into the free bank |
| VM | 0xFA000, 8 KB | pairings and settings, fixed location |
| BTIF | 0xFC000, 4 KB | Bluetooth data |
| key_mac | 0xFF000 (address at 0xFFF40) | the device's Bluetooth address, kept from the factory firmware |

Update flow (`c2_ota.c`): `ota start <size>`, data lines `ota d <hex>` (256 bytes each, acknowledged only after
they are written), `ota ende <crc16>`. The bank is switched only if CRC16 and Jieli's bank check pass; otherwise
the old firmware keeps running. Important: the app needs the task `dw_update` in `task_info_table`
(`apps/hid/app_main.c`), otherwise `dual_bank_passive_update_init` silently restarts.

## Installing

The normal way is the "Installieren" page of `c2lite-custom` (see README). This section describes the way via the
ROM download mode: for recovery after a brick (the ESP32-C6 from `c6-rettung/` sends the USB key) or when C2 Lite
Custom is already running (hold Home + Minus for 5 s, or `c2lite-custom download-modus`). With the **stock
firmware** it also works without the Bluetooth bridge and without an ESP32: Home + Y + X while USB traffic is
running, see [`WERKS-DOWNLOAD.en.md`](WERKS-DOWNLOAD.en.md). The controller then shows up as "BR23 UBOOT1.00"
(USB 4c4a:2342). Access needs root or the udev rule `werkzeuge/99-jieli-br23.rules`.

1. **Back up:** `c2lite-custom download lesen backup.bin` (whole flash, 1 MB). Keep it, it is your way back.
2. **Build the image** (files only): `python3 werkzeuge/soll_bauen.py --dualbank backup.bin release/jl_isd.bin target.bin`.
   Writes bank A, clears bank B, VM and BTIF; the Bluetooth address (0xFFF40) is kept. With `--vm-behalten` pairings
   and settings are kept (only if C2 Lite Custom is already installed).
3. **Write and verify:** `c2lite-custom download schreiben 0 target.bin.region`, then
   `c2lite-custom download lesen readback.bin 0 <length>` and `cmp readback.bin target.bin.region`.
4. **Restart:** `c2lite-custom download neustart`.

**Later updates:** `c2lite-custom update release/db_update_data.bin` or the "Firmware" page.

**Back to the factory firmware:** "Installieren" page → "Zurück zur Werks-Firmware" (from the backup made during
installation). With your own backup from step 1: write it back in download mode
(`c2lite-custom download schreiben 0 backup.bin`).

## Tool c2lite-custom (Rust)

`c2lite-custom/`: build with `cargo build --release`, binary `target/release/c2lite-custom`. Without arguments it
opens a GUI (overview, button test, mapping & turbo, light & rumble, firmware update, diagnostics, backup; the GUI
is in German). It finds the controller by itself via USB (PC mode) or via Bluetooth with `--bluetooth`. On Linux
the user must be in the `dialout` group.

```
c2lite-custom                        GUI
c2lite-custom test                   live view: buttons, sticks, battery
c2lite-custom info                   firmware, uptime, reset source, battery
c2lite-custom licht [mode]           light mode: regenbogen (rainbow) | farbwechsel (colour cycle) | atmen (breathe)
                                     | statisch (static) | reaktiv (reactive) | aus (off)
c2lite-custom licht farbe ff8800     colour (or rot, blau, lila, türkis, ...); helligkeit (brightness) / logo / tempo
                                     / standard (defaults)
c2lite-custom belegung | turbo ...   back paddles, M, macros, turbo (see c2lite-custom hilfe)
c2lite-custom wecken [einrichten|lernen|test|zuruecksetzen]
                                     Switch 2 wake identity: show / capture with the laptop / let the controller
                                     learn it / send wake signal now / reset
c2lite-custom update db_update_data.bin   firmware update (also with --bluetooth)
c2lite-custom led 1,2 | alle | aus | auto   player LEDs: players 1,2 / all / off / show state
c2lite-custom vibration 60 60 500    rumble left % right % duration ms
c2lite-custom download-modus         jump into the ROM download mode (for flashing)
```

## Controls on the controller

| Input | Effect |
|---|---|
| Slider | PC, off, Switch (changing it restarts the controller) |
| Light key short | next light mode |
| Hold light key 3 s | record buttons, finish with back 1, back 2 or M |
| Light + button | turbo of that button: while held → automatic → off |
| Light + back 1/2/M | macro once ↔ endless (if a button is mapped there: its turbo) |
| Light + left stick | left/right turbo speed 5/8/12 Hz, up/down rumble strength |
| Light + right stick | up/down brightness |
| Light + Home | learn the Switch 2 wake identity (see below) |
| Home (Switch mode, not connected) | wake the Switch 2 and connect |
| Home + Minus 5 s (with USB) | ROM download mode for flashing |

## Lights and indicators

| State | Player LEDs | Logo |
|---|---|---|
| Power on | running light 1→4 | white |
| Bluetooth searching | running light 1→4 | blue, blinking |
| connected (USB or Bluetooth) | player 1 (Switch: number from the console) | white |
| on a charger | all blink (full: off) | green, strip shows a charge bar |
| charge sleep | off | dim green breathing (full: off) |
| battery low | – | red, blinking |
| recording | – | turquoise, blinking |
| learning the wake identity | – | orange blinking, then 3× green (found) or 2× red |
| holding Home + Minus | fill up, then fast blinking | purple |
| download mode (afterwards) | off | briefly red, then all dark while the cable is connected |

After 5 min without input the lights are dimmed. A PC counts as connected as long as the USB frame counter runs
(SOF), not only once a program reads the controller. On connect the controller rumbles briefly (not at rumble 0 %).

Hardware (verified on the device): player LEDs 1..4 = PC5/PC4/PC3/PC2 against the common line PA7. RGB like WS2812
via bit-bang: PA7 logo + 7 strip LEDs (R,G,B), PA8 2 side LEDs (G,R,B). PA7/PA8 are also LED and button matrix
lines, so everything alternates within the 10 ms polling round.

## Switch 2

From stage 11 on, with the slider on Switch the C2 Lite is a **Pro Controller 2** over Bluetooth LE: pair, play, wake
without a Joy-Con. Details and the knowledge for your own implementations: [`PRO-CONTROLLER-2.en.md`](PRO-CONTROLLER-2.en.md).

The previous way (Switch 1 Pro Controller over Bluetooth Classic) can be selected with `pro2 klassisch`:

1. **Register:** slider to Switch, connect to the Switch 2 with a USB cable (top port, "Pro Controller Wired
   Communication" enabled), confirm in "Change Grip/Order". Wireless afterwards.
2. **Set up waking (once):** the Switch 2 only wakes up for controllers it has paired over Bluetooth LE itself
   (Joy-Con 2, Pro Controller 2). The C2 Lite presents itself as a Switch 1 Pro Controller and therefore borrows
   the identity of a Joy-Con 2, as 8BitDo and GameSir appear to do as well (their setup procedures involve a
   Joy-Con). Put the Switch into standby, press light + Home on the C2 Lite, then detach a Joy-Con 2 and press a
   button on it. Alternatively use `c2lite-custom` (overview → "Switch 2 aufwecken").
3. **Wake:** press Home on the C2 Lite.

## Log and commands (USB serial)

Follow the log with a terminal program, e.g. `picocom /dev/ttyACM0` (slider on PC). Commands end with
`\r`. Only one program at a time on the port, otherwise both share the lines.

| Command | Effect |
|---|---|
| `info`, `status`, `einstellungen` | state (machine readable for tools) |
| `setze <anzeige\|vibration\|totzone\|licht\|hell\|logo\|tempo\|farbe> <value>` | change and store a setting |
| `led <mask\|auto>`, `motor <l%> <r%> [ms]` | player LEDs, rumble |
| `rgb alle\|a\|b ...`, `rgb auto`, `rgb zeit`, `rgb messen` | set RGB directly (test), timing measurement |
| `neustart`, `haenger`, `download-modus`, `hilfe` | restart, watchdog test, ROM download mode, help |
| `belegung ...`, `turbo ...` | back paddles, M, macros, turbo |
| `switch diag`, `switch weck [setzen] [pid] [address]`, `switch weck lernen` | Switch state, wake identity |
| `ota start\|d\|ende\|abbruch` | update into the free bank (used by `c2lite-custom update`) |
| `log [chars]`, `btlog [from]`, `ruhe <s>` | end of log, Bluetooth capture, fake idle time (test) |

Reset source (P3_RST_SRC, from `cpu.a`/`power_hw.c`): 0x01 VDDIO POR, 0x02 VDDIO LVD, 0x04 WDT, 0x08 VCM,
0x10 PPINR, 0x20 SYS RESET, 0x40 SOFT RESET.

## Known pitfalls

- USB serial: do not re-initialise endpoints when the port is opened/closed (data toggle), block sending while
  DTR is 0 (otherwise the firmware blocks until the watchdog fires). See `cdc.c` (C2 Lite).
- No long loops at startup: otherwise the watchdog (4 s) restarts in a loop (stage 2w).
- **System clock at least 48 MHz** while USB is active. At 24 MHz (as in the factory firmware) the chip silently
  restarts when Bluetooth connects (SOFT RESET, no crash report). Only hinted at in the community register docs
  (`USB_CON0.LOWP_MD`), not in the SDK.
- The library's exception handler prints its analysis only on UART0; our firmware replaces
  `exception_analyze_release()` and keeps the values in RAM.
- Never repack images into Mobapad's old frame (that is how the first attempt bricked: wrong load address).

## License

Own code in this repository: Apache-2.0 (`LICENSE`). Jieli SDK, toolchain, packer and jl-uboot-tool belong to
their authors and are not included.

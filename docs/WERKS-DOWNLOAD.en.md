# USB download mode of the stock firmware (Home + Y + X)

The stock firmware 0.04 of the Mobapad C2 Lite has its own entry into the ROM download mode: **Home + Y + X with
the USB cable connected**. The controller then shows up as "BR23 UBOOT1.00" (USB `4c4a:2342`, USB mass storage),
so the flash can be read and written without the Bluetooth bridge and without an ESP32. **Confirmed on the device
on 2026-10-31**: reading, writing and verifying the whole flash through this entry.

Deutsche Fassung: [`WERKS-DOWNLOAD.md`](WERKS-DOWNLOAD.md).

## Usage

**In the program:** page "Installieren", way "USB-Kabel", or `c2lite-custom installieren --usb
[--probelauf]`. The program sets the required USB quirk (see below), sends the pulses, waits for the download mode,
backs up, writes and verifies. In the dry run it writes nothing and restarts the stock firmware afterwards.

**By hand:**

1. On Linux, set the quirk if it is missing (valid until reboot):
   `echo 2345:e06e:i | sudo tee /sys/module/usbcore/parameters/quirks`. Without it Linux requests the
   Device_Qualifier; the stock firmware hangs on that, and the same request blocks Home + Y + X.
2. Switch on the controller with stock firmware **without the cable** (slider on PC, Home).
3. Connect the USB cable to the computer. The controller appears as an Xbox controller (`2345:e06e`).
4. Start USB traffic: `python3 werkzeuge/usb_wachhalten.py` (light buzz every 250 ms).
5. **Hold Home + Y + X for about 2 s.** The buzzing stops, and `lsusb` shows `4c4a:2342 BR23UBOOT1.00`.
6. Continue as described under "Installing" in [`FIRMWARE.en.md`](FIRMWARE.en.md):
   `c2lite-custom download lesen backup.bin` (keep it, it is your way back), write, verify,
   `c2lite-custom download neustart`.

**Without step 4 it does not work** (see below). The vendor tool of the reference design presumably keeps the
traffic going by itself. Other Jieli controllers use the same pattern, e.g. GameSir Tarantula Pro: "press Logo + X
+ Y until the device shows online in the upgrade software"
([guide](https://gamesir.com/de/pages/tutorial-how-to-upgrade-the-dongle-firmware-of-the-tarantula-pro)).
This suggests that the combination comes from Jieli's gamepad reference design (assumption).

## Condition (from the stock firmware, objdump)

The firmware checks its button combinations in the button scan. The scan runs every **6 ms**: it is registered
with 400 ms but re-arms its timer with `sys_timer_modify(id, 6)` after every pass. The jump into download mode
happens when all of the following hold for **333 consecutive passes (≈ 2 s)**:

| Condition | Meaning |
|---|---|
| exactly three buttons pressed: Home, Y and X | count over all button bytes, slider and unused bits masked |
| "USB active" set | the host talked to the controller within the last ≈ 1.1 s |
| no re-enumeration by the Switch console | flag set by the Switch USB handshake and certain descriptor requests |
| normal operation | not in the charging / wake-up state (see below) |

The firmware then logs `usb_FW_update` and jumps into the mask ROM loader like `go_mask_usb_updata()` from the
Jieli SDK (`nvram_set_boot_state(2)`, reset).

**Why holding still is not enough:** in Xbox mode the stock firmware only sends reports when something changes.
Without a USB interrupt, a 100 ms watchdog clears "USB active" after about 1.1 s (log text `USB_Config_Flag1`), so
the counter never reaches 2 s. Every report from the host to the controller, e.g. a rumble command, triggers a USB
interrupt and keeps the flag set. Confirmed by a usbmon capture (no reports for 35 s after enumeration) and by the
test on the device.

**Start state (from the code, not tested separately on the device):** at start-up the firmware reads Home (PB1)
and USB voltage (PB5). If the controller starts with the cable connected and Home not pressed, it stays in the
charging state, and the combination is not checked while USB is present. That is why step 1 switches it on
without the cable.

## Watchdog

The stock firmware sets the watchdog to 4 s (`WDT_4S`) and does **not** disable it before the jump. Measured on the
device this does not matter: reading 1 MB in 2.7 s, twice in a row (5.5 s of continuous operation), writing 1 MB
in 8.2 s, more than 3 min idle, without a reset. The loader apparently keeps the watchdog at bay itself.

## Other combinations in the same code

Read from the code, **not** tested on the device. Times at 6 ms per pass:

| Combination | Condition | Effect | Time |
|---|---|---|---|
| Home + Plus + A | radio test enabled | radio test mode (carrier on fixed channels, selected with Plus + D-pad) | ≈ 2 s |
| Home | connected via Bluetooth | power off | ≈ 5 s |
| Home | no USB, not connected | delete stored pairing | ≈ 3 s |
| Home + one button | on wake-up | choose connection mode | short |

A third way into download mode is a Bluetooth command (bytes 1..4 = `53 10 01 01`, log text `ISP`, jump after
100 ms). It has not been tried.

## References

Addresses are offsets in the application of stock firmware 0.04 (runtime = offset + 0x1E000C0).

- Combinations: function at 0x14bb6. USB update check 0x14e84..0x14ec2, jump 0x15248 → 0x14ac (= SDK
  `go_mask_usb_updata`).
- Timing: `sys_timer_add(scan, 400)` at 0x981c, `sys_timer_modify(id, 6)` at 0x1a804.
- USB watchdog: timer function 0x1b908 (100 ms), activity flag at the end of the USB interrupt routine (0x6f78).
- Start state: application start at 0x908a..0x90cc and 0x98b4..0x98ca.
- ROM loader: wait loop with `wdt_clr` in `usb_slave_mode` (SDK `cpu/br23/tools/rom.lst`).

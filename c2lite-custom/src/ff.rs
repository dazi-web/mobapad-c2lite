//! Force Feedback (Rumble) über evdev: Effekt anlegen und abspielen (Linux, struct ff_effect / input_event auf 64 Bit).
//! Genutzt vom USB-Weg der Installation (werk_usb: Impulse halten „USB aktiv“) und vom Bluetooth-Test (bt_pc).

use std::io::{Error, Result};
use std::path::Path;

const EV_FF: u16 = 0x15;
const FF_RUMBLE: u16 = 0x50;
/// _IOW('E', 0x80, struct ff_effect); sizeof(struct ff_effect) = 48 auf 64-Bit-Linux
const EVIOCSFF: u64 = 0x4030_4580;

/// Offenes Eingabegerät mit einem angelegten Rumble-Effekt
pub struct Effekt {
    fd: i32,
    id: i16,
}

impl Effekt {
    /// `stark` = linker Motor, `schwach` = rechter Motor (0..=0xFFFF), `dauer_ms` je Abspielen
    pub fn neu(pfad: &Path, stark: u16, schwach: u16, dauer_ms: u16) -> Result<Effekt> {
        let c = std::ffi::CString::new(pfad.as_os_str().to_string_lossy().as_bytes()).map_err(Error::other)?;
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        // struct ff_effect: type, id (-1 = neu), direction, trigger{button, interval}, replay{length, delay}, 2 Byte
        // Füllung, union ab Byte 16 (ff_rumble_effect: strong_magnitude, weak_magnitude)
        let mut eff = [0u8; 48];
        eff[0..2].copy_from_slice(&FF_RUMBLE.to_ne_bytes());
        eff[2..4].copy_from_slice(&(-1i16).to_ne_bytes());
        eff[10..12].copy_from_slice(&dauer_ms.to_ne_bytes());
        eff[16..18].copy_from_slice(&stark.to_ne_bytes());
        eff[18..20].copy_from_slice(&schwach.to_ne_bytes());
        if unsafe { libc::ioctl(fd, EVIOCSFF as _, eff.as_mut_ptr()) } < 0 {
            let e = Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        Ok(Effekt { fd, id: i16::from_ne_bytes([eff[2], eff[3]]) })
    }

    /// Starten (1) oder stoppen (0); struct input_event auf 64-Bit-Linux: 16 Byte Zeit, type, code, value
    pub fn spielen(&self, wert: i32) -> Result<()> {
        let mut ev = [0u8; 24];
        ev[16..18].copy_from_slice(&EV_FF.to_ne_bytes());
        ev[18..20].copy_from_slice(&(self.id as u16).to_ne_bytes());
        ev[20..24].copy_from_slice(&wert.to_ne_bytes());
        let n = unsafe { libc::write(self.fd, ev.as_ptr().cast(), ev.len()) };
        if n == ev.len() as isize { Ok(()) } else { Err(Error::last_os_error()) }
    }
}

impl Drop for Effekt {
    fn drop(&mut self) {
        let _ = self.spielen(0);
        unsafe { libc::close(self.fd) };
    }
}

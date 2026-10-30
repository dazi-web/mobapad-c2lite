//! ROM-Download-Modus des Jieli BR23 (Mobapad C2 Lite): USB-Massenspeicher „BR23 UBOOT1.00“ (4c4a:2342), Befehle als
//! SCSI-CDBs über /dev/sgN (Linux SG_IO). Nachbau von jl-uboot-tool (kagaimiq, MIT, jltech/uboot.py):
//!   ROM (UBOOT1.00): Speicher schreiben FB06, anspringen FB08 -> Lader laden (br23loader.bin bei 0x12000)
//!   Lader (V2):      Flash lesen FD05, schreiben FB04, löschen 64 KB FB00 / 4 KB FB01, Puffergröße FC14,
//!                    Speicher anspringen FB08 (Neustart über ROM-chip_reset 0x1102DE)
//! Der Lader selbst ist ein Jieli-Programm und liegt nicht im Repo: Er wird aus jl-uboot-tool (fester Commit)
//! geladen und per SHA-256 geprüft.

use std::io::{Error, ErrorKind, Result};
use std::time::{Duration, Instant};

pub const LADER_URL: &str =
    "https://raw.githubusercontent.com/kagaimiq/jl-uboot-tool/adb3f18889e88ac512ce0a3c4d8cc3d3cb30696a/data/loaderblobs/usb/br23loader.bin";
pub const LADER_SHA256: &str = "7384429e33ee1f340fbffe46aa928a0099c163e1374a40f6f9f27468a6f16ddf";
const LADER_ADRESSE: u32 = 0x12000;
const LADER_ARGUMENT: u16 = 0x0001; // SPI-NOR, Standard-Takt (wie jl-uboot-tool)
const CHIP_RESET: u32 = 0x1102DE; // ROM-Funktion chip_reset (rom.lst)
pub const FLASH_GROESSE: u32 = 0x100000;

/// CRC-16/XMODEM (Jieli „jl_crc16“)
pub fn crc16(d: &[u8], start: u16) -> u16 {
    let mut c = start;
    for &b in d {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 };
        }
    }
    c
}

// ---------------------------------------------------------------- SG_IO

#[repr(C)]
struct SgIoHdr {
    interface_id: i32,
    dxfer_direction: i32,
    cmd_len: u8,
    mx_sb_len: u8,
    iovec_count: u16,
    dxfer_len: u32,
    dxferp: *mut u8,
    cmdp: *const u8,
    sbp: *mut u8,
    timeout: u32,
    flags: u32,
    pack_id: i32,
    usr_ptr: *mut libc::c_void,
    status: u8,
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8,
    host_status: u16,
    driver_status: u16,
    resid: i32,
    duration: u32,
    info: u32,
}

const SG_IO: libc::c_ulong = 0x2285;

pub struct Download {
    fd: i32,
    puffer: usize,
}

impl Drop for Download {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

/// /dev/sgN des Controllers im Download-Modus (Hersteller „BR23“), sonst None
pub fn geraet_finden() -> Option<String> {
    let mut namen: Vec<_> = std::fs::read_dir("/sys/class/scsi_generic").ok()?.flatten().map(|e| e.file_name()).collect();
    namen.sort();
    namen.into_iter().find_map(|n| {
        let n = n.to_string_lossy().to_string();
        let v = std::fs::read_to_string(format!("/sys/class/scsi_generic/{n}/device/vendor")).ok()?;
        v.contains("BR23").then(|| format!("/dev/{n}"))
    })
}

/// Wartet bis zu `dauer` auf den Download-Modus
pub fn warten(dauer: Duration) -> Option<String> {
    let bis = Instant::now() + dauer;
    loop {
        if let Some(p) = geraet_finden() {
            return Some(p);
        }
        if Instant::now() > bis {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

impl Download {
    pub fn oeffnen(pfad: &str) -> Result<Download> {
        let c = std::ffi::CString::new(pfad).unwrap();
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        Ok(Download { fd, puffer: 256 })
    }

    fn ausfuehren(&self, cdb: &[u8], raus: Option<&[u8]>, rein: Option<&mut [u8]>) -> Result<()> {
        if std::env::var_os("C2_DOWNLOAD_PROTOKOLL").is_some() && (cdb.starts_with(&[0xfb, 0x08]) || cdb.starts_with(&[0xfb, 0x06])) {
            let h: String = cdb.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("CDB {h} raus {} rein {}", raus.map_or(0, |d| d.len()), rein.as_ref().map_or(0, |d| d.len()));
        }
        let mut sense = [0u8; 32];
        let (richtung, zeiger, laenge) = match (raus, rein) {
            (Some(d), None) => (-2, d.as_ptr() as *mut u8, d.len()),
            (None, Some(d)) => (-3, d.as_mut_ptr(), d.len()),
            _ => (-1, std::ptr::null_mut(), 0),
        };
        let mut h = SgIoHdr {
            interface_id: b'S' as i32,
            dxfer_direction: richtung,
            cmd_len: cdb.len() as u8,
            mx_sb_len: sense.len() as u8,
            iovec_count: 0,
            dxfer_len: laenge as u32,
            dxferp: zeiger,
            cmdp: cdb.as_ptr(),
            sbp: sense.as_mut_ptr(),
            timeout: 5000,
            flags: 0,
            pack_id: 0,
            usr_ptr: std::ptr::null_mut(),
            status: 0,
            masked_status: 0,
            msg_status: 0,
            sb_len_wr: 0,
            host_status: 0,
            driver_status: 0,
            resid: 0,
            duration: 0,
            info: 0,
        };
        let r = unsafe { libc::ioctl(self.fd, SG_IO, &mut h as *mut SgIoHdr) };
        if r < 0 {
            return Err(Error::last_os_error());
        }
        if h.info & 1 != 0 {
            return Err(Error::other(format!(
                "Übertragung fehlgeschlagen (Status {:02x}, Host {:02x}, Treiber {:02x})",
                h.status, h.host_status, h.driver_status
            )));
        }
        Ok(())
    }

    fn cdb(befehl: u16, argumente: &[u8]) -> [u8; 16] {
        let mut c = [0xFFu8; 16];
        c[0..2].copy_from_slice(&befehl.to_be_bytes());
        c[2..2 + argumente.len()].copy_from_slice(argumente);
        c
    }

    fn befehl(&self, befehl: u16, argumente: &[u8]) -> Result<[u8; 14]> {
        let mut antwort = [0u8; 16];
        self.ausfuehren(&Self::cdb(befehl, argumente), None, Some(&mut antwort))?;
        if u16::from_be_bytes([antwort[0], antwort[1]]) != befehl {
            return Err(Error::other(format!("unerwartete Antwort auf {befehl:04x}")));
        }
        Ok(antwort[2..].try_into().unwrap())
    }

    /// Hersteller und Produkt (z. B. „BR23“, „UBOOT1.00“)
    pub fn kennung(&self) -> Result<(String, String)> {
        let mut d = [0u8; 36];
        self.ausfuehren(&[0x12, 0, 0, 0, 36, 0], None, Some(&mut d))?; // SCSI INQUIRY (6-Byte-CDB)
        let t = |a: usize, b: usize| String::from_utf8_lossy(&d[a..b]).trim().to_string();
        Ok((t(8, 16), t(16, 32)))
    }

    fn speicher_schreiben(&self, befehl: u16, adresse: u32, daten: &[u8]) -> Result<()> {
        let mut a = Vec::with_capacity(9);
        a.extend_from_slice(&adresse.to_be_bytes());
        a.extend_from_slice(&(daten.len() as u16).to_be_bytes());
        a.push(0);
        a.extend_from_slice(&crc16(daten, 0).to_le_bytes());
        self.ausfuehren(&Self::cdb(befehl, &a), Some(daten), None)
    }

    fn anspringen(&self, adresse: u32, argument: u16) -> Result<()> {
        let mut a = adresse.to_be_bytes().to_vec();
        a.extend_from_slice(&argument.to_be_bytes());
        self.befehl(0xFB08, &a).map(|_| ())
    }

    /// Lader in den RAM laden und starten (nur im Zustand UBOOT1.00); `lader` = br23loader.bin (verschleiert)
    pub fn lader_starten(&mut self, lader: &[u8]) -> Result<()> {
        let (_, produkt) = self.kennung()?;
        if produkt == "UBOOT1.00" {
            let melden = std::env::var_os("C2_DOWNLOAD_PROTOKOLL").is_some();
            /* Das BR23-ROM nimmt Speicherdaten MengLi-verschleiert an und entschleiert sie an Ort und Stelle
             * (jl-uboot-tool chips.yaml: memory-rw-mengli-crypt), der Lader geht also unverändert raus. */
            for (i, block) in lader.chunks(512).enumerate() {
                self.speicher_schreiben(0xFB06, LADER_ADRESSE + (i * 512) as u32, block)
                    .map_err(|e| Error::other(format!("Lader-Block {i}: {e}")))?;
                if melden {
                    eprintln!("Lader-Block {i} geschrieben");
                }
            }
            self.anspringen(LADER_ADRESSE, LADER_ARGUMENT).map_err(|e| Error::other(format!("Lader starten: {e}")))?;
        }
        self.puffer = self.befehl(0xFC14, &[]).map(|r| u32::from_be_bytes([r[0], r[1], r[2], r[3]]) as usize).unwrap_or(256);
        if !(256..=4096).contains(&self.puffer) {
            self.puffer = 256;
        }
        Ok(())
    }

    pub fn lesen(&self, adresse: u32, laenge: u32, fortschritt: &mut dyn FnMut(f32)) -> Result<Vec<u8>> {
        let mut alles = Vec::with_capacity(laenge as usize);
        while (alles.len() as u32) < laenge {
            let a = adresse + alles.len() as u32;
            let n = (laenge - alles.len() as u32).min(self.puffer as u32) as usize;
            let mut d = vec![0u8; n];
            let mut arg = a.to_be_bytes().to_vec();
            arg.extend_from_slice(&(n as u16).to_be_bytes());
            self.ausfuehren(&Self::cdb(0xFD05, &arg), None, Some(&mut d))?;
            alles.extend_from_slice(&d);
            fortschritt(alles.len() as f32 / laenge as f32);
        }
        Ok(alles)
    }

    /// Bereich löschen (64-KB-Blöcke, wo möglich, sonst 4 KB) und schreiben
    pub fn schreiben(&self, adresse: u32, daten: &[u8], fortschritt: &mut dyn FnMut(f32)) -> Result<()> {
        if adresse & 0xFFF != 0 {
            return Err(Error::new(ErrorKind::InvalidInput, "Adresse muss auf 4 KB ausgerichtet sein"));
        }
        let ende = (adresse + daten.len() as u32 + 0xFFF) & !0xFFF;
        let mut a = adresse;
        while a < ende {
            if a & 0xFFFF == 0 && ende - a >= 0x10000 {
                self.befehl(0xFB00, &a.to_be_bytes())?;
                a += 0x10000;
            } else {
                self.befehl(0xFB01, &a.to_be_bytes())?;
                a += 0x1000;
            }
        }
        let mut fertig = 0usize;
        for stueck in daten.chunks(self.puffer) {
            self.speicher_schreiben(0xFB04, adresse + fertig as u32, stueck)?;
            fertig += stueck.len();
            fortschritt(fertig as f32 / daten.len() as f32);
        }
        Ok(())
    }

    /// Neustart aus dem Download-Modus (ROM chip_reset über den Lader); die Verbindung reißt dabei ab
    pub fn neustart(&self) {
        let _ = self.anspringen(CHIP_RESET, 0);
    }
}

/// Lader aus dem Zwischenspeicher oder per curl aus jl-uboot-tool holen und per SHA-256 prüfen
pub fn lader_holen() -> Result<Vec<u8>> {
    crate::abbild::holen(LADER_URL, LADER_SHA256, "br23loader.bin")
}

/// SHA-256 (klein, ohne Abhängigkeit; nur für die Prüfung heruntergeladener Dateien)
pub fn sha256_hex(daten: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut m = daten.to_vec();
    let bits = (daten.len() as u64) * 8;
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&bits.to_be_bytes());
    for block in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[4 * i..4 * i + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_bekannt() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}

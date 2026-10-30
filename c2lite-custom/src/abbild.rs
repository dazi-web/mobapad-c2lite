//! Abbilder für die Erstinstallation (reine Dateiarbeit, kein Gerätezugriff):
//! - Brücke: offizielle Werks-Firmware 0.04 (zur Laufzeit vom Mobapad-Server, SHA-256 geprüft, nicht im Repo), deren
//!   App-Anfang durch bruecke/warte_auf_usb.bin ersetzt ist (wartet auf USB-Spannung, dann ROM-Download-Modus).
//!   Jieli-Verschlüsselung „ENC“ (Schlüssel 0xA80F ^ (Offset >> 2) je 32-Byte-Block) und JLFS-Prüfsummenkette neu.
//! - Abbild der eigenen Firmware (C2 Lite Custom) für den ganzen Flash aus der Sicherung und jl_isd.bin, wie werkzeuge/soll_bauen.py --dualbank:
//!   Bank A = jl_isd.bin, Rest bis 0xFA000 leer, VM + BTIF leer, ab 0xFD000 unverändert (Bluetooth-Adresse 0xFFF40).
//! - Rückweg zur Werks-Firmware aus dieser Sicherung: Die Werks-Firmware hat zwei
//!   Bänke (Bootinfo bei 0x3000 und 0x7C000, Container ab +0x20); die Brücke liegt in der beim Update freien Bank.
//!   Deren Bootinfo-Sektor wird gelöscht, dann startet der Bootloader die unveränderte Werks-Firmware der anderen Bank
//!   (so sah der Werks-Stand aus, von dem aus am 30.10. installiert wurde).

use crate::download::{crc16, sha256_hex};
use std::io::{Error, Result};
use std::path::PathBuf;

pub const WERK_URL: &str = "https://mopaioss.oss-cn-shenzhen.aliyuncs.com/firmware/C2-lite/C2-lite-0.04.bin";
pub const WERK_SHA256: &str = "4b91720d6f41b261f70128f80853be7d94364b7fcf6c941386da9b01eb94a17c";
const WARTE_AUF_USB: &[u8] = include_bytes!("../bruecke/warte_auf_usb.bin");
const ENC_SCHLUESSEL: u16 = 0xA80F;
const DATEN_ANFANG: usize = 0xC0; // App-Einsprung 0x01E000C0
const WERK_DOWNLOAD: usize = 0x156C; // werkseigene Download-Funktion, darf nicht überschrieben werden
pub const BEREICH_ENDE: usize = 0xFD000; // geschrieben wird 0..0xFD000, Bluetooth-Adresse dahinter bleibt
pub const RUECKWEG_ENDE: usize = 0xFF000; // Rückweg schreibt auch das Ende des Werks-VM zurück, nicht key_mac
pub const ADRESSE: std::ops::Range<usize> = 0xFFF40..0xFFF48; // Bluetooth-Adresse (6 B) + CRC16
const WERK_BAENKE: [usize; 2] = [0x3000, 0x7C000]; // Bootinfo der Werks-Bänke (app_dir_head / app_dir_head2)

fn enc_block(d: &mut [u8], mut k: u16) {
    for b in d {
        *b ^= k as u8;
        k = (k << 1) ^ if k & 0x8000 != 0 { 0x1021 } else { 0 };
    }
}

/// Jieli ENC über das ganze OTA-Abbild (symmetrisch): 32-Byte-Blöcke, Schlüssel 0xA80F ^ (Offset >> 2)
fn crypt(d: &[u8]) -> Vec<u8> {
    let mut o = d.to_vec();
    for (i, blk) in o.chunks_mut(32).enumerate() {
        enc_block(blk, ENC_SCHLUESSEL ^ (((i * 32) >> 2) as u16));
    }
    o
}

struct Eintrag {
    name: String,
    groesse: usize,
    ort: usize, // Lage des 32-Byte-Eintrags
}

/// JLFS-Verzeichnis am Anfang des entschlüsselten Abbilds
fn verzeichnis(p: &[u8]) -> Vec<Eintrag> {
    let mut v = Vec::new();
    let mut o = 0;
    while o + 32 <= p.len() {
        let h = &p[o..o + 32];
        if o > 0 && crc16(&h[2..], 0) != u16::from_le_bytes([h[0], h[1]]) {
            break;
        }
        let name = String::from_utf8_lossy(h[16..32].split(|&b| b == 0).next().unwrap_or(&[])).to_string();
        v.push(Eintrag { name, groesse: u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize, ort: o });
        o += 32;
    }
    v
}

/// Prüfsummenkette neu: app.bin (Daten + Kopf), app_area_head (Daten ab 0x20 + Kopf)
fn kette(p: &mut [u8]) -> Result<()> {
    let app = verzeichnis(p).into_iter().find(|e| e.name == "app.bin").ok_or_else(|| Error::other("app.bin fehlt"))?;
    let eo = app.ort;
    let dc = crc16(&p[DATEN_ANFANG..DATEN_ANFANG + app.groesse], 0);
    p[eo + 2..eo + 4].copy_from_slice(&dc.to_le_bytes());
    let hc = crc16(&p[eo + 2..eo + 32], 0);
    p[eo..eo + 2].copy_from_slice(&hc.to_le_bytes());
    let dc = crc16(&p[0x20..], 0);
    p[2..4].copy_from_slice(&dc.to_le_bytes());
    let hc = crc16(&p[2..32], 0);
    p[0..2].copy_from_slice(&hc.to_le_bytes());
    Ok(())
}

/// Brücke 2 aus der offiziellen 0.04 bauen; prüft vorher, dass die Prüfsummenkette das Original exakt reproduziert
pub fn bruecke_bauen(werk: &[u8]) -> Result<Vec<u8>> {
    if sha256_hex(werk) != WERK_SHA256 {
        return Err(Error::other("Die Werks-Firmware ist nicht die erwartete 0.04"));
    }
    let mut p = crypt(werk);
    let mut probe = p.clone();
    kette(&mut probe)?;
    if crypt(&probe) != werk {
        return Err(Error::other("Prüfsummenkette reproduziert das Original nicht"));
    }
    if p[DATEN_ANFANG..DATEN_ANFANG + 4] != [0x82, 0xea, 0x56, 0x1c] || DATEN_ANFANG + WARTE_AUF_USB.len() >= WERK_DOWNLOAD {
        return Err(Error::other("unerwarteter App-Anfang in der Werks-Firmware"));
    }
    p[DATEN_ANFANG..DATEN_ANFANG + WARTE_AUF_USB.len()].copy_from_slice(WARTE_AUF_USB);
    kette(&mut p)?;
    let raus = crypt(&p);
    if crypt(&raus)[DATEN_ANFANG..DATEN_ANFANG + WARTE_AUF_USB.len()] != *WARTE_AUF_USB {
        return Err(Error::other("Gegenprobe der Brücke fehlgeschlagen"));
    }
    Ok(raus)
}

/// Ganzer Flash für C2 Lite Custom aus der Sicherung (1 MB) und jl_isd.bin (Dual-Bank-Abbild); geschrieben wird 0..BEREICH_ENDE
pub fn custom_abbild(sicherung: &[u8], jl_isd: &[u8]) -> Result<Vec<u8>> {
    if sicherung.len() != 0x100000 {
        return Err(Error::other("Die Sicherung muss genau 1 MB groß sein"));
    }
    if jl_isd.len() % 0x1000 != 0 || jl_isd.len() > 0x7E000 || jl_isd.len() < 0x10000 {
        return Err(Error::other("jl_isd.bin hat eine unerwartete Größe"));
    }
    let mut s = sicherung.to_vec();
    s[..jl_isd.len()].copy_from_slice(jl_isd);
    s[jl_isd.len()..BEREICH_ENDE].fill(0xFF); // Bank B, VM, BTIF leer
    Ok(s)
}

/// Liegt in der Bank die Brücke? App-Anfang des Containers entschlüsseln und mit warte_auf_usb.bin vergleichen
fn ist_bruecke(sicherung: &[u8], bank: usize) -> bool {
    let c = bank + 0x20;
    let mut a = sicherung[c + DATEN_ANFANG..c + DATEN_ANFANG + 0x40].to_vec();
    for (i, blk) in a.chunks_mut(32).enumerate() {
        enc_block(blk, ENC_SCHLUESSEL ^ (((DATEN_ANFANG + i * 32) >> 2) as u16));
    }
    a[..WARTE_AUF_USB.len()] == *WARTE_AUF_USB
}

/// Bootinfo gültig und passt zum Container? (Kopf-CRC, Größe, CRC des verschlüsselten Containers)
fn bootinfo_gueltig(sicherung: &[u8], bank: usize) -> bool {
    let mut b = sicherung[bank..bank + 32].to_vec();
    enc_block(&mut b, 0xFFFF);
    let groesse = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    crc16(&b[2..], 0) == u16::from_le_bytes([b[0], b[1]])
        && groesse > 0x10000
        && bank + 0x20 + groesse <= sicherung.len()
        && crc16(&sicherung[bank + 0x20..bank + 0x20 + groesse], 0) == u16::from_le_bytes([b[4], b[5]])
}

/// Werks-Abbild aus der Sicherung der Installation: Bootinfo der Brücken-Bank gelöscht, sonst unverändert.
/// Geschrieben wird 0..RUECKWEG_ENDE; vorher prüft der Aufrufer die Bluetooth-Adresse (ADRESSE) gegen den Controller.
pub fn werk_abbild(sicherung: &[u8]) -> Result<Vec<u8>> {
    if sicherung.len() != 0x100000 {
        return Err(Error::other("Die Sicherung muss genau 1 MB groß sein"));
    }
    let Some(&bruecke) = WERK_BAENKE.iter().find(|&&b| ist_bruecke(sicherung, b)) else {
        return Err(Error::other("Das ist keine Sicherung aus „Installieren“ (keine Brücke gefunden)"));
    };
    let werk = WERK_BAENKE.into_iter().find(|&b| b != bruecke).unwrap();
    if !bootinfo_gueltig(sicherung, werk) {
        return Err(Error::other("Die Werks-Firmware in der Sicherung ist nicht vollständig, nicht verwendet"));
    }
    let mut s = sicherung.to_vec();
    s[bruecke..bruecke + 0x1000].fill(0xFF);
    Ok(s)
}

/// Cache-Ordner ~/.cache/c2lite-custom
pub fn zwischenspeicher() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("c2lite-custom")
}

/// Datei aus dem Zwischenspeicher oder per curl laden, per SHA-256 geprüft
pub fn holen(url: &str, sha256: &str, name: &str) -> Result<Vec<u8>> {
    let ordner = zwischenspeicher();
    let pfad = ordner.join(name);
    if let Ok(d) = std::fs::read(&pfad) {
        if sha256_hex(&d) == sha256 {
            return Ok(d);
        }
    }
    std::fs::create_dir_all(&ordner)?;
    let ok = std::process::Command::new("curl").args(["-fsSL", "-m", "120", "-o"]).arg(&pfad).arg(url).status()?.success();
    if !ok {
        return Err(Error::other(format!("{name} nicht herunterzuladen (Internet? curl installiert?)")));
    }
    let d = std::fs::read(&pfad)?;
    if sha256_hex(&d) != sha256 {
        let _ = std::fs::remove_file(&pfad);
        return Err(Error::other(format!("{name} hat eine falsche Prüfsumme, nicht verwendet")));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gegenproben gegen lokale Referenzdateien (enthalten Werks-Firmware bzw. Gerätedaten, nicht im Repo):
    /// Ordner in C2LITE_TESTDATEN, sonst werden die Tests übersprungen.
    fn lesen(name: &str) -> Option<Vec<u8>> {
        std::fs::read(PathBuf::from(std::env::var_os("C2LITE_TESTDATEN")?).join(name)).ok()
    }

    /// Brücke wie ein unabhängig erzeugtes Referenzabbild (werk-0.04.bin = offizielle Datei)
    #[test]
    fn bruecke_wie_referenz() {
        let (Some(werk), Some(soll)) = (lesen("werk-0.04.bin"), lesen("bruecke-referenz.bin")) else { return };
        assert!(bruecke_bauen(&werk).unwrap() == soll, "Brücke weicht von der Referenz ab");
    }

    /// Gesamtabbild wie werkzeuge/soll_bauen.py --dualbank
    #[test]
    fn custom_abbild_wie_soll_bauen() {
        let (Some(s), Some(jl), Some(soll)) = (lesen("werk-sicherung.bin"), lesen("jl_isd.bin"), lesen("soll.bin")) else {
            return;
        };
        assert!(custom_abbild(&s, &jl).unwrap() == soll, "Abbild weicht von soll_bauen.py ab");
    }

    /// Rückweg aus der Sicherung einer Installation gegen den Werks-Stand, der vorher lief: gleich bis auf Reste der
    /// Brücke hinter ihrer gelöschten Bootinfo (Brücke in Bank 2)
    #[test]
    fn werk_abbild_wie_vorher() {
        let (Some(s), Some(vorher)) = (lesen("sicherung-mit-bruecke.bin"), lesen("werk-vorher.bin")) else { return };
        assert!(ist_bruecke(&s, 0x7C000) && !ist_bruecke(&s, 0x3000));
        assert!(bootinfo_gueltig(&s, 0x3000) && bootinfo_gueltig(&vorher, 0x3000) && !bootinfo_gueltig(&vorher, 0x7C000));
        let w = werk_abbild(&s).unwrap();
        assert!(w[..0x7D000] == vorher[..0x7D000] && w[0xD2000..] == vorher[0xD2000..]);
        assert!(werk_abbild(&vorher).is_err(), "Sicherung ohne Brücke muss abgelehnt werden");
    }
}

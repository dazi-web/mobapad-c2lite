//! Verbindung zum Controller über USB-seriell (CDC) der eigenen Firmware.
//!
//! Die Firmware schickt laufend Protokollzeilen; Antworten auf Befehle beginnen mit einem festen Präfix
//! (`info:`, `status:`, `ok:`, `fehler:`, `einstellungen:`). Befehle enden mit `\r`.

use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

/// USB-Kennungen der eigenen Firmware: ab Stufe 10a die der Werks-Firmware (2345:e06e, PC-Modus als XInput; die
/// Werks-Firmware selbst hat keinen seriellen Port), davor Jieli-VID mit eigener PID. Unter Linux sucht finden() über
/// /dev/serial/by-id.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub const KENNUNGEN: &[(u16, u16)] = &[(0x2345, 0xe06e), (0x4c4a, 0x4155)];

/// Ein Weg zum Controller: USB-seriell oder Bluetooth, beide mit denselben Textbefehlen
pub trait Kanal {
    /// Befehl senden und auf die erste Zeile mit einem der Präfixe warten (Text ab dem Präfix)
    fn befehl(&mut self, befehl: &str, praefixe: &[&str], warten: Duration) -> Result<String, String>;
    fn name(&self) -> String;
    /// Befehl senden und alle Zeilen mit einem der Präfixe sammeln, bis `warten` abgelaufen ist
    fn befehl_alle(&mut self, befehl: &str, praefixe: &[&str], warten: Duration) -> Result<Vec<String>, String> {
        let mut zeilen = vec![self.befehl(befehl, praefixe, warten)?];
        // weitere Zeilen derselben Antwort: mit einem leeren Befehl abholen (Firmware ignoriert leere Zeilen)
        while let Ok(z) = self.befehl("", praefixe, Duration::from_millis(300)) {
            zeilen.push(z);
        }
        Ok(zeilen)
    }
}

pub struct Geraet {
    port: BufReader<Box<dyn serialport::SerialPort>>,
    pub pfad: String,
}

/// Sucht den seriellen Port des Controllers. Unter Linux über /dev/serial/by-id (ohne libudev), sonst über die
/// USB-Kennung, die serialport meldet.
pub fn finden() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let eintraege = std::fs::read_dir("/dev/serial/by-id").ok()?;
        let mut treffer: Vec<String> = eintraege
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_string_lossy().into_owned())
            .filter(|p| (p.contains("dazi") || p.contains("Jieli")) && p.ends_with("-if01"))
            .collect();
        treffer.sort();
        treffer.into_iter().next()
    }
    #[cfg(not(target_os = "linux"))]
    {
        serialport::available_ports().ok()?.into_iter().find_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) if KENNUNGEN.contains(&(u.vid, u.pid)) => Some(p.port_name),
            _ => None,
        })
    }
}

impl Geraet {
    pub fn oeffnen(pfad: &str) -> Result<Geraet, String> {
        let port = serialport::new(pfad, 115_200)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| format!("{pfad}: {e}"))?;
        let mut g = Geraet { port: BufReader::new(port), pfad: pfad.to_string() };
        // Ältere Firmware (vor Stufe 2r) verwirft das erste Paket nach dem Öffnen: eine leere Zeile vorweg
        g.schreiben("")?;
        std::thread::sleep(Duration::from_millis(100));
        g.leeren();
        Ok(g)
    }

    fn schreiben(&mut self, befehl: &str) -> Result<(), String> {
        let port = self.port.get_mut();
        port.write_all(format!("{befehl}\r").as_bytes()).map_err(|e| e.to_string())?;
        port.flush().map_err(|e| e.to_string())
    }

    /// Verwirft alles, was schon im Empfangspuffer liegt
    pub fn leeren(&mut self) {
        let _ = self.port.get_mut().clear(serialport::ClearBuffer::Input);
        let n = self.port.buffer().len();
        self.port.consume(n);
    }

    /// Nächste Zeile (ohne Zeilenende) oder None nach Ablauf der Port-Wartezeit
    pub fn zeile(&mut self) -> Option<String> {
        let mut roh = Vec::new();
        match self.port.read_until(b'\n', &mut roh) {
            Ok(n) if n > 0 => Some(String::from_utf8_lossy(&roh).trim_end().to_string()),
            _ => None,
        }
    }

}

impl Kanal for Geraet {
    fn name(&self) -> String {
        self.pfad.clone()
    }

    /// Schickt einen Befehl und wartet auf die erste Zeile, die eines der Präfixe enthält. Gibt den Text ab dem
    /// Präfix zurück (Protokollzeilen davor und Störzeichen am Zeilenanfang werden übersprungen).
    fn befehl(&mut self, befehl: &str, praefixe: &[&str], warten: Duration) -> Result<String, String> {
        self.schreiben(befehl)?;
        let ende = Instant::now() + warten;
        while Instant::now() < ende {
            if let Some(z) = self.zeile() {
                for p in praefixe {
                    if let Some(i) = crate::geraet::praefix_finden(&z, p) {
                        return Ok(z[i..].to_string());
                    }
                }
            }
        }
        Err(format!("keine Antwort auf „{befehl}“ (Firmware zu alt oder Controller beschäftigt?)"))
    }
}

/// Stelle eines Antwort-Präfixes in einer Zeile. Davor darf Störzeichen-Müll stehen, aber kein Buchstabe, keine
/// Ziffer und kein Unterstrich: sonst wäre es Teil eines anderen Worts (z. B. eine Protokollzeile mit „…_info:“).
pub fn praefix_finden(zeile: &str, p: &str) -> Option<usize> {
    zeile.match_indices(p).map(|(i, _)| i).find(|&i| {
        zeile[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
}

/// Zustand aus einer `status:`-Zeile
#[derive(Default, Clone)]
pub struct Status {
    pub tasten: u64,
    pub achsen: [u16; 4],
    pub akku: u8,
    pub usb: bool,
    pub led: u8,
}

impl Status {
    pub fn lesen(zeile: &str) -> Option<Status> {
        let mut s = Status::default();
        for teil in zeile.trim_start_matches("status:").split_whitespace() {
            let (k, v) = teil.split_once('=')?;
            match k {
                "tasten" => s.tasten = u64::from_str_radix(v, 16).ok()?,
                "lx" => s.achsen[0] = v.parse().ok()?,
                "ly" => s.achsen[1] = v.parse().ok()?,
                "rx" => s.achsen[2] = v.parse().ok()?,
                "ry" => s.achsen[3] = v.parse().ok()?,
                "akku" => s.akku = v.parse().ok()?,
                "usb" => s.usb = v == "1",
                "led" => s.led = u8::from_str_radix(v, 16).ok()?,
                _ => {}
            }
        }
        Some(s)
    }

    pub fn gedrueckt(&self, bit: u8) -> bool {
        self.tasten & (1u64 << bit) != 0
    }
}

/// Rollen-Bits der Firmware (enum rolle in c2_eingaben.c)
pub const TASTEN: &[(&str, u8)] = &[
    ("A", 1), ("B", 0), ("X", 3), ("Y", 4),
    ("L", 6), ("R", 7), ("ZL", 8), ("ZR", 9),
    ("−", 10), ("+", 11), ("Home", 12), ("Kamera", 15),
    ("L3", 13), ("R3", 14), ("Moba", 16), ("Licht", 17),
    ("Rück 1", 18), ("Rück 2", 19),
    ("↑", 32), ("→", 33), ("↓", 34), ("←", 35),
];
pub const SCHIEBER_PC3: u8 = 36;
pub const SCHIEBER_PA3: u8 = 37;

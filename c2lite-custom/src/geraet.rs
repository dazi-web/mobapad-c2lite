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
    /// USB: echtes Gerät hinter dem Port beim Öffnen (z. B. /dev/ttyACM0); ändert es sich, hat sich der Controller neu
    /// angemeldet (Neustart, Ruhezustand) und die Verbindung ist tot
    fn usb_ziel(&self) -> Option<std::path::PathBuf> {
        None
    }
    /// Befehl senden, ohne auf die Antwort zu warten (Klavier: „chitu live …“ antwortet mit „live:“, das andere Befehle
    /// beim Lesen überspringen). Standard: senden mit sehr kurzer Wartezeit.
    fn schicken(&mut self, befehl: &str) -> Result<(), String> {
        // Bei Erfolg antwortet die Firmware nicht (chitu live ab CRe): kurz senden, Zeitablauf ist kein Fehler
        match self.befehl(befehl, &["live:"], Duration::from_millis(20)) {
            Err(e) if crate::geraet::ist_verbindungsfehler(&e) => Err(e),
            _ => Ok(()),
        }
    }
    /// Alles abholen, was der Controller inzwischen geschrieben hat, ohne zu warten. Liest niemand den offenen Port, läuft
    /// der Puffer am PC voll und die Firmware hängt beim Senden (Watchdog-Neustart, Chitu 4.11.: Spur app=0x42).
    fn ablesen(&mut self) {}
    /// Beim Öffnen gesicherter Nachlass eines abgestürzten Laufs (nur einmal abholbar)
    fn nachlass_datei(&mut self) -> Option<std::path::PathBuf> {
        None
    }
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
    ziel: Option<std::path::PathBuf>,
    nachlass: Option<std::path::PathBuf>,
    /// angefangene Zeile: bleibt erhalten, wenn die Wartezeit mitten in einer Zeile abläuft (sonst ging der
    /// Zeilenanfang verloren, z. B. eine info-Zeile, deren Ende erst mit der nächsten Protokollzeile kommt)
    teil: Vec<u8>,
}

/// Alle Controller mit eigener Firmware am USB: (Port, Anzeigename). Unter Linux über /dev/serial/by-id (ohne
/// libudev), sonst über die USB-Kennung, die serialport meldet. Sortiert, damit die Reihenfolge stabil ist.
pub fn alle() -> Vec<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        let Ok(eintraege) = std::fs::read_dir("/dev/serial/by-id") else {
            return Vec::new();
        };
        let mut treffer: Vec<String> = eintraege
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_string_lossy().into_owned())
            .filter(|p| (p.contains("dazi") || p.contains("Jieli")) && p.ends_with("-if01"))
            .collect();
        treffer.sort();
        treffer.into_iter().map(|p| {
            let a = anzeige(&p);
            (p, a)
        }).collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        serialport::available_ports()
            .map(|v| {
                v.into_iter()
                    .filter_map(|p| match p.port_type {
                        serialport::SerialPortType::UsbPort(u) if KENNUNGEN.contains(&(u.vid, u.pid)) => {
                            let name = u.product.unwrap_or_default();
                            let serie = u.serial_number.unwrap_or_default();
                            Some((p.port_name.clone(), format!("{} {}", kurzname(&name), kurz(&serie)).trim().to_string()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Seriennummer aus dem by-id-Namen („usb-dazi_<Produkt>_<Serie>-if01“); bleibt gleich, auch wenn sich der
/// Produktname mit der Firmware ändert (Generalprobe-Firmware nennt sich am C2 Lite „Chitu 2 HD“)
pub fn serie(pfad: &str) -> String {
    let n = pfad.rsplit('/').next().unwrap_or(pfad).trim_end_matches("-if01");
    n.rsplit('_').next().unwrap_or("").to_string()
}

fn kurz(serie: &str) -> String {
    if serie.len() > 4 { format!("…{}", &serie[serie.len() - 4..]) } else { serie.to_string() }
}

fn kurzname(produkt: &str) -> &'static str {
    if produkt.contains("Chitu") { "Chitu 2 HD" } else { "C2 Lite" }
}

/// „Chitu 2 HD …3215“ bzw. „C2 Lite …9401“
pub fn anzeige(pfad: &str) -> String {
    format!("{} {}", kurzname(pfad), kurz(&serie(pfad)))
}

/// Gewählter Controller (Seriennummer bzw. Port), wenn mehrere stecken (Oberfläche)
static WAHL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn waehlen(serie_oder_port: Option<String>) {
    *WAHL.lock().unwrap() = serie_oder_port;
}

/// Port des Controllers: der gewählte, wenn er steckt, sonst der erste
pub fn finden() -> Option<String> {
    let alle = alle();
    let wahl = WAHL.lock().unwrap().clone();
    if let Some(w) = wahl {
        if let Some((p, _)) = alle.iter().find(|(p, _)| serie(p) == w || *p == w) {
            return Some(p.clone());
        }
    }
    alle.into_iter().next().map(|(p, _)| p)
}

impl Geraet {
    pub fn oeffnen(pfad: &str) -> Result<Geraet, String> {
        let port = serialport::new(pfad, 115_200)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| format!("{pfad}: {e}"))?;
        let ziel = std::fs::canonicalize(pfad).ok();
        let mut g = Geraet { port: BufReader::new(port), pfad: pfad.to_string(), ziel, nachlass: None, teil: Vec::new() };
        // Ältere Firmware (vor Stufe 2r) verwirft das erste Paket nach dem Öffnen: eine leere Zeile vorweg
        g.schreiben("")?;
        // Rückstau kurz mitlesen statt verwerfen: steht darin der Nachlass eines abgestürzten Laufs (die Firmware gibt
        // ihn nur einmal beim Hochfahren aus), wird er als Datei gesichert (Fehlersuche nach Watchdog-Neustarts)
        let mut rueckstau = String::new();
        let ende = Instant::now() + Duration::from_millis(1500);
        let mut still = 0;
        while Instant::now() < ende && rueckstau.len() < 256 * 1024 && still < 4 {
            match g.zeile() {
                Some(z) => {
                    still = 0;
                    rueckstau.push_str(&z);
                    rueckstau.push('\n');
                }
                None => still += 1,
            }
        }
        if let Some(n) = nachlass_finden(&rueckstau) {
            g.nachlass = nachlass_sichern(n);
        }
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
        self.teil.clear();
        let _ = self.port.get_mut().clear(serialport::ClearBuffer::Input);
        let n = self.port.buffer().len();
        self.port.consume(n);
    }

    /// Nächste Zeile (ohne Zeilenende) oder None nach Ablauf der Port-Wartezeit
    pub fn zeile(&mut self) -> Option<String> {
        // read_until hängt auch bei Zeitablauf schon Gelesenes an „teil“ an; erst bei „\n“ ist die Zeile fertig
        let _ = self.port.read_until(b'\n', &mut self.teil);
        if self.teil.last() == Some(&b'\n') {
            let z = String::from_utf8_lossy(&self.teil).trim_end().to_string();
            self.teil.clear();
            Some(z)
        } else {
            if self.teil.len() > 64 * 1024 {
                self.teil.clear(); // Schutz gegen endlose Zeilen
            }
            None
        }
    }

}

impl Kanal for Geraet {
    fn name(&self) -> String {
        self.pfad.clone()
    }

    fn usb_ziel(&self) -> Option<std::path::PathBuf> {
        self.ziel.clone()
    }

    fn nachlass_datei(&mut self) -> Option<std::path::PathBuf> {
        self.nachlass.take()
    }

    fn schicken(&mut self, befehl: &str) -> Result<(), String> {
        self.schreiben(befehl)
    }

    fn ablesen(&mut self) {
        for _ in 0..64 {
            let warten = self.port.get_ref().bytes_to_read().unwrap_or(0) > 0 || !self.port.buffer().is_empty();
            if !warten {
                break;
            }
            let _ = self.zeile();
        }
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
        Err(crate::sprache::tr("keine Antwort auf „{}“ (Firmware zu alt oder Controller beschäftigt?)", &[&befehl]))
    }
}

/// Nachlass-Block aus dem Rückstau (von „--- Nachlass des letzten Laufs“ bis „--- Ende Nachlass ---“)
pub fn nachlass_finden(text: &str) -> Option<&str> {
    let a = text.find("--- Nachlass des letzten Laufs")?;
    let rest = &text[a..];
    let e = rest.find("--- Ende Nachlass ---").map_or(rest.len(), |e| e + "--- Ende Nachlass ---".len());
    Some(&rest[..e])
}

/// Nachlass nach $XDG_CACHE_HOME bzw. ~/.cache/c2lite-custom/nachlass-<zeit>.txt, die neuesten 20 bleiben
fn nachlass_sichern(text: &str) -> Option<std::path::PathBuf> {
    let Some(basis) = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
    else {
        return None;
    };
    let ordner = basis.join("c2lite-custom");
    if std::fs::create_dir_all(&ordner).is_err() {
        return None;
    }
    let zeit = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let datei = ordner.join(format!("nachlass-{zeit}.txt"));
    let gesichert = std::fs::write(&datei, text).is_ok();
    let mut alte: Vec<_> = std::fs::read_dir(&ordner)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("nachlass-")))
        .collect();
    alte.sort();
    while alte.len() > 20 {
        let _ = std::fs::remove_file(alte.remove(0));
    }
    gesichert.then_some(datei)
}

/// Fehler beim Schreiben/Lesen, die bedeuten: der Port ist tot (Controller neu angemeldet oder abgezogen)
pub fn ist_verbindungsfehler(e: &str) -> bool {
    ["Broken pipe", "Datenübergabe unterbrochen", "Input/output error", "Eingabe-/Ausgabefehler", "No such device", "Kein passendes Gerät"]
        .iter()
        .any(|m| e.contains(m))
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
    /// Chitu ab CG4: Kontakt zum Ladedock (PA0 low), auch ohne Strom am Dock; None = Firmware meldet es nicht
    pub dock: Option<bool>,
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
                "pa0" => s.dock = Some(v == "0"),
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

#[cfg(test)]
mod tests {
    use super::nachlass_finden;

    #[test]
    fn status_dock() {
        // am Chitu 5.11. gemessen: im Dock ohne Strom, draußen am Kabel; C2 Lite ohne die Felder
        let s = super::Status::lesen("status: tasten=0000001000000000 lx=512 ly=512 rx=511 ry=512 akku=100 usb=0 led=04 dock=0 pa0=0").unwrap();
        assert_eq!((s.usb, s.dock), (false, Some(true)));
        let s = super::Status::lesen("status: tasten=0000001000000000 lx=512 ly=512 rx=511 ry=512 akku=100 usb=1 led=04 dock=0 pa0=1").unwrap();
        assert_eq!((s.usb, s.dock), (true, Some(false)));
        let s = super::Status::lesen("status: tasten=0000000000000000 lx=512 ly=512 rx=511 ry=512 akku=80 usb=1 led=04").unwrap();
        assert_eq!(s.dock, None);
    }

    #[test]
    fn serie_und_anzeige() {
        let c = "/dev/serial/by-id/usb-dazi_Chitu_2_HD_dazi_edition_1234567890ABCDEF-if01";
        let l = "/dev/serial/by-id/usb-dazi_C2_Lite_dazi_edition_FEDCBA0987654321-if01";
        assert_eq!(super::serie(c), "1234567890ABCDEF");
        assert_eq!(super::anzeige(c), "Chitu 2 HD …CDEF");
        assert_eq!(super::anzeige(l), "C2 Lite …4321");
    }

    #[test]
    fn nachlass_aus_rueckstau() {
        let t = "lebt 5 s\n--- Nachlass des letzten Laufs (Ende, 12 Zeichen) ---\nChitu-DAC: an\n\n--- Ende Nachlass ---\nlebt 10 s\n";
        let n = nachlass_finden(t).unwrap();
        assert!(n.starts_with("--- Nachlass des letzten Laufs"));
        assert!(n.ends_with("--- Ende Nachlass ---"));
        assert!(n.contains("Chitu-DAC: an"));
        // abgeschnitten (Ende fehlt): bis Textende
        assert!(nachlass_finden("x\n--- Nachlass des letzten Laufs ---\nabc").unwrap().ends_with("abc"));
        assert!(nachlass_finden("--- Kein Nachlass (Kaltstart) ---").is_none());
    }
}

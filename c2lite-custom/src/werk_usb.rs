//! USB-Weg in den ROM-Download-Modus über die Werks-Firmware (docs/WERKS-DOWNLOAD.md): Die Werks-Firmware springt bei
//! Home + Y + X (≈ 2 s) in den Masken-ROM-Lader, aber nur, solange „USB aktiv“ gesetzt ist. Im Xbox-Modus sendet sie nur
//! bei Änderungen; ohne USB-Interrupt löscht ihr Wächter den Merker nach ≈ 1,1 s. Der Wachhalter schickt deshalb alle
//! 250 ms einen kurzen Rumble-Impuls über evdev (Treiber xpad). Jeder ist ein USB-Bericht an den Controller.
//! Am Gerät erprobt am 31.10.2026 (Skript werkzeuge/usb_wachhalten.py, gleiche Werte).

use std::io::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

const STAERKE: u16 = 0x9000; // spürbar: Das Brummen zeigt dem Menschen, dass es läuft, und hört im Download-Modus auf
const DAUER_MS: u16 = 50;
const TAKT: Duration = Duration::from_millis(250); // deutlich unter den ≈ 1,1 s des Wächters

/// Eingabegerät (/dev/input/eventN) einer C2 Lite mit Werks-Firmware: USB 2345:e06e ohne „dazi“ im Produktnamen
pub fn eingabegeraet() -> Option<PathBuf> {
    let mut namen: Vec<_> = std::fs::read_dir("/sys/class/input").ok()?.flatten().map(|e| e.file_name()).collect();
    namen.sort();
    namen.into_iter().find_map(|n| {
        let n = n.to_string_lossy().to_string();
        if !n.starts_with("event") {
            return None;
        }
        let mut p = std::fs::canonicalize(format!("/sys/class/input/{n}/device")).ok()?;
        // Zum USB-Gerät hochgehen (das Verzeichnis mit idVendor)
        while !p.join("idVendor").exists() {
            p = p.parent()?.to_path_buf();
        }
        let lies = |d: &str| std::fs::read_to_string(p.join(d)).map(|t| t.trim().to_string()).unwrap_or_default();
        (lies("idVendor") == "2345" && lies("idProduct") == "e06e" && !lies("product").contains("dazi"))
            .then(|| PathBuf::from(format!("/dev/input/{n}")))
    })
}

type Rumpeln = crate::ff::Effekt;

/// Schickt im Hintergrund Rumble-Impulse, bis er fallen gelassen wird. Verschwindet das Eingabegerät (Ummelden der
/// Werks-Firmware nach dem Einstecken, Sprung in den Download-Modus), sucht er es neu.
pub struct Wachhalter {
    stopp: Arc<AtomicBool>,
    faden: Option<JoinHandle<()>>,
}

impl Wachhalter {
    /// Startet mit dem ersten gefundenen Eingabegerät; Fehler, wenn sich keins öffnen lässt (Rechte, kein Rumble)
    pub fn starten(geraet: &Path) -> Result<Wachhalter> {
        let erst = Rumpeln::neu(geraet, STAERKE, STAERKE, DAUER_MS)?;
        let stopp = Arc::new(AtomicBool::new(false));
        let s = stopp.clone();
        let faden = std::thread::spawn(move || {
            let mut aktiv = Some(erst);
            while !s.load(Ordering::Relaxed) {
                if aktiv.as_ref().is_some_and(|r| r.spielen(1).is_err()) {
                    aktiv = None;
                }
                if aktiv.is_none() {
                    aktiv = eingabegeraet().and_then(|p| Rumpeln::neu(&p, STAERKE, STAERKE, DAUER_MS).ok());
                }
                std::thread::sleep(TAKT);
            }
        });
        Ok(Wachhalter { stopp, faden: Some(faden) })
    }
}

impl Drop for Wachhalter {
    fn drop(&mut self) {
        self.stopp.store(true, Ordering::Relaxed);
        if let Some(f) = self.faden.take() {
            let _ = f.join();
        }
    }
}

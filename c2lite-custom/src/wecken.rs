//! Wecken einrichten: Die Switch 2 wacht nur bei Controllern auf, die sie per Bluetooth LE gekoppelt hat. Der C2 Lite
//! sendet sein Wecksignal deshalb mit der Kennung eines Joy-Con 2. Hier fängt der Laptop das Wecksignal eines
//! abgenommenen Joy-Con 2 auf (Herstellerdaten Nintendo 0x0553, am Gerät mitgeschnitten):
//!   01 00 03 7e 05 | Produkt-ID (LE) | 00 01 | 81 | Konsolenadresse (LE) | 0f 00 …

use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const NINTENDO: u16 = 0x0553;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kennung {
    pub pid: u16,
    /// Absender wie Linux ihn anzeigt, klein geschrieben (12:34:56:78:9a:bc)
    pub adresse: String,
    /// Konsole, die geweckt werden soll (aus dem Wecksignal); leer, wenn unbekannt
    pub konsole: String,
}

impl Kennung {
    pub fn name(&self) -> &'static str {
        geraet_name(self.pid)
    }

    /// Firmware-Befehl, der die Kennung dauerhaft im Controller speichert
    pub fn befehl(&self) -> String {
        format!("switch weck setzen {:04x} {}", self.pid, self.adresse)
    }

    /// Aus der Zeile von „switch diag“: „switch: weck pid=2066 absender 12:34:56:78:9a:bc status=0“.
    /// Absender 00:00:00:00:00:00 heißt: nicht eingerichtet (eigene Adresse, weckt die Switch 2 nicht).
    pub fn aus_diag(zeile: &str) -> Option<Option<Kennung>> {
        let rest = zeile.strip_prefix("switch: weck ")?;
        let mut t = rest.split_whitespace();
        let pid = u16::from_str_radix(t.next()?.strip_prefix("pid=")?, 16).ok()?;
        (t.next()? == "absender").then_some(())?;
        let adresse = t.next()?.to_string();
        if adresse == "00:00:00:00:00:00" {
            return Some(None);
        }
        Some(Some(Kennung { pid, adresse, konsole: String::new() }))
    }
}

pub fn geraet_name(pid: u16) -> &'static str {
    match pid {
        0x2066 => "Joy-Con 2 (L)",
        0x2067 => "Joy-Con 2 (R)",
        0x2069 => "Pro Controller 2",
        _ => "Nintendo-Controller",
    }
}

/// Wecksignal erkennen: Produkt-ID und Konsolenadresse (Anzeigereihenfolge)
fn lesen(d: &[u8]) -> Option<(u16, String)> {
    if d.len() < 16 || d[0] != 0x01 || d[3] != 0x7e || d[4] != 0x05 || d[9] != 0x81 {
        return None;
    }
    let pid = u16::from_le_bytes([d[5], d[6]]);
    let konsole = d[10..16].iter().rev().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":");
    Some((pid, konsole))
}

/// Lauscht höchstens `dauer` auf ein Wecksignal; `abbruch` beendet vorzeitig
pub fn auffangen(dauer: Duration, abbruch: &AtomicBool) -> Result<Kennung, String> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(lauschen(dauer, abbruch))
}

async fn lauschen(dauer: Duration, abbruch: &AtomicBool) -> Result<Kennung, String> {
    let manager = Manager::new().await.map_err(|e| format!("Bluetooth: {e}"))?;
    let adapter = manager
        .adapters()
        .await
        .map_err(|e| format!("Bluetooth: {e}"))?
        .into_iter()
        .next()
        .ok_or("kein Bluetooth-Adapter gefunden")?;
    let mut events = adapter.events().await.map_err(|e| e.to_string())?;
    adapter.start_scan(ScanFilter::default()).await.map_err(|e| format!("Suche: {e}"))?;
    let ende = Instant::now() + dauer;
    let ergebnis = loop {
        if abbruch.load(Ordering::Relaxed) {
            break Err("abgebrochen".to_string());
        }
        if Instant::now() > ende {
            break Err("kein Wecksignal empfangen (Switch im Standby? Joy-Con abgenommen und Taste gedrückt?)".into());
        }
        // Meldungen der Suche; daneben die Geräte, die in dieser Suche gesehen wurden (RSSI gesetzt), falls BlueZ
        // gleiche Daten nicht noch einmal meldet
        let mut kandidaten = Vec::new();
        if let Ok(Some(CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data })) =
            tokio::time::timeout(Duration::from_millis(250), events.next()).await
        {
            if let Some(d) = manufacturer_data.get(&NINTENDO) {
                if let Ok(p) = adapter.peripheral(&id).await {
                    kandidaten.push((p.address().to_string(), d.clone()));
                }
            }
        }
        for p in adapter.peripherals().await.unwrap_or_default() {
            if let Ok(Some(props)) = p.properties().await {
                if let (Some(_), Some(d)) = (props.rssi, props.manufacturer_data.get(&NINTENDO)) {
                    kandidaten.push((props.address.to_string(), d.clone()));
                }
            }
        }
        if let Some(k) = kandidaten.into_iter().find_map(|(adr, d)| {
            lesen(&d).map(|(pid, konsole)| Kennung { pid, adresse: adr.to_lowercase(), konsole })
        }) {
            break Ok(k);
        }
    };
    let _ = adapter.stop_scan().await;
    ergebnis
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joycon_signal() {
        // Aufbau wie ein mitgeschnittenes Joy-Con-2-Signal (L), Adressen durch Beispielwerte ersetzt: Konsole 11:22:33:44:55:66
        let d = [
            0x01, 0x00, 0x03, 0x7e, 0x05, 0x66, 0x20, 0x00, 0x01, 0x81, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x0f, 0, 0,
            0, 0, 0, 0, 0,
        ];
        assert_eq!(lesen(&d), Some((0x2066, "11:22:33:44:55:66".into())));
        let mut anders = d;
        anders[9] = 0x80;
        assert_eq!(lesen(&anders), None);
    }

    #[test]
    fn diag_zeile() {
        let k = Kennung::aus_diag("switch: weck pid=2066 absender 12:34:56:78:9a:bc status=0").unwrap().unwrap();
        assert_eq!(k.befehl(), "switch weck setzen 2066 12:34:56:78:9a:bc");
        assert_eq!(Kennung::aus_diag("switch: weck pid=2069 absender 00:00:00:00:00:00 status=0"), Some(None));
    }
}

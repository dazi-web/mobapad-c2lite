//! Bluetooth-Verbindung zum Controller: dieselben Textbefehle wie über USB-seriell.
//! Firmware: Dienst ae40, Befehle schreiben auf ae41 (ohne Antwort), Antworten als Benachrichtigung auf ae42.

use crate::geraet::Kanal;
use btleplug::api::{Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use uuid::Uuid;

const SCHREIBEN: Uuid = Uuid::from_u128(0x0000ae41_0000_1000_8000_00805f9b34fb);
const MELDUNG: Uuid = Uuid::from_u128(0x0000ae42_0000_1000_8000_00805f9b34fb);
/// Die Firmware hält Bluetooth-Pakete auf 20 Byte, damit sie in jede MTU passen
const STUECK: usize = 20;

pub struct BleGeraet {
    rt: Runtime,
    geraet: Peripheral,
    schreiben: Characteristic,
    empfang: Arc<Mutex<Vec<u8>>>,
    name: String,
}

impl BleGeraet {
    /// Sucht den Controller (Name enthält „C2 Lite“ oder passende Adresse) und verbindet sich
    pub fn verbinden(adresse: Option<&str>) -> Result<BleGeraet, String> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let (geraet, name) = rt.block_on(suchen(adresse))?;
        let (schreiben, empfang) = rt.block_on(einrichten(&geraet))?;
        Ok(BleGeraet { rt, geraet, schreiben, empfang, name })
    }
}

async fn suchen(adresse: Option<&str>) -> Result<(Peripheral, String), String> {
    let manager = Manager::new().await.map_err(|e| format!("Bluetooth: {e}"))?;
    let adapter = manager
        .adapters()
        .await
        .map_err(|e| format!("Bluetooth: {e}"))?
        .into_iter()
        .next()
        .ok_or("kein Bluetooth-Adapter gefunden")?;
    let passt = |name: &Option<String>, adr: &str| -> bool {
        match adresse {
            Some(a) => adr.eq_ignore_ascii_case(a),
            None => name.as_deref().is_some_and(|n| n.contains("C2 Lite")),
        }
    };
    // Schon verbundene oder bekannte Geräte zuerst, sonst kurz suchen
    let mut events = adapter.events().await.map_err(|e| e.to_string())?;
    adapter.start_scan(ScanFilter::default()).await.map_err(|e| format!("Suche: {e}"))?;
    let ende = Instant::now() + Duration::from_secs(15);
    loop {
        for p in adapter.peripherals().await.map_err(|e| e.to_string())? {
            if let Ok(Some(props)) = p.properties().await {
                let adr = props.address.to_string();
                if passt(&props.local_name, &adr) {
                    let _ = adapter.stop_scan().await;
                    return Ok((p, format!("{} ({adr})", props.local_name.unwrap_or_default())));
                }
            }
        }
        if Instant::now() > ende {
            let _ = adapter.stop_scan().await;
            return Err("Controller per Bluetooth nicht gefunden (an, in Reichweite, gekoppelt?)".into());
        }
        let _ = tokio::time::timeout(Duration::from_millis(500), async {
            while let Some(e) = events.next().await {
                if matches!(e, CentralEvent::DeviceDiscovered(_) | CentralEvent::DeviceUpdated(_)) {
                    break;
                }
            }
        })
        .await;
    }
}

async fn einrichten(geraet: &Peripheral) -> Result<(Characteristic, Arc<Mutex<Vec<u8>>>), String> {
    if !geraet.is_connected().await.unwrap_or(false) {
        geraet.connect().await.map_err(|e| format!("Verbinden: {e}"))?;
    }
    geraet.discover_services().await.map_err(|e| format!("Dienste: {e}"))?;
    let merkmale = geraet.characteristics();
    let finde = |u: Uuid| merkmale.iter().find(|c| c.uuid == u).cloned();
    let schreiben = finde(SCHREIBEN).ok_or("Befehlskanal ae41 fehlt (Firmware zu alt?)")?;
    let meldung = finde(MELDUNG).ok_or("Antwortkanal ae42 fehlt (Firmware zu alt?)")?;
    geraet.subscribe(&meldung).await.map_err(|e| format!("Abonnieren: {e}"))?;
    let empfang = Arc::new(Mutex::new(Vec::new()));
    let ziel = empfang.clone();
    let mut strom = geraet.notifications().await.map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        while let Some(n) = strom.next().await {
            if n.uuid == MELDUNG {
                ziel.lock().unwrap().extend_from_slice(&n.value);
            }
        }
    });
    Ok((schreiben, empfang))
}

impl Kanal for BleGeraet {
    fn befehl(&mut self, befehl: &str, praefixe: &[&str], warten: Duration) -> Result<String, String> {
        let daten = format!("{befehl}\r").into_bytes();
        let (geraet, schreiben, empfang) = (&self.geraet, &self.schreiben, self.empfang.clone());
        self.rt.block_on(async move {
            if daten.len() > 1 {
                empfang.lock().unwrap().clear(); /* leerer Befehl: nur weitere Zeilen abholen */
            }
            for stueck in daten.chunks(STUECK) {
                geraet
                    .write(schreiben, stueck, WriteType::WithoutResponse)
                    .await
                    .map_err(|e| format!("Senden: {e}"))?;
            }
            let ende = Instant::now() + warten;
            while Instant::now() < ende {
                {
                    let mut puffer = empfang.lock().unwrap();
                    while let Some(pos) = puffer.iter().position(|&b| b == b'\n') {
                        let zeile: Vec<u8> = puffer.drain(..=pos).collect();
                        let zeile = String::from_utf8_lossy(&zeile).trim().to_string();
                        for p in praefixe {
                            if let Some(i) = crate::geraet::praefix_finden(&zeile, p) {
                                return Ok(zeile[i..].to_string());
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(format!("keine Antwort auf „{befehl}“ über Bluetooth"))
        })
    }

    fn name(&self) -> String {
        format!("Bluetooth {}", self.name)
    }
}

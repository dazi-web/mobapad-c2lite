//! Bluetooth am PC (ab Firmware-Stufe 13): Der C2 Lite meldet sich über BLE als Xbox Wireless Controller 1708
//! (045E:02FD), Linux bindet hid-microsoft und bietet Rumble an. Hier: Gerät finden und die Vibration über genau diesen
//! Weg testen (PC → Bluetooth → Controller), nicht über den Befehlskanal.

use std::path::PathBuf;

/// Per Bluetooth als Gamepad verbundener C2 Lite
#[derive(Clone, Debug, PartialEq)]
pub struct BtGamepad {
    /// /dev/input/eventN des Gamepads (nicht „Consumer Control“)
    pub event: PathBuf,
    /// gebundener HID-Treiber, z. B. „microsoft“ (Vibration) oder „hid-generic“ (alte Firmware)
    pub treiber: String,
    /// Linux bietet Force Feedback an
    pub vibration: bool,
}

/// Sucht einen per Bluetooth verbundenen C2 Lite (HID 0005:045E:02FD, Name mit „C2 Lite“); Ältere Firmware (vor Stufe 13)
/// meldete sich anders und wird nicht gefunden
pub fn finden() -> Option<BtGamepad> {
    let mut geraete: Vec<_> = std::fs::read_dir("/sys/bus/hid/devices").ok()?.flatten().map(|e| e.path()).collect();
    geraete.sort();
    geraete.into_iter().find_map(|hid| {
        let name = hid.file_name()?.to_string_lossy().to_uppercase();
        if !name.starts_with("0005:045E:02FD.") {
            return None;
        }
        let treiber = std::fs::read_link(hid.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        let inputs = std::fs::read_dir(hid.join("input")).ok()?;
        for input in inputs.flatten() {
            let p = input.path();
            let n = std::fs::read_to_string(p.join("name")).unwrap_or_default();
            if !n.contains("C2 Lite") || n.contains("Consumer") {
                continue;
            }
            let ff = std::fs::read_to_string(p.join("capabilities/ff")).unwrap_or_default();
            let vibration = ff.split_whitespace().any(|w| w != "0");
            let event = std::fs::read_dir(&p).ok()?.flatten().map(|e| e.file_name().to_string_lossy().to_string()).find(|n| n.starts_with("event"))?;
            return Some(BtGamepad { event: PathBuf::from(format!("/dev/input/{event}")), treiber: treiber.clone(), vibration });
        }
        None
    })
}

/// Links (stark), Pause, rechts (schwach), je 0,6 s; blockiert etwa 2 s
pub fn vibration_testen(g: &BtGamepad) -> Result<(), String> {
    let fehler = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!("Kein Zugriff auf {} (Rechte)", g.event.display()),
        _ => format!("{}: {e}", g.event.display()),
    };
    for (stark, schwach) in [(0xFFFF, 0), (0, 0xFFFF)] {
        let e = crate::ff::Effekt::neu(&g.event, stark, schwach, 600).map_err(fehler)?;
        e.spielen(1).map_err(fehler)?;
        std::thread::sleep(std::time::Duration::from_millis(900));
    }
    Ok(())
}

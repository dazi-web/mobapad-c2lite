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
            if !(n.contains("C2 Lite") || n.contains("Chitu")) || n.contains("Consumer") {
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
        std::io::ErrorKind::PermissionDenied => crate::sprache::tr("Kein Zugriff auf {} (Rechte)", &[&g.event.display()]),
        _ => format!("{}: {e}", g.event.display()),
    };
    for (stark, schwach) in [(0xFFFF, 0), (0, 0xFFFF)] {
        let e = crate::ff::Effekt::neu(&g.event, stark, schwach, 600).map_err(fehler)?;
        e.spielen(1).map_err(fehler)?;
        std::thread::sleep(std::time::Duration::from_millis(900));
    }
    Ok(())
}

/// Kopplungen des Rechners (BlueZ) mit C2 Lite Custom entfernen; liefert die entfernten Adressen. Nach einer Installation
/// ist der Einstellungsspeicher des Controllers frisch: Nur der Rechner kennt noch den alten Schlüssel, BlueZ verbindet
/// dann etwa einmal pro Sekunde, jedes Lesen scheitert, und der Controller „sucht“ ohne Ende (am Gerät 2.11.).
/// Erkannt am Namen, den die Firmware wirbt („C2 Lite dazi“ bzw. „Chitu dazi“, BlueZ hängt „(BLE)“ an); nur die der
/// gerade installierten Platine, damit die Kopplung eines zweiten Controllers bleibt. Ohne bluetoothctl: nichts.
pub fn alte_kopplungen_entfernen(name: &str) -> Vec<String> {
    let Ok(aus) = std::process::Command::new("bluetoothctl").arg("devices").output() else { return Vec::new() };
    adressen(&String::from_utf8_lossy(&aus.stdout), name)
        .into_iter()
        .filter(|adr| {
            let r = std::process::Command::new("bluetoothctl").args(["remove", adr]).output();
            matches!(r, Ok(r) if r.status.success())
        })
        .collect()
}

/// Adressen aus „bluetoothctl devices“, deren Name mit `name` beginnt
fn adressen(liste: &str, name_anfang: &str) -> Vec<String> {
    liste
        .lines()
        .filter_map(|z| {
            // „Device XX:XX:XX:XX:XX:XX C2 Lite dazi(BLE)“
            let (adr, name) = z.strip_prefix("Device ")?.split_once(' ')?;
            name.starts_with(name_anfang).then(|| adr.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn nur_c2_lite_custom() {
        let liste = "Device 11:22:33:44:55:66 Lautsprecher\n\
                     Device 11:22:33:44:55:77 C2 Lite dazi(BLE)\n\
                     Device 11:22:33:44:55:88 Mobapad C2-lite\n\
                     Device 11:22:33:44:55:99 Xbox Wireless Controller\n\
                     Device 11:22:33:44:55:AA Chitu dazi(BLE)\n\
                     Device 11:22:33:44:55:BB Mobapad-Pro2-HD\n";
        assert_eq!(super::adressen(liste, "C2 Lite dazi"), ["11:22:33:44:55:77"]);
        assert_eq!(super::adressen(liste, "Chitu dazi"), ["11:22:33:44:55:AA"]);
    }
}

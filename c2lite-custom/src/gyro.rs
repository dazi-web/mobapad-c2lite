//! Gyro-Test am Chitu 2 HD (Firmware ab chitu-CG3): Bewegungssensor so lesen, wie er im Switch-Bericht 0x30 steht.
//! Ablauf: „chitu gyro dauer 1“ (Dauerlesen auch in Stellung PC), dann wiederholt „chitu gyro bericht“, am Ende
//! „chitu gyro dauer 0“. Achsen wie im Bericht (am Gerät belegt 5.11.): +X zu den Schultertasten, +Y nach links,
//! +Z nach oben (Tasten); Kippen nach vorn = Gyro Y positiv.
//! Skalen: Beschleunigung ±8 g = 4096 je g (am Gerät: flach |a| ≈ 4110). Gyro: Sensor ±2000 °/s = 70 m°/s je Schritt,
//! die Firmware rechnet wie die Werks-FW ×258/255 → ≈ 0,0692 °/s je Schritt (Vermutung bis zum Abgleich mit einem
//! echten Pro Controller).

use crate::geraet::Kanal;
use crate::sprache::{t, tr};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const A_JE_G: f32 = 4096.0;
pub const DPS_JE_SCHRITT: f32 = 0.070 * 255.0 / 258.0;

/// Ab hier gilt eine Achse als „gedreht“ (°/s); ruhig liegend rauscht es um 0,5 °/s
pub const DREH_SCHWELLE: f32 = 90.0;
/// Ruhe: Betrag 1 g ± 10 % und jede Gyro-Achse unter 3 °/s, mindestens 1 s am Stück
const RUHE_DPS: f32 = 3.0;
const RUHE_DAUER: Duration = Duration::from_secs(1);
/// Lage erkannt ab 85 % der Schwerkraft auf einer Achse
const LAGE_G: f32 = 0.85;

/// Eine Probe aus „ok: gyro bericht gueltig=1 alter=0 a=x,y,z g=x,y,z | …“ (erste, neueste Probe)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Probe {
    pub gueltig: bool,
    pub alter: u8,
    pub a: [i16; 3],
    pub g: [i16; 3],
}

fn drei(v: &str) -> Option<[i16; 3]> {
    let mut it = v.split(',').map(|x| x.trim().parse::<i16>());
    let r = [it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?];
    it.next().is_none().then_some(r)
}

impl Probe {
    pub fn lesen(zeile: &str) -> Option<Probe> {
        let i = crate::geraet::praefix_finden(zeile, "ok: gyro bericht")?;
        let rest = &zeile[i + "ok: gyro bericht".len()..];
        let erste = rest.split('|').next()?;
        let (mut gueltig, mut alter, mut a, mut g) = (None, None, None, None);
        for teil in erste.split_whitespace() {
            let (k, v) = teil.split_once('=')?;
            match k {
                "gueltig" => gueltig = Some(v == "1"),
                "alter" => alter = v.parse().ok(),
                "a" => a = drei(v),
                "g" => g = drei(v),
                _ => {}
            }
        }
        Some(Probe { gueltig: gueltig?, alter: alter?, a: a?, g: g? })
    }

    /// Beschleunigung in g
    pub fn a_g(&self) -> [f32; 3] {
        self.a.map(|v| v as f32 / A_JE_G)
    }

    /// Drehrate in °/s
    pub fn g_dps(&self) -> [f32; 3] {
        self.g.map(|v| v as f32 * DPS_JE_SCHRITT)
    }

    pub fn betrag_g(&self) -> f32 {
        self.a_g().iter().map(|v| v * v).sum::<f32>().sqrt()
    }
}

/// Sensorstand aus „ok: gyro status init=1 …“: 1 bereit, 0 noch nicht eingestellt, -1 kein Sensor
pub fn init_lesen(zeile: &str) -> Option<i32> {
    zeile.split_whitespace().find_map(|w| w.strip_prefix("init=")).and_then(|v| v.parse().ok())
}

/// Namen der Drehachsen für Laien (Reihenfolge X, Y, Z)
pub const DREH_NAMEN: [&str; 3] = ["Rollen (wie ein Lenkrad)", "Kippen nach vorn/hinten", "Drehen flach auf dem Tisch"];

/// Fortschritt des geführten Tests
#[derive(Clone, Debug, Default)]
pub struct Pruefung {
    pub proben: u32,
    pub ungueltig: u32,
    pub ruhe: bool,
    ruhe_seit: Option<Instant>,
    /// Mittel der Drehrate in Ruhe (°/s), Nullpunkt-Fehler
    pub ruhe_mittel: [f32; 3],
    ruhe_summe: [f32; 3],
    ruhe_n: u32,
    pub flach: bool,
    pub hochkant: bool,
    pub dreh: [bool; 3],
    pub max_dps: [f32; 3],
}

impl Pruefung {
    pub fn neu(&mut self, p: &Probe, jetzt: Instant) {
        self.proben += 1;
        if !p.gueltig {
            self.ungueltig += 1;
            self.ruhe_seit = None;
            return;
        }
        let a = p.a_g();
        let g = p.g_dps();
        let betrag = p.betrag_g();
        let still = (0.9..=1.1).contains(&betrag) && g.iter().all(|v| v.abs() < RUHE_DPS);
        if still {
            let seit = *self.ruhe_seit.get_or_insert(jetzt);
            if jetzt.duration_since(seit) >= RUHE_DAUER {
                self.ruhe = true;
            }
            for i in 0..3 {
                self.ruhe_summe[i] += g[i];
            }
            self.ruhe_n += 1;
            self.ruhe_mittel = self.ruhe_summe.map(|s| s / self.ruhe_n as f32);
        } else {
            self.ruhe_seit = None;
        }
        // Lagen nur ohne starke Drehung (sonst zählt ein Schwenk durch die Lage mit)
        let ruhig = g.iter().all(|v| v.abs() < 30.0);
        if ruhig && a[2] > LAGE_G {
            self.flach = true;
        }
        if ruhig && a[0] < -LAGE_G {
            self.hochkant = true;
        }
        for i in 0..3 {
            self.max_dps[i] = self.max_dps[i].max(g[i].abs());
            if g[i].abs() >= DREH_SCHWELLE {
                self.dreh[i] = true;
            }
        }
    }

    /// Schritte mit Text und Stand (für Liste und Abschluss)
    pub fn schritte(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("Ruhig liegen lassen (1 s)", self.ruhe),
            ("Flach hinlegen, Tasten nach oben", self.flach),
            ("Auf die Oberkante stellen (Schultertasten unten)", self.hochkant),
            (DREH_NAMEN[0], self.dreh[0]),
            (DREH_NAMEN[1], self.dreh[1]),
            (DREH_NAMEN[2], self.dreh[2]),
        ]
    }

    pub fn fertig(&self) -> bool {
        self.schritte().iter().all(|(_, ok)| *ok)
    }
}

/// Lage im Raum für die 3D-Ansicht (Mahony-Filter ohne I-Anteil): Drehrate aufintegrieren, Neigen/Rollen langsam zur
/// Schwerkraft ziehen. Gieren (Drehen um die Hochachse) hat keinen Bezug und driftet; `ausrichten` setzt es auf 0.
/// `r` bildet Gerätekoordinaten auf Weltkoordinaten ab (Zeilen = Weltachsen in Gerätekoordinaten), Welt: X weg vom
/// Spieler, Y links, Z oben – liegt der Controller flach mit den Schultertasten nach vorn, ist r die Einheitsmatrix.
#[derive(Clone, Debug)]
pub struct Lage {
    pub r: [[f32; 3]; 3],
    bereit: bool,
}

impl Default for Lage {
    fn default() -> Lage {
        Lage { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], bereit: false }
    }
}

/// Zug zur Schwerkraft (1/s): klein genug, dass Schwenks nicht wackeln, groß genug gegen Drift
const LAGE_KP: f32 = 1.5;

fn kreuz(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn skalar(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f32; 3]) -> Option<[f32; 3]> {
    let l = skalar(a, a).sqrt();
    (l > 1e-6).then(|| a.map(|v| v / l))
}

impl Lage {
    /// Neigen/Rollen aus der Schwerkraft, Gieren 0 (Schultertasten zeigen vom Spieler weg)
    pub fn ausrichten(&mut self, p: &Probe) {
        let Some(z) = norm(p.a_g()) else { return };
        let x = norm({
            let k = z[0];
            [1.0 - k * z[0], -k * z[1], -k * z[2]]
        })
        .or_else(|| norm([-z[1] * z[0], 1.0 - z[1] * z[1], -z[1] * z[2]]));
        let Some(x) = x else { return };
        let y = kreuz(z, x);
        self.r = [x, y, z];
        self.bereit = true;
    }

    /// Eine Probe nach `dt` Sekunden; `nullpunkt` = Drehrate in Ruhe (°/s), wird abgezogen
    pub fn neu(&mut self, p: &Probe, dt: f32, nullpunkt: [f32; 3]) {
        if !p.gueltig {
            return;
        }
        if !self.bereit {
            self.ausrichten(p);
            return;
        }
        let dt = dt.clamp(0.0, 0.1);
        let g = p.g_dps();
        let mut w = [0.0f32; 3];
        for i in 0..3 {
            w[i] = (g[i] - nullpunkt[i]).to_radians();
        }
        // Schwerkraft nur nutzen, wenn |a| nahe 1 g (sonst beschleunigt der Spieler gerade)
        let betrag = p.betrag_g();
        if (0.8..=1.2).contains(&betrag) {
            let a = p.a_g().map(|v| v / betrag);
            let v = self.r[2]; // Welt-oben in Gerätekoordinaten (Zeile, nicht Spalte)
            let e = kreuz(a, v);
            for i in 0..3 {
                w[i] += LAGE_KP * e[i];
            }
        }
        // r = r · exp([w]× dt) (Drehung im Gerätesystem, Rodrigues)
        let winkel = skalar(w, w).sqrt() * dt;
        if winkel > 1e-7 {
            let k = w.map(|v| v * dt / winkel);
            let (s, c) = winkel.sin_cos();
            let t = 1.0 - c;
            let d = [
                [c + k[0] * k[0] * t, k[0] * k[1] * t - k[2] * s, k[0] * k[2] * t + k[1] * s],
                [k[1] * k[0] * t + k[2] * s, c + k[1] * k[1] * t, k[1] * k[2] * t - k[0] * s],
                [k[2] * k[0] * t - k[1] * s, k[2] * k[1] * t + k[0] * s, c + k[2] * k[2] * t],
            ];
            let alt = self.r;
            for i in 0..3 {
                for j in 0..3 {
                    self.r[i][j] = (0..3).map(|m| alt[i][m] * d[m][j]).sum();
                }
            }
            // Rundungsfehler: Zeilen wieder senkrecht und Länge 1
            if let (Some(x), Some(z0)) = (norm(self.r[0]), norm(self.r[2])) {
                let y = norm(kreuz(z0, x)).unwrap_or(self.r[1]);
                let z = kreuz(x, y);
                self.r = [x, y, z];
            }
        }
    }

    /// Gerätepunkt in Weltkoordinaten
    pub fn welt(&self, v: [f32; 3]) -> [f32; 3] {
        [skalar(self.r[0], v), skalar(self.r[1], v), skalar(self.r[2], v)]
    }
}

/// Ist ein Chitu mit Gyro-Firmware am anderen Ende? Gibt den Sensorstand zurück.
pub fn vorbereiten(g: &mut dyn Kanal) -> Result<i32, String> {
    let info = g.befehl("info", &["info:"], Duration::from_secs(2))?;
    if !crate::musik::ist_chitu(&info) {
        return Err(t("Gyro-Test nur am Chitu 2 HD: dieser Controller meldet sich nicht als Chitu.").into());
    }
    let z = g.befehl("chitu gyro status", &["ok: gyro status", "fehler:"], Duration::from_secs(2))
        .map_err(|_| t("Die Firmware kennt den Gyro noch nicht (ab chitu-CG3). Bitte zuerst aktualisieren.").to_string())?;
    init_lesen(&z).ok_or_else(|| tr("unerwartete Antwort: {}", &[&z]))
}

/// Dauerlesen ein/aus (in Stellung Switch liest die Firmware ohnehin, sobald die Konsole es verlangt)
pub fn dauer(g: &mut dyn Kanal, an: bool) -> Result<(), String> {
    let z = g.befehl(&format!("chitu gyro dauer {}", an as u8), &["ok: gyro dauer", "fehler:"], Duration::from_secs(2))?;
    if z.starts_with("fehler:") {
        return Err(z);
    }
    Ok(())
}

pub fn probe_holen(g: &mut dyn Kanal) -> Result<Probe, String> {
    let z = g.befehl("chitu gyro bericht", &["ok: gyro bericht", "fehler:"], Duration::from_millis(600))?;
    Probe::lesen(&z).ok_or(z)
}

/// Strg+C beendet die Schleifen sauber, damit „dauer 0“ noch rausgeht (sonst liest die Firmware bis zum Neustart weiter)
static ABBRUCH: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
extern "C" fn bei_strg_c(_: libc::c_int) {
    ABBRUCH.store(true, Ordering::Relaxed);
}

fn strg_c_abfangen() {
    ABBRUCH.store(false, Ordering::Relaxed);
    #[cfg(target_os = "linux")]
    unsafe {
        libc::signal(libc::SIGINT, bei_strg_c as *const () as libc::sighandler_t);
    }
}

fn abgebrochen() -> bool {
    ABBRUCH.load(Ordering::Relaxed)
}

/// Kommandozeile: „gyro“ = geführter Test, „gyro live“ = nur Werte zeigen (Strg+C beendet)
pub fn befehl(g: &mut dyn Kanal, a: &[String]) -> Result<(), String> {
    let nur_live = a.first().map(String::as_str) == Some("live");
    match vorbereiten(g)? {
        1 => {}
        0 => return Err(t("Der Gyro wird gerade eingestellt. In ein paar Sekunden noch einmal versuchen.").into()),
        _ => return Err(t("Kein Bewegungssensor gefunden (oder aufgegeben). Neustart des Controllers versuchen.").into()),
    }
    strg_c_abfangen();
    dauer(g, true)?;
    let r = if nur_live { live(g) } else { gefuehrt(g) };
    let _ = dauer(g, false);
    r
}

fn zeile(p: &Probe) -> String {
    let a = p.a_g();
    let d = p.g_dps();
    format!(
        "a {:+5.2} {:+5.2} {:+5.2} g (|a| {:4.2})   {} {:+7.1} {:+7.1} {:+7.1} °/s",
        a[0], a[1], a[2], p.betrag_g(), t("Gyro"), d[0], d[1], d[2]
    )
}

fn live(g: &mut dyn Kanal) -> Result<(), String> {
    println!("{}", t("Achsen wie an der Switch: X zu den Schultertasten, Y nach links, Z nach oben. Strg+C beendet."));
    while !abgebrochen() {
        let p = probe_holen(g)?;
        print!("\r{}   ", zeile(&p));
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_millis(50));
    }
    println!();
    Ok(())
}

fn gefuehrt(g: &mut dyn Kanal) -> Result<(), String> {
    println!("{}", t("Gyro-Test: die Schritte in beliebiger Reihenfolge ausführen, erledigte werden abgehakt (höchstens 2 Minuten)."));
    let mut pr = Pruefung::default();
    let start = Instant::now();
    let mut gezeigt = Vec::new();
    while start.elapsed() < Duration::from_secs(120) && !pr.fertig() && !abgebrochen() {
        let p = probe_holen(g)?;
        pr.neu(&p, Instant::now());
        let stand = pr.schritte();
        if stand.iter().map(|s| s.1).collect::<Vec<_>>() != gezeigt {
            gezeigt = stand.iter().map(|s| s.1).collect();
            println!("\r{:100}", "");
            for (text, ok) in &stand {
                println!("  [{}] {}", if *ok { "x" } else { " " }, t(text));
            }
        }
        print!("\r{}   ", zeile(&p));
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_millis(30));
    }
    println!();
    let m = pr.ruhe_mittel;
    println!(
        "{}",
        tr(
            "Proben: {}, ungültig: {}. Nullpunkt in Ruhe: {} °/s. Größte Drehrate: {} °/s.",
            &[
                &pr.proben,
                &pr.ungueltig,
                &format!("{:+.1} {:+.1} {:+.1}", m[0], m[1], m[2]),
                &format!("{:.0} {:.0} {:.0}", pr.max_dps[0], pr.max_dps[1], pr.max_dps[2]),
            ]
        )
    );
    if pr.fertig() {
        println!("{}", t("Gyro in Ordnung: alle Lagen und Drehachsen erkannt."));
        Ok(())
    } else if abgebrochen() {
        Err(t("Abgebrochen").into())
    } else {
        let offen: Vec<_> = pr.schritte().into_iter().filter(|s| !s.1).map(|s| t(s.0).to_string()).collect();
        Err(tr("Zeit abgelaufen, nicht erkannt: {}", &[&offen.join(", ")]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bericht_lesen() {
        let p = Probe::lesen("ok: gyro bericht gueltig=1 alter=0 a=-742,57,4057 g=2,-2,-6 | a=-741,57,4056 g=2,-1,-5").unwrap();
        assert_eq!(p, Probe { gueltig: true, alter: 0, a: [-742, 57, 4057], g: [2, -2, -6] });
        assert!((p.betrag_g() - 1.0).abs() < 0.03);
        // Störzeichen davor, ungültig
        let p = Probe::lesen("\u{1}ok: gyro bericht gueltig=0 alter=255 a=0,0,0 g=0,0,0 | a=0,0,0 g=0,0,0").unwrap();
        assert!(!p.gueltig);
        assert!(Probe::lesen("ok: gyro bericht gueltig=1 alter=0 a=1,2 g=1,2,3").is_none());
        assert!(Probe::lesen("ok: gyro status init=1").is_none());
    }

    #[test]
    fn init() {
        assert_eq!(init_lesen("ok: gyro status init=1 (1 ok, 0 offen, -1 kein Sensor/aufgegeben) warte=5"), Some(1));
        assert_eq!(init_lesen("ok: gyro status init=-1 (1 ok)"), Some(-1));
        assert_eq!(init_lesen("ok: gyro dauer=1"), None);
    }

    #[test]
    fn lage_folgt() {
        let mut l = Lage::default();
        let flach = Probe { gueltig: true, alter: 0, a: [0, 0, 4096], g: [0, 0, 0] };
        l.neu(&flach, 0.03, [0.0; 3]);
        assert!((l.r[2][2] - 1.0).abs() < 1e-4 && (l.r[0][0] - 1.0).abs() < 1e-4);
        // 1 s lang 90 °/s um Z (Hochachse): Schultertasten (Gerät +X) zeigen danach nach links (Welt +Y)
        let dps = (90.0 / DPS_JE_SCHRITT).round() as i16;
        for _ in 0..100 {
            l.neu(&Probe { g: [0, 0, dps], ..flach }, 0.01, [0.0; 3]);
        }
        let x = l.welt([1.0, 0.0, 0.0]);
        assert!(x[1] > 0.99, "{x:?}");
        // Kippen nach vorn (Y positiv) ohne Schwerkraft-Bezug: Schultertasten gehen nach unten
        let mut l = Lage::default();
        l.neu(&flach, 0.03, [0.0; 3]);
        for _ in 0..50 {
            l.neu(&Probe { a: [0, 0, 0], g: [0, dps, 0], ..flach }, 0.01, [0.0; 3]);
        }
        assert!(l.welt([1.0, 0.0, 0.0])[2] < -0.6);
        // Schwerkraft zieht eine falsche Neigung zurück
        let mut l = Lage::default();
        l.ausrichten(&Probe { a: [-4096, 0, 0], ..flach }); // steht auf der Oberkante
        for _ in 0..400 {
            l.neu(&flach, 0.01, [0.0; 3]);
        }
        assert!(l.welt([0.0, 0.0, 1.0])[2] > 0.95);
        // schräg im Dock (am Gerät 5.11.: a ≈ 0,77/0,02/0,65): Lage bleibt über 10 s stehen, „oben“ = a, Y links
        let dock = Probe { a: [3143, 87, 2659], ..flach };
        let mut l = Lage::default();
        l.neu(&dock, 0.03, [0.0; 3]);
        for _ in 0..1000 {
            l.neu(&dock, 0.01, [0.0; 3]);
        }
        let a = dock.a_g().map(|v| v / dock.betrag_g());
        let oben = l.welt(a);
        assert!(oben[2] > 0.999, "{oben:?}");
        assert!(l.welt([0.0, 1.0, 0.0])[1] > 0.99);
        assert!(l.welt([1.0, 0.0, 0.0])[2] > 0.7); // Schultertasten oben
        // falsch ausgerichtet (flach angenommen, liegt aber schräg): Schwerkraft zieht hin
        let mut l = Lage::default();
        l.ausrichten(&flach);
        for _ in 0..500 {
            l.neu(&dock, 0.01, [0.0; 3]);
        }
        assert!(l.welt(a)[2] > 0.98, "{:?}", l.welt(a));
        // auf der Oberkante: Gerät +X zeigt nach unten
        let mut l = Lage::default();
        l.ausrichten(&Probe { a: [-4096, 0, 0], ..flach });
        assert!(l.welt([1.0, 0.0, 0.0])[2] < -0.99);
    }

    #[test]
    fn pruefung_ablauf() {
        let mut pr = Pruefung::default();
        let t0 = Instant::now();
        let flach = Probe { gueltig: true, alter: 0, a: [-742, 57, 4050], g: [3, -1, -5] };
        for i in 0..40 {
            pr.neu(&flach, t0 + Duration::from_millis(30 * i));
        }
        assert!(pr.ruhe && pr.flach && !pr.hochkant);
        assert!(pr.ruhe_mittel[2] < 0.0);
        // Schwenk durch die Lage zählt nicht als hochkant
        pr.neu(&Probe { gueltig: true, alter: 0, a: [-4050, 0, 0], g: [0, 1500, 0] }, t0 + Duration::from_secs(2));
        assert!(!pr.hochkant && pr.dreh[1]);
        pr.neu(&Probe { gueltig: true, alter: 0, a: [-4050, -150, -400], g: [0, 0, 0] }, t0 + Duration::from_secs(3));
        assert!(pr.hochkant);
        pr.neu(&Probe { gueltig: true, alter: 0, a: [0, 0, 4096], g: [-2000, 0, 2000] }, t0 + Duration::from_secs(4));
        assert!(pr.fertig());
        // ungültige Probe zählt nur mit
        pr.neu(&Probe::default(), t0 + Duration::from_secs(5));
        assert_eq!(pr.ungueltig, 1);
    }
}

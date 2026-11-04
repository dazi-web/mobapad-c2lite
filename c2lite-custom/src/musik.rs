//! Musik über die HD-Rumble-Linearmotoren des Chitu 2 HD (Firmware ab chitu-CRb, Befehl „chitu ton“).
//!
//! Nur am Chitu: vorher „info“ lesen, `geraet=chitu` muss dabei sein (der C2 Lite hat Unwuchtmotoren und kennt
//! „chitu ton“ nicht). Alle Noten gehen in EINER Verbindung in die Warteschlange der Firmware (32 Plätze); ist sie voll,
//! wird kurz gewartet. Ein Befehl je Note über einzelne Aufrufe von „roh“ ist zu langsam für kurze Noten.
//!
//! Notenformat (Datei oder direkt als Argument): Tokens durch Leerzeichen, Komma oder Zeilenumbruch getrennt,
//! „#“ bis Zeilenende ist Kommentar. Token = <Ton>[:<ms>]
//!   Ton:  Notenname A..G (H = B), danach optional # oder b, dann Oktave (A4 = 440 Hz), z. B. C#4, Eb3, H3
//!         oder eine Frequenz in Hz (z. B. 250), oder P bzw. - für eine Pause
//!   ms:   Dauer; ohne Angabe gilt die Dauer der vorigen Note (anfangs 400)

use crate::geraet::Kanal;
use crate::sprache::{t, tr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Grenzen der Firmware (chitu_dac.c, Stand CRb)
const HZ_MIN: u32 = 40;
const HZ_MAX: u32 = 1000;
const TON_MAX_MS: u32 = 3000;
const PEGEL_MAX: u32 = 50;
/// Lastkonto der Firmware: so viele Sekunden Ton am Stück, danach schweigt sie, bis genauso lange Pause war
const LAST_MAX_S: f64 = 30.0; // ab Firmware chitu-CRd (vorher 15 s)
pub const ABSTAND_MAX_MS: u32 = 60;
/// Resonanz der Motoren am Chitu (Durchlauf 3.11.2026, Nutzer): am lautesten um 250 Hz
const RESONANZ_HZ: f64 = 250.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// 0 = Pause
    pub hz: f64,
    pub ms: u32,
}

/// Frequenz eines Notennamens (gleichstufig, A4 = 440 Hz), None bei unbekanntem Namen
fn notenname_hz(s: &str) -> Option<f64> {
    let mut z = s.chars();
    let basis = match z.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' | 'H' => 11,
        _ => return None,
    };
    let rest: String = z.collect();
    let (vorzeichen, oktave) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest.as_str())
    };
    let oktave: i32 = oktave.parse().ok()?;
    if !(0..=8).contains(&oktave) {
        return None;
    }
    let halbton = (oktave + 1) * 12 + basis + vorzeichen; // MIDI-Nummer, A4 = 69
    Some(440.0 * 2f64.powf((halbton - 69) as f64 / 12.0))
}

/// Noten aus Text lesen
pub fn lesen(text: &str) -> Result<Vec<Note>, String> {
    let mut noten = Vec::new();
    let mut ms_vorher = 400u32;
    for (nr, zeile) in text.lines().enumerate() {
        // „#“ ist Kommentar nur am Zeilenanfang oder nach Leerraum (sonst Kreuz wie in C#4)
        let ende = zeile
            .char_indices()
            .find(|&(i, c)| c == '#' && zeile[..i].chars().next_back().is_none_or(char::is_whitespace))
            .map_or(zeile.len(), |(i, _)| i);
        let zeile = &zeile[..ende];
        for token in zeile.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()) {
            let (ton, ms) = match token.split_once(':') {
                Some((ton, ms)) => {
                    let ms: u32 = ms
                        .parse()
                        .map_err(|_| tr("Zeile {}: Dauer „{}“ ist keine Zahl (ms)", &[&(nr + 1), &ms]))?;
                    (ton, ms)
                }
                None => (token, ms_vorher),
            };
            if ms == 0 || ms > 60_000 {
                return Err(tr("Zeile {}: Dauer {} ms außerhalb 1..60000", &[&(nr + 1), &ms]));
            }
            ms_vorher = ms;
            let hz = if ton.eq_ignore_ascii_case("p") || ton == "-" {
                0.0
            } else if let Ok(hz) = ton.parse::<f64>() {
                hz
            } else {
                notenname_hz(ton).ok_or_else(|| tr("Zeile {}: unbekannte Note „{}“", &[&(nr + 1), &ton]))?
            };
            noten.push(Note { hz, ms });
        }
    }
    if noten.is_empty() {
        return Err(t("keine Noten gefunden").into());
    }
    Ok(noten)
}

/// Um Halbtöne verschieben (Pausen bleiben)
pub fn transponieren(noten: &mut [Note], halbtoene: i32) {
    let f = 2f64.powf(halbtoene as f64 / 12.0);
    for n in noten.iter_mut().filter(|n| n.hz > 0.0) {
        n.hz *= f;
    }
}

/// Halbtöne, die die Melodie um die Resonanz der Motoren legen (geometrische Mitte von tiefstem und höchstem Ton)
pub fn auto_lage(noten: &[Note]) -> i32 {
    let toene: Vec<f64> = noten.iter().filter(|n| n.hz > 0.0).map(|n| n.hz).collect();
    if toene.is_empty() {
        return 0;
    }
    let tief = toene.iter().cloned().fold(f64::MAX, f64::min);
    let hoch = toene.iter().cloned().fold(0.0, f64::max);
    let mitte = (tief * hoch).sqrt();
    (12.0 * (RESONANZ_HZ / mitte).log2()).round() as i32
}

/// Liegt alles im Bereich der Firmware? Fehler nennt die erste Note außerhalb
pub fn pruefen(noten: &[Note]) -> Result<(), String> {
    for (i, n) in noten.iter().enumerate() {
        if n.hz > 0.0 && (n.hz.round() < HZ_MIN as f64 || n.hz.round() > HZ_MAX as f64) {
            return Err(tr(
                "Note {} hat {} Hz, die Firmware spielt {}..{} Hz (--transponieren oder --lage auto)",
                &[&(i + 1), &format!("{:.0}", n.hz), &HZ_MIN, &HZ_MAX],
            ));
        }
    }
    Ok(())
}

/// Lücke nach jedem Ton einfügen (Gesamtdauer bleibt gleich)
pub fn mit_abstand(noten: &[Note], abstand_ms: u32) -> Vec<Note> {
    let mut v = Vec::with_capacity(noten.len() * 2);
    for n in noten {
        let luecke = if n.hz > 0.0 { abstand_ms.min(n.ms.saturating_sub(20)) } else { 0 };
        v.push(Note { hz: n.hz, ms: n.ms - luecke });
        if luecke > 0 {
            v.push(Note { hz: 0.0, ms: luecke });
        }
    }
    v
}

/// Befehle für die Firmware: lange Noten in Stücke zu höchstens TON_MAX_MS
pub fn befehle(noten: &[Note], pegel: u32, kanal: &str) -> Vec<String> {
    let mut b = Vec::new();
    for n in noten {
        let mut rest = n.ms;
        while rest > 0 {
            let ms = rest.min(TON_MAX_MS);
            rest -= ms;
            if n.hz > 0.0 {
                b.push(format!("chitu ton {} {} {} {}", n.hz.round() as u32, ms, pegel, kanal));
            } else {
                b.push(format!("chitu ton 0 {ms} 0"));
            }
        }
    }
    b
}

/// Meldet diese „info:“-Zeile ein Chitu?
pub fn ist_chitu(info: &str) -> bool {
    info.split_whitespace().any(|w| w == "geraet=chitu")
}

/// Ist am anderen Ende ein Chitu mit Ton-Firmware?
fn chitu_pruefen(g: &mut dyn Kanal) -> Result<(), String> {
    let info = g.befehl("info", &["info:"], Duration::from_secs(2))?;
    if !ist_chitu(&info) {
        return Err(t("Musik geht nur am Chitu 2 HD (HD-Rumble-Linearmotoren). Dieser Controller meldet sich nicht als Chitu \
                      (der C2 Lite hat Unwuchtmotoren ohne Tonausgabe).")
            .into());
    }
    Ok(())
}

/// Ein Stück, fertig für die Firmware
pub struct Stueck {
    pub befehle: Vec<String>,
    pub noten: usize,
    pub gesamt_s: f64,
    pub ton_s: f64,
    pub halbtoene: i32,
}

/// Noten lesen, verschieben, prüfen und in Firmware-Befehle umsetzen (Kommandozeile und Oberfläche)
/// `abstand_ms`: Lücke nach jedem Ton (der Ton wird um so viel kürzer, bleibt aber mindestens 20 ms), damit gleiche
/// Töne hintereinander getrennt klingen.
pub fn vorbereiten(text: &str, pegel: u32, halbtoene: i32, auto: bool, kanal: &str, abstand_ms: u32) -> Result<Stueck, String> {
    if !(1..=PEGEL_MAX).contains(&pegel) {
        return Err(tr("Pegel {} außerhalb 1..{} %", &[&pegel, &PEGEL_MAX]));
    }
    if !(-36..=36).contains(&halbtoene) {
        return Err(t("--transponieren höchstens ±36 Halbtöne").into());
    }
    if !["l", "r", "lr"].contains(&kanal) {
        return Err(hilfe());
    }
    if abstand_ms > ABSTAND_MAX_MS {
        return Err(tr("Abstand höchstens {} ms", &[&ABSTAND_MAX_MS]));
    }
    let mut noten = lesen(text)?;
    let halbtoene = if auto { halbtoene + auto_lage(&noten) } else { halbtoene };
    transponieren(&mut noten, halbtoene);
    pruefen(&noten)?;
    let noten = mit_abstand(&noten, abstand_ms);
    Ok(Stueck {
        befehle: befehle(&noten, pegel, kanal),
        noten: noten.iter().filter(|n| n.hz > 0.0).count(),
        gesamt_s: noten.iter().map(|n| n.ms as f64 / 1000.0).sum(),
        ton_s: noten.iter().filter(|n| n.hz > 0.0).map(|n| n.ms as f64 / 1000.0).sum(),
        halbtoene,
    })
}

/// Schickt die Befehle in die Warteschlange der Firmware (wartet, solange sie voll ist). Kehrt zurück, sobald alles
/// angenommen ist (die Musik läuft dann noch). Ist `abbruch` gesetzt, wird angehalten und „chitu ton stop“ gesendet.
pub fn abspielen(g: &mut dyn Kanal, befehle: &[String], abbruch: &AtomicBool) -> Result<(), String> {
    chitu_pruefen(g)?;
    for b in befehle {
        let ende = Instant::now() + Duration::from_secs(TON_MAX_MS as u64 / 1000 * 33 + 5);
        loop {
            if abbruch.load(Ordering::Relaxed) {
                g.befehl("chitu ton stop", &["ok:", "fehler:"], Duration::from_secs(2))?;
                return Ok(());
            }
            let z = g.befehl(b, &["ok:", "fehler:"], Duration::from_secs(2))?;
            if z.starts_with("ok:") {
                break;
            }
            if z.contains("warteschlange voll") && Instant::now() < ende {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            return Err(z);
        }
    }
    Ok(())
}

/// Anzeigename: erste Kommentarzeile bis zum ersten „. “, sonst der Dateiname
pub fn titel(text: &str, dateiname: &str) -> String {
    text.lines()
        .find_map(|z| z.trim().strip_prefix('#'))
        .map(|z| z.trim())
        .map(|z| z.split(". ").next().unwrap_or(z).trim_end_matches('.').to_string())
        .filter(|z| !z.is_empty())
        .unwrap_or_else(|| dateiname.to_string())
}

fn hilfe() -> String {
    t("musik <datei|noten> [--pegel 1..50] [--transponieren <halbtöne>] [--lage auto] [--abstand 0..60] [--kanal l|r|lr] | musik stop").into()
}

/// „musik …“ auf der Kommandozeile
pub fn befehl(g: &mut dyn Kanal, a: &[String]) -> Result<(), String> {
    if a.first().map(String::as_str) == Some("stop") {
        chitu_pruefen(g)?;
        let z = g.befehl("chitu ton stop", &["ok:", "fehler:"], Duration::from_secs(2))?;
        return if z.starts_with("ok:") { Ok(()) } else { Err(z) };
    }
    let mut pegel = 40u32; // Hörtest am Chitu 3.11.: 40 % und 30 ms Abstand am besten
    let mut halbtoene = 0i32;
    let mut auto = false;
    let mut kanal = "lr".to_string();
    let mut abstand = 30u32;
    let mut quelle: Option<String> = None;
    let mut i = 0;
    while i < a.len() {
        let wert = |i: usize| a.get(i + 1).cloned().ok_or_else(hilfe);
        match a[i].as_str() {
            "--pegel" => {
                pegel = wert(i)?.parse().map_err(|_| hilfe())?;
                i += 1;
            }
            "--transponieren" => {
                halbtoene = wert(i)?.parse().map_err(|_| hilfe())?;
                i += 1;
            }
            "--lage" => {
                if wert(i)? != "auto" {
                    return Err(hilfe());
                }
                auto = true;
                i += 1;
            }
            "--abstand" => {
                abstand = wert(i)?.parse().map_err(|_| hilfe())?;
                i += 1;
            }
            "--kanal" => {
                kanal = wert(i)?;
                i += 1;
            }
            s if s.starts_with("--") => return Err(hilfe()),
            s => {
                quelle = Some(match &quelle {
                    Some(q) => format!("{q} {s}"),
                    None => s.to_string(),
                })
            }
        }
        i += 1;
    }
    let quelle = quelle.ok_or_else(hilfe)?;
    let text = match std::fs::read_to_string(&quelle) {
        Ok(text) => text,
        Err(_) if !std::path::Path::new(&quelle).exists() => quelle.clone(), // direkt als Noten
        Err(e) => return Err(format!("{quelle}: {e}")),
    };
    let s = vorbereiten(&text, pegel, halbtoene, auto, &kanal, abstand)?;
    if s.ton_s > LAST_MAX_S {
        println!(
            "{}",
            tr("Hinweis: {} s Ton, die Firmware schweigt nach {} s Ton am Stück, bis gleich lange Pause war.", &[
                &format!("{:.1}", s.ton_s),
                &LAST_MAX_S
            ])
        );
    }
    let start = Instant::now();
    // erst nach der Chitu-Prüfung ansagen (abspielen prüft zuerst)
    let ansage = tr("Spiele {} Noten, {} s, Pegel {} %, transponiert {} Halbtöne", &[
        &s.noten,
        &format!("{:.1}", s.gesamt_s),
        &pegel,
        &s.halbtoene,
    ]);
    abspielen(g, &s.befehle, &AtomicBool::new(false)).inspect(|_| println!("{ansage}"))?;
    // warten, bis die Melodie durch ist (Programmende = Musikende)
    let rest = Duration::from_secs_f64(s.gesamt_s).saturating_sub(start.elapsed());
    std::thread::sleep(rest + Duration::from_millis(200));
    println!("{}", t("fertig"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notennamen() {
        assert!((notenname_hz("A4").unwrap() - 440.0).abs() < 0.01);
        assert!((notenname_hz("A3").unwrap() - 220.0).abs() < 0.01);
        assert!((notenname_hz("C4").unwrap() - 261.63).abs() < 0.01);
        assert!((notenname_hz("H3").unwrap() - notenname_hz("B3").unwrap()).abs() < 0.001);
        assert!((notenname_hz("C#4").unwrap() - notenname_hz("Db4").unwrap()).abs() < 0.001);
        assert!(notenname_hz("X4").is_none());
        assert!(notenname_hz("A").is_none());
    }

    #[test]
    fn lesen_und_dauer() {
        let n = lesen("A3:400 D3 # Kommentar\n250:100, P, -:50").unwrap();
        assert_eq!(n.len(), 5);
        assert_eq!(n[1].ms, 400); // übernimmt die vorige Dauer
        assert_eq!(n[2], Note { hz: 250.0, ms: 100 });
        assert_eq!(n[3], Note { hz: 0.0, ms: 100 });
        assert_eq!(n[4].ms, 50);
        // Kreuz ist kein Kommentar, „#“ nach Leerraum schon
        let k = lesen("# Kopf\nF#3:450 C#4 #Rest F4\nDb4:100 # Ende").unwrap();
        assert_eq!(k.len(), 3);
        assert!((k[0].hz - notenname_hz("F#3").unwrap()).abs() < 0.001);
        assert!((k[1].hz - notenname_hz("C#4").unwrap()).abs() < 0.001);
        assert_eq!(k[1].ms, 450);
        assert!(lesen("Q3:100").is_err());
        assert!(lesen("# nur Kommentar").is_err());
    }

    #[test]
    fn transponieren_auto_pruefen() {
        let mut n = lesen("A3:100 A4:100 P:100").unwrap();
        transponieren(&mut n, 12);
        assert!((n[0].hz - 440.0).abs() < 0.01);
        assert_eq!(n[2].hz, 0.0);
        // tief 131 .. hoch 262 Hz, Mitte ~185 Hz -> ~+5 Halbtöne Richtung 250 Hz
        let m = lesen("C3 C4").unwrap();
        assert_eq!(auto_lage(&m), 5);
        assert!(pruefen(&lesen("30:100").unwrap()).is_err());
        assert!(pruefen(&lesen("1200:100").unwrap()).is_err());
        assert!(pruefen(&lesen("40:100 1000:100 P:5000").unwrap()).is_ok());
    }

    /// Nachgebauter Kanal: antwortet auf „info“ mit der gegebenen Zeile, merkt sich alle Befehle
    struct Attrappe {
        info: &'static str,
        gesendet: Vec<String>,
    }
    impl Kanal for Attrappe {
        fn befehl(&mut self, befehl: &str, _p: &[&str], _w: Duration) -> Result<String, String> {
            self.gesendet.push(befehl.to_string());
            Ok(if befehl == "info" { self.info.to_string() } else { "ok: ton".to_string() })
        }
        fn name(&self) -> String {
            "attrappe".into()
        }
    }

    #[test]
    fn nur_am_chitu() {
        let mut c2 = Attrappe { info: "info: firmware=c2lite-custom stand=16o akku=4636mV/100%", gesendet: vec![] };
        let a: Vec<String> = ["250:10"].iter().map(|s| s.to_string()).collect();
        assert!(befehl(&mut c2, &a).is_err());
        assert_eq!(c2.gesendet, vec!["info"]); // kein „chitu ton“ an den C2 Lite
        assert!(befehl(&mut c2, &["stop".to_string()]).is_err());
        assert_eq!(c2.gesendet, vec!["info", "info"]);

        let mut chitu = Attrappe { info: "info: firmware=c2lite-custom stand=chitu-CRb geraet=chitu", gesendet: vec![] };
        assert!(befehl(&mut chitu, &a).is_ok());
        assert_eq!(chitu.gesendet, vec!["info", "chitu ton 250 10 40 lr"]); // Vorgabe 40 %, 10-ms-Ton bekommt keine Lücke
    }

    #[test]
    fn abstand_trennt_gleiche_toene() {
        let n = mit_abstand(&lesen("E4:200 E4:30 P:100").unwrap(), 25);
        assert_eq!(n.len(), 5);
        assert_eq!((n[0].ms, n[1].hz, n[1].ms), (175, 0.0, 25)); // Ton kürzer, Lücke dahinter
        assert_eq!((n[2].ms, n[3].ms), (20, 10)); // mindestens 20 ms Ton
        assert_eq!(n[4], Note { hz: 0.0, ms: 100 }); // Pausen bleiben
        let s = vorbereiten("E4:200 E4:200", 20, 0, false, "lr", 25).unwrap();
        assert_eq!(s.noten, 2);
        assert!((s.gesamt_s - 0.4).abs() < 1e-9);
        assert!(vorbereiten("E4:200", 20, 0, false, "lr", 61).is_err());
    }

    #[test]
    fn titel_und_kennung() {
        assert_eq!(titel("# Axel F – Crazy Frog (2005). Nur lokal.
F3:100", "axel-f"), "Axel F – Crazy Frog (2005)");
        assert_eq!(titel("F3:100", "axel-f"), "axel-f");
        assert!(ist_chitu("info: stand=chitu-CRb akku=1mV/1% geraet=chitu"));
        assert!(!ist_chitu("info: stand=16o geraet=c2lite"));
        assert!(!ist_chitu("info: stand=chitu-x")); // Stand allein reicht nicht
        assert!(!ist_chitu("info: stand=chitu-CRc-probe geraet=chitu-probe")); // Generalprobe auf dem C2 Lite
    }

    #[test]
    fn lange_noten_werden_geteilt() {
        let b = befehle(&lesen("250:7000 P:3500").unwrap(), 20, "lr");
        assert_eq!(b, vec![
            "chitu ton 250 3000 20 lr",
            "chitu ton 250 3000 20 lr",
            "chitu ton 250 1000 20 lr",
            "chitu ton 0 3000 0",
            "chitu ton 0 500 0",
        ]);
    }
}

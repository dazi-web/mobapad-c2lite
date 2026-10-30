//! c2lite-custom – Werkzeug für den Mobapad C2 Lite mit eigener Firmware: testen, LEDs, Vibration, Einstellungen.

mod ble;
#[cfg(target_os = "linux")]
mod abbild;
#[cfg(target_os = "linux")]
mod download;
#[cfg(target_os = "linux")]
mod installieren;
#[cfg(target_os = "linux")]
mod werk_ble;
mod geraet;
mod gui;
mod test;
mod wecken;

use geraet::{Geraet, Kanal};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const HILFE: &str = "\
c2lite-custom – Werkzeug für den Mobapad C2 Lite mit eigener Firmware

Aufruf: c2lite-custom                (ohne Befehl: Oberfläche)
        c2lite-custom [--port <pfad> | --bluetooth [adresse]] <befehl> [argumente]

Ohne Option über USB-Kabel; mit --bluetooth drahtlos (gleiche Befehle, auch update).

Befehle:
  test                         Live-Ansicht: Tasten, Sticks, Akku (q beendet, l LEDs, v Vibration)
  info                         Firmware, Laufzeit, Reset-Quelle, Akku
  led <alle|aus|1,3|0x3c>      Spieler-LEDs fest setzen (Spieler 1..4 oder Rohmaske 0x00..0x7f)
  led auto                     LEDs zeigen wieder den Zustand an (verbunden, sucht, lädt, ...)
  vibration <links%> <rechts%> [ms]   Motoren kurz laufen lassen (höchstens 5000 ms)
  licht                        Lichteinstellungen anzeigen
  licht <modus>                regenbogen | farbwechsel | atmen | statisch | reaktiv | aus
  licht farbe <RRGGBB|name>    Farbe für statisch/atmen (z. B. ff8800, rot, blau, lila)
  licht helligkeit <0..100>    Streifen und Seiten
  licht logo <0..100>          Helligkeit des Logos (Statusanzeige)
  licht tempo <1..100>         Geschwindigkeit der Effekte
  licht standard               Regenbogen, 15 %, Logo 25 %, Tempo 30
  einstellungen                gespeicherte Einstellungen anzeigen
  setze <anzeige|vibration|totzone> <wert>   Einstellung ändern und speichern (anzeige 0/1 = Spieler-LEDs)
  belegung                     Rücktasten, M-Taste und Turbo anzeigen
  belegung <rueck1|rueck2|m> aus | taste <a+b> | makro <taste:halten:pause> …
                               | loop <einmal|endlos|halten> | intervall <ms>
                               Tasten: a b x y l r zl zr minus plus home l3 r3 kamera hoch rechts runter links,
                               Stick-Richtungen lhoch lrechts lrunter llinks rhoch rrechts rrunter rlinks
  belegung standard            Werkszustand: Rück1 = B, Rück2 = A, M = Makro wie Mobapad
  turbo                        Turbo anzeigen
  turbo <taste|rueck1|rueck2|m> <aus|halten|auto>   Turbo einer Taste
  turbo tempo <0|1|2>          5 / 8 / 12 Hz
  wecken                       Weckkennung für die Switch 2 anzeigen (Home am Controller weckt die Konsole)
  wecken einrichten            Wecksignal eines Joy-Con 2 auffangen und im Controller speichern:
                               Switch 2 in den Standby, Joy-Con abnehmen, eine Taste daran drücken
  wecken lernen                dasselbe ohne Laptop: der Controller lauscht 30 s selbst (am Gerät: Licht + Home)
  wecken test                  Wecksignal jetzt senden (Switch 2 im Standby, Controller nicht verbunden)
  wecken zuruecksetzen         Kennung löschen (eigene Adresse, weckt die Switch 2 nicht)
  installieren <jl_isd.bin> [--probelauf]
                               Erstinstallation auf einer C2 Lite mit Werks-Firmware (Linux, ohne Zusatzhardware):
                               Bluetooth koppelt, Brücke als Update, dann USB-Kabel -> Sicherung + C2 Lite Custom. Fragt einmal
                               nach dem Passwort. --probelauf: nur prüfen und koppeln, nichts verändern
  werk-zurueck [werk-sicherung.bin]
                               zurück zur Werks-Firmware aus der Sicherung von „installieren“ (Vorgabe: die neueste
                               in ~/c2lite-sicherung). Controller am USB; sichert vorher den aktuellen Stand
  update <db_update_data.bin>  Firmware-Update über USB in die freie Bank (Dual-Bank-Firmware)
  neustart                     Controller neu starten
  roh <befehl …>               Firmware-Befehl direkt senden (Fehlersuche, z. B. „roh akku diag“)
  download-modus               in den ROM-Download-Modus springen (zum Flashen)
  download lesen <datei> [adresse] [länge]
                               im ROM-Download-Modus Flash lesen (Vorgabe: ganzer Flash, 1 MB); braucht root oder
                               die udev-Regel werkzeuge/99-jieli-br23.rules
  download schreiben <adresse> <datei>
                               im ROM-Download-Modus Flash schreiben (Adresse hex, auf 4 KB ausgerichtet)
  download neustart            aus dem ROM-Download-Modus neu starten
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let port = match args.iter().position(|a| a == "--port") {
        Some(i) if i + 1 < args.len() => {
            let p = args.remove(i + 1);
            args.remove(i);
            Some(p)
        }
        _ => None,
    };
    let bluetooth = match args.iter().position(|a| a == "--bluetooth" || a == "--ble") {
        Some(i) => {
            args.remove(i);
            // optionale Adresse direkt dahinter (Form AA:BB:CC:DD:EE:FF)
            Some(if i < args.len() && args[i].len() == 17 && args[i].matches(':').count() == 5 {
                Some(args.remove(i))
            } else {
                None
            })
        }
        None => None,
    };
    if args.is_empty() && port.is_none() && bluetooth.is_none() {
        return match gui::starten() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Oberfläche nicht zu starten: {e}");
                ExitCode::FAILURE
            }
        };
    }
    #[cfg(target_os = "linux")]
    match args.first().map(String::as_str) {
        Some("installieren-kern") => {
            return if installieren::kern(&args[1..]).is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
        }
        Some("installieren") => return installieren_befehl(&args[1..]),
        Some("werk-zurueck-kern") => {
            return if installieren::werk_zurueck_kern(&args[1..]).is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
        }
        Some("werk-zurueck") => return werk_zurueck_befehl(&args[1..]),
        _ => {}
    }
    #[cfg(target_os = "linux")]
    if args.first().map(String::as_str) == Some("download") {
        return match download_befehl(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Fehler: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.is_empty() || args[0] == "hilfe" || args[0] == "--help" || args[0] == "-h" {
        print!("{HILFE}");
        return ExitCode::SUCCESS;
    }
    let mut g: Box<dyn Kanal> = if let Some(adresse) = bluetooth {
        match ble::BleGeraet::verbinden(adresse.as_deref()) {
            Ok(g) => Box::new(g),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let Some(pfad) = port.or_else(geraet::finden) else {
            eprintln!("Kein Controller mit eigener Firmware gefunden (USB-Kabel gesteckt? Sonst --bluetooth).");
            return ExitCode::FAILURE;
        };
        match Geraet::oeffnen(&pfad) {
            Ok(g) => Box::new(g),
            Err(e) => {
                eprintln!("Port nicht zu öffnen: {e}");
                #[cfg(target_os = "linux")]
                eprintln!("Tipp: Benutzer in die Gruppe dialout aufnehmen (sudo usermod -aG dialout $USER, neu anmelden).");
                return ExitCode::FAILURE;
            }
        }
    };
    match ausfuehren(g.as_mut(), &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}

const ANTWORT: &[&str] = &["ok:", "fehler:"];
const WARTEN: Duration = Duration::from_secs(2);

fn ausfuehren(g: &mut dyn Kanal, args: &[String]) -> Result<(), String> {
    let rest = |i: usize| args.get(i).map(String::as_str);
    match args[0].as_str() {
        "test" => test::starten(g),
        "info" => zeigen(g.befehl("info", &["info:"], WARTEN)?),
        "einstellungen" => zeigen(g.befehl("einstellungen", &["einstellungen:"], WARTEN)?),
        "led" => {
            let wert = rest(1).ok_or("led braucht ein Ziel, z. B. „c2lite-custom led alle“ oder „led auto“")?;
            if wert == "auto" {
                return antwort(g.befehl("led auto", ANTWORT, WARTEN)?);
            }
            let maske = led_maske(wert)?;
            antwort(g.befehl(&format!("led {maske}"), ANTWORT, WARTEN)?)
        }
        "vibration" => {
            let zahl = |i: usize, vorgabe: Option<u32>| -> Result<u32, String> {
                match rest(i) {
                    Some(t) => t.trim_end_matches('%').parse().map_err(|_| format!("„{t}“ ist keine Zahl")),
                    None => vorgabe.ok_or("vibration braucht <links%> <rechts%> [ms]".to_string()),
                }
            };
            let (l, r, ms) = (zahl(1, None)?, zahl(2, None)?, zahl(3, Some(1000))?);
            antwort(g.befehl(&format!("motor {} {} {}", l.min(100), r.min(100), ms.min(5000)), ANTWORT, WARTEN)?)
        }
        "licht" => licht(g, &args[1..]),
        "setze" => {
            let (name, wert) = (rest(1).ok_or("setze <name> <wert>")?, rest(2).ok_or("setze <name> <wert>")?);
            antwort(g.befehl(&format!("setze {name} {wert}"), ANTWORT, WARTEN)?)
        }
        "update" => update(g, rest(1).ok_or("update braucht die Datei db_update_data.bin")?),
        "neustart" => antwort(g.befehl("neustart", ANTWORT, WARTEN)?),
        "belegung" | "turbo" => {
            let befehl = args.join(" ");
            let zeilen = g.befehl_alle(&befehl, &["belegung:", "turbo:", "ok:", "fehler:"], WARTEN)?;
            let mut fehler = false;
            for z in zeilen {
                if z.starts_with("ok:") {
                    continue;
                }
                fehler |= z.starts_with("fehler:");
                println!("{}", z.trim_start_matches("belegung: ").trim_start_matches("turbo: "));
            }
            if fehler {
                return Err("Firmware meldet einen Fehler".into());
            }
            Ok(())
        }
        "wecken" => wecken_befehl(g, rest(1)),
        "roh" => {
            let befehl = args[1..].join(" ");
            let alle = ["ok:", "fehler:", "info:", "status:", "einstellungen:", "akku:", "rgb:", "motor:", "switch:", "log:", "belegung:", "turbo:", "Befehle:"];
            for z in g.befehl_alle(&befehl, &alle, WARTEN)? {
                println!("{z}");
            }
            Ok(())
        }
        "download-modus" => {
            antwort(g.befehl("download-modus", ANTWORT, WARTEN)?)?;
            println!("Der Controller meldet sich gleich als „BR23 UBOOT1.00“ (USB 4c4a:2342).");
            Ok(())
        }
        andere => Err(format!("unbekannter Befehl „{andere}“ (c2lite-custom hilfe)")),
    }
}

fn wecken_befehl(g: &mut dyn Kanal, was: Option<&str>) -> Result<(), String> {
    match was {
        None => {
            let zeilen = g.befehl_alle("switch diag", &["switch:"], WARTEN)?;
            match zeilen.iter().find_map(|z| wecken::Kennung::aus_diag(z)) {
                Some(Some(k)) => println!("Wecken eingerichtet: {} {} (Produkt-ID {:04x})", k.name(), k.adresse, k.pid),
                Some(None) => println!("Wecken nicht eingerichtet (c2lite-custom wecken einrichten)"),
                None => return Err("Firmware kennt das Wecken noch nicht (Stand 9d oder neuer nötig)".into()),
            }
            Ok(())
        }
        Some("einrichten") => {
            println!("Switch 2 in den Standby schalten, einen Joy-Con 2 abnehmen und eine Taste daran drücken.");
            println!("Die Switch wacht dabei auf, das ist normal. Ich lausche 90 s …");
            let k = wecken::auffangen(Duration::from_secs(90), &std::sync::atomic::AtomicBool::new(false))?;
            println!("Aufgefangen: {} {} für die Konsole {}", k.name(), k.adresse, k.konsole);
            let a = g.befehl(&k.befehl(), ANTWORT, WARTEN)?;
            antwort_still(a)?;
            println!("Im Controller gespeichert. Ab jetzt weckt Home am C2 Lite die Switch 2.");
            Ok(())
        }
        Some("lernen") => {
            antwort(g.befehl("switch weck lernen", ANTWORT, WARTEN)?)?;
            println!("Der Controller lauscht 30 s (Logo orange). Joy-Con 2 abnehmen und eine Taste daran drücken;");
            println!("gefunden: Logo 3× grün und kurze Vibration, sonst 2× rot. Prüfen mit „c2lite-custom wecken“.");
            Ok(())
        }
        Some("test") => antwort(g.befehl("switch weck", ANTWORT, WARTEN)?),
        Some("zuruecksetzen") => antwort(g.befehl("switch weck setzen 2069 eigen", ANTWORT, WARTEN)?),
        Some(a) => Err(format!("wecken: unbekannt „{a}“ (einrichten, lernen, test, zuruecksetzen)")),
    }
}

/// Erstinstallation auf der Kommandozeile: „installieren <jl_isd.bin> [--probelauf]“
#[cfg(target_os = "linux")]
fn installieren_befehl(a: &[String]) -> ExitCode {
    let probelauf = a.iter().any(|x| x == "--probelauf");
    let Some(firmware) = a.iter().find(|x| !x.starts_with("--")) else {
        eprintln!("Aufruf: c2lite-custom installieren <jl_isd.bin> [--probelauf]");
        return ExitCode::FAILURE;
    };
    if probelauf {
        println!("PROBELAUF: Es wird nichts verändert, nur geprüft, ob alles bereit ist.");
    }
    let mut ausgabe = meldung_ausgeben();
    ergebnis_ausgeben(installieren::starten(std::path::Path::new(firmware), probelauf, &mut ausgabe))
}

/// Rückweg zur Werks-Firmware: „werk-zurueck [werk-sicherung.bin]“ (Vorgabe: neueste Sicherung aus „installieren“)
#[cfg(target_os = "linux")]
fn werk_zurueck_befehl(a: &[String]) -> ExitCode {
    let Some(sicherung) = a.first().map(std::path::PathBuf::from).or_else(installieren::neueste_werk_sicherung) else {
        eprintln!("Keine Sicherung gefunden. Aufruf: c2lite-custom werk-zurueck <werk-sicherung.bin>");
        return ExitCode::FAILURE;
    };
    println!("Sicherung: {}", sicherung.display());
    let mut ausgabe = meldung_ausgeben();
    ergebnis_ausgeben(installieren::werk_zurueck(&sicherung, &mut ausgabe))
}

#[cfg(target_os = "linux")]
fn meldung_ausgeben() -> impl FnMut(installieren::Meldung) {
    let mut im_fortschritt = false;
    move |m| {
        use installieren::Meldung::*;
        if im_fortschritt && !matches!(m, Fortschritt(_)) {
            println!();
            im_fortschritt = false;
        }
        match m {
            Schritt(t) => println!("» {t}"),
            Hinweis(t) => println!("\n  >>> {t} <<<\n"),
            Fortschritt(f) => {
                print!("\r    {:3.0} %", f * 100.0);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                im_fortschritt = true;
            }
            Fertig(t) => println!("\n✔ {t}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn ergebnis_ausgeben(r: std::io::Result<()>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("\n✘ {e}");
            ExitCode::FAILURE
        }
    }
}

/// Fehlersuche im ROM-Download-Modus: „download lesen <datei> [adresse] [länge]“, „download schreiben <adresse> <datei>“,
/// „download neustart“. Der Controller muss schon im Download-Modus sein (z. B. nach „download-modus“).
#[cfg(target_os = "linux")]
fn download_befehl(a: &[String]) -> Result<(), String> {
    let zahl = |t: Option<&String>, vorgabe: u32| -> Result<u32, String> {
        match t {
            None => Ok(vorgabe),
            Some(t) => u32::from_str_radix(t.trim_start_matches("0x"), 16).map_err(|_| format!("„{t}“ ist keine Hex-Zahl")),
        }
    };
    let pfad = download::warten(Duration::from_secs(10)).ok_or("Kein Controller im Download-Modus (BR23 UBOOT1.00)")?;
    let mut d = download::Download::oeffnen(&pfad).map_err(|e| format!("{pfad}: {e}"))?;
    let (h, p) = d.kennung().map_err(|e| e.to_string())?;
    eprintln!("{pfad}: {h} {p}");
    let t = Instant::now();
    d.lader_starten(&download::lader_holen().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    eprintln!("Lader läuft ({:.2} s)", t.elapsed().as_secs_f32());
    let mut fortschritt = |f: f32| eprint!("\r{:3.0} %", f * 100.0);
    match a.first().map(String::as_str) {
        Some("lesen") => {
            let datei = a.get(1).ok_or("Datei fehlt")?;
            let (adr, len) = (zahl(a.get(2), 0)?, zahl(a.get(3), download::FLASH_GROESSE)?);
            let daten = d.lesen(adr, len, &mut fortschritt).map_err(|e| e.to_string())?;
            std::fs::write(datei, &daten).map_err(|e| e.to_string())?;
            eprintln!("\r{len} Byte gelesen in {:.2} s", t.elapsed().as_secs_f32());
        }
        Some("schreiben") => {
            let adr = zahl(a.get(1), 0)?;
            let daten = std::fs::read(a.get(2).ok_or("Datei fehlt")?).map_err(|e| e.to_string())?;
            d.schreiben(adr, &daten, &mut fortschritt).map_err(|e| e.to_string())?;
            eprintln!("\r{} Byte geschrieben in {:.2} s", daten.len(), t.elapsed().as_secs_f32());
        }
        Some("neustart") => d.neustart(),
        _ => return Err("download lesen <datei> [adresse] [länge] | schreiben <adresse> <datei> | neustart".into()),
    }
    Ok(())
}

fn zeigen(zeile: String) -> Result<(), String> {
    println!("{zeile}");
    Ok(())
}

fn antwort(zeile: String) -> Result<(), String> {
    match zeile.strip_prefix("fehler:") {
        Some(rest) => Err(rest.trim().to_string()),
        None => zeigen(zeile),
    }
}

/// Spieler-LEDs 1..4 sitzen auf den Kanälen 3..6 (Maske 0x04..0x20, am Gerät geprüft); die Kanäle 0x01, 0x02
/// und 0x40 sind nicht sichtbar. „alle“, „aus“, Spieler-Nummern („1,3“) oder Rohmaske als Hex („0x3c“).
pub fn led_maske(t: &str) -> Result<u8, String> {
    let fehler = || format!("„{t}“: erwartet alle, aus, Spieler-Nummern wie 1,3 oder eine Maske 0x00..0x7f");
    match t {
        "alle" | "an" => return Ok(0x3c),
        "aus" => return Ok(0),
        _ => {}
    }
    if let Some(h) = t.strip_prefix("0x") {
        return u8::from_str_radix(h, 16).ok().filter(|m| *m <= 0x7f).ok_or_else(fehler);
    }
    let mut m = 0u8;
    for n in t.split(',') {
        let n: u8 = n.trim().parse().map_err(|_| fehler())?;
        if !(1..=4).contains(&n) {
            return Err(fehler());
        }
        m |= spieler_bit(n);
    }
    Ok(m)
}

/// Maskenbit der Spieler-LED n (1..4)
pub fn spieler_bit(n: u8) -> u8 {
    0x04 << (n - 1)
}

const MODI: &[(&str, u8)] =
    &[("statisch", 0), ("atmen", 1), ("regenbogen", 2), ("farbwechsel", 3), ("aus", 4), ("reaktiv", 5)];
const FARBEN: &[(&str, u32)] = &[
    ("weiss", 0xffffff), ("weiß", 0xffffff), ("rot", 0xff0000), ("gruen", 0x00ff00), ("grün", 0x00ff00),
    ("blau", 0x0000ff), ("gelb", 0xffc800), ("orange", 0xff6000), ("lila", 0xa000ff), ("pink", 0xff0090),
    ("tuerkis", 0x00dcff), ("türkis", 0x00dcff),
];

/// Lichteinstellungen: ohne Argument anzeigen, sonst setzen (der Controller speichert sofort)
fn licht(g: &mut dyn Kanal, a: &[String]) -> Result<(), String> {
    let setze = |g: &mut dyn Kanal, name: &str, wert: u32| -> Result<(), String> {
        antwort(g.befehl(&format!("setze {name} {wert}"), ANTWORT, WARTEN)?)
    };
    let zahl = |t: Option<&String>, max: u32| -> Result<u32, String> {
        let t = t.ok_or("Wert fehlt")?;
        let v: u32 = t.trim_end_matches('%').parse().map_err(|_| format!("„{t}“ ist keine Zahl"))?;
        if v > max { Err(format!("{v} ist größer als {max}")) } else { Ok(v) }
    };
    match a.first().map(String::as_str) {
        None => {
            let z = g.befehl("einstellungen", &["einstellungen:"], WARTEN)?;
            let wert = |k: &str| z.split_whitespace().find_map(|t| t.strip_prefix(&format!("{k}=")).map(str::to_string));
            let modus = wert("licht").and_then(|m| m.parse::<u8>().ok());
            let name = MODI.iter().find(|(_, n)| Some(*n) == modus).map(|(s, _)| *s).unwrap_or("?");
            println!(
                "Modus {name}, Helligkeit {} %, Logo {} %, Tempo {}, Farbe #{}",
                wert("hell").unwrap_or_default(),
                wert("logo").unwrap_or_default(),
                wert("tempo").unwrap_or_default(),
                wert("farbe").unwrap_or_default()
            );
            Ok(())
        }
        Some("farbe") => {
            let t = a.get(1).ok_or("Farbe fehlt, z. B. ff8800 oder rot")?;
            let v = FARBEN
                .iter()
                .find(|(n, _)| n == t)
                .map(|(_, v)| *v)
                .or_else(|| u32::from_str_radix(t.trim_start_matches('#'), 16).ok().filter(|v| *v <= 0xffffff))
                .ok_or_else(|| format!("„{t}“: erwartet RRGGBB (z. B. ff8800) oder einen Namen"))?;
            setze(g, "farbe", v)
        }
        Some("helligkeit") => setze(g, "hell", zahl(a.get(1), 100)?),
        Some("logo") => setze(g, "logo", zahl(a.get(1), 100)?),
        Some("tempo") => setze(g, "tempo", zahl(a.get(1), 100)?.max(1)),
        Some("standard") => {
            setze(g, "licht", 2)?;
            setze(g, "hell", 15)?;
            setze(g, "logo", 25)?;
            setze(g, "tempo", 30)
        }
        Some(m) => match MODI.iter().find(|(n, _)| *n == m) {
            Some((_, n)) => setze(g, "licht", *n as u32),
            None => Err(format!("unbekannt: „{m}“ (c2lite-custom hilfe)")),
        },
    }
}

/// CRC16-CCITT, Startwert 0 (wie die Firmware, c2_ota.c)
fn crc16(daten: &[u8]) -> u16 {
    let mut c: u16 = 0;
    for &b in daten {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 };
        }
    }
    c
}

/// Update in die freie Bank: Daten in Zeilen zu 256 Byte, jede Zeile wird erst bestätigt, wenn sie gespeichert ist.
/// Bei einem Fehler bleibt die laufende Firmware aktiv.
fn update(g: &mut dyn Kanal, datei: &str) -> Result<(), String> {
    let daten = std::fs::read(datei).map_err(|e| format!("{datei}: {e}"))?;
    let start = std::time::Instant::now();
    let mut zuletzt = usize::MAX;
    update_daten(g, &daten, &mut |anteil| {
        let fertig = (anteil * 100.0) as usize;
        if fertig != zuletzt {
            eprint!("\rÜbertrage {fertig:3} %");
            zuletzt = fertig;
        }
    })?;
    eprintln!("\rÜbertragen und geprüft in {:.1} s", start.elapsed().as_secs_f32());
    println!("Der Controller startet gleich mit der neuen Firmware neu.");
    Ok(())
}

/// Überträgt ein Update (db_update_data.bin) und schaltet die Bank um; `fortschritt` bekommt 0.0..1.0
pub fn update_daten(g: &mut dyn Kanal, daten: &[u8], fortschritt: &mut dyn FnMut(f32)) -> Result<(), String> {
    if daten.is_empty() {
        return Err("Die Update-Datei ist leer".into());
    }
    let lang = Duration::from_secs(5);
    antwort_still(g.befehl(&format!("ota start {}", daten.len()), ANTWORT, lang)?)?;
    for (i, stueck) in daten.chunks(256).enumerate() {
        let hex: String = stueck.iter().map(|b| format!("{b:02x}")).collect();
        let a = g.befehl(&format!("ota d {hex}"), ANTWORT, lang)?;
        if a.starts_with("fehler:") {
            let _ = g.befehl("ota abbruch", ANTWORT, WARTEN);
            return Err(a);
        }
        fortschritt((i * 256 + stueck.len()) as f32 / daten.len() as f32);
    }
    let crc = crc16(daten);
    let a = g.befehl(&format!("ota ende {crc}"), &["ok: ota fertig", "fehler:"], Duration::from_secs(20))?;
    antwort_still(a)
}

fn antwort_still(zeile: String) -> Result<(), String> {
    match zeile.strip_prefix("fehler:") {
        Some(rest) => Err(rest.trim().to_string()),
        None => Ok(()),
    }
}

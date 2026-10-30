//! Erstinstallation der eigenen Firmware (C2 Lite Custom) auf einer C2 Lite mit Werks-Firmware, ohne Zusatzhardware (Linux).
//! Ablauf (am Gerät erprobt 30.10.): Laptop-Bluetooth koppelt mit der Werks-Firmware (werk_ble) und spielt über den
//! offiziellen Update-Dienst die Brücke ein (abbild::bruecke_bauen); der Controller wartet danach dunkel auf das
//! USB-Kabel und springt dann in den ROM-Download-Modus. Dort: ganzer Flash als Sicherung, eigene Firmware schreiben, prüfen.
//! Die Benutzerseite lädt und baut alles; die Arbeit am Gerät braucht root (Bluetooth-Adapter exklusiv, /dev/sg) und
//! läuft als „installieren-kern“ über pkexec. Der Kern meldet sich zeilenweise auf stdout (SCHRITT/HINWEIS/
//! FORTSCHRITT/FERTIG/FEHLER), technische Einzelheiten gehen auf stderr ins Protokoll.
//! Rückweg („werk-zurueck-kern“): eigene Firmware per Befehl in den Download-Modus, aktuellen Speicher sichern, Werks-Abbild aus
//! der Sicherung der Installation (abbild::werk_abbild) schreiben und prüfen.

use crate::{abbild, download, werk_ble};
use std::io::{BufRead, BufReader, Error, Result, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Meldung {
    /// neuer Abschnitt („Brücke übertragen“)
    Schritt(String),
    /// Aufforderung an den Menschen („Jetzt das USB-Kabel einstecken“)
    Hinweis(String),
    Fortschritt(f32),
    Fertig(String),
}

fn zeile(z: &str) {
    println!("{z}");
    let _ = std::io::stdout().flush();
}

// ---------------------------------------------------------------- Geräte-Teil (root)

/// installieren-kern <brücke.bin> <lader.bin> <jl_isd.bin> <sicherungsordner> <uid> [probelauf]
pub fn kern(a: &[String]) -> Result<()> {
    let r = kern_ablauf(a);
    if let Err(e) = &r {
        zeile(&format!("FEHLER {e}"));
    }
    r
}

fn kern_ablauf(a: &[String]) -> Result<()> {
    if a.len() < 5 {
        return Err(Error::other("installieren-kern: Argumente fehlen"));
    }
    let bruecke = std::fs::read(&a[0])?;
    let lader = std::fs::read(&a[1])?;
    let jl_isd = std::fs::read(&a[2])?;
    let ordner = PathBuf::from(&a[3]);
    let (uid, gid) = benutzer(&a[4]);
    let probelauf = a.get(5).map(String::as_str) == Some("probelauf");

    zeile("SCHRITT Bluetooth vorbereiten");
    let mut w = werk_ble::Werk::oeffnen()?;
    if werk_am_usb() {
        /* Mit Kabel koppelt die Werks-Firmware nicht (am Gerät beobachtet): erst abziehen lassen */
        zeile("HINWEIS Bitte zuerst das USB-Kabel vom Controller abziehen.");
        let bis = std::time::Instant::now() + Duration::from_secs(300);
        while werk_am_usb() {
            if std::time::Instant::now() > bis {
                return Err(Error::other("Das USB-Kabel steckt noch. Bitte abziehen und die Installation neu starten."));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    zeile("HINWEIS Controller einschalten und in den Kopplungsmodus bringen (USB-Kabel bleibt ab).");
    zeile("SCHRITT Controller suchen");
    let adr = w.suchen(Duration::from_secs(180))?.ok_or_else(|| {
        Error::other("Kein Controller mit Werks-Firmware gefunden (Kopplungsmodus an? Kabel ab? In der Nähe?)")
    })?;
    zeile(&format!("SCHRITT Verbinden mit {}", werk_ble::adresse_text(&adr)));
    let mut s = w.verbinden(&adr, &mut |t| zeile(&format!("HINWEIS {t}")))?;
    if probelauf {
        let akku = s.abfrage(&[3, 49, 1])?;
        let text = match akku {
            Some(d) if d.len() >= 6 && d[1] == 49 => format!("Akku {} %", d[5]),
            Some(_) => "Antwort erhalten".into(),
            None => "keine Antwort auf die Akku-Abfrage".into(),
        };
        zeile(&format!(
            "FERTIG Probelauf erfolgreich: Verbindung zur Werks-Firmware steht, verschlüsselt, {text}. Nichts wurde verändert."
        ));
        return Ok(());
    }
    zeile("SCHRITT Brücke übertragen (etwa 4–6 Minuten, Controller liegen lassen)");
    let mut zuletzt = -1.0f32;
    s.update(&bruecke, &mut |f| {
        if f - zuletzt >= 0.005 {
            zuletzt = f;
            zeile(&format!("FORTSCHRITT {f:.3}"));
        }
    })?;
    drop(s);
    drop(w);
    zeile("HINWEIS Jetzt das USB-Kabel vom Rechner an den Controller stecken.");
    zeile("SCHRITT Warte auf den Download-Modus");
    let pfad = download::warten(Duration::from_secs(300))
        .ok_or_else(|| Error::other("Der Controller erscheint nicht im Download-Modus (Kabel gesteckt? Datenkabel?)"))?;
    let mut d = download::Download::oeffnen(&pfad)?;
    d.lader_starten(&lader)?;
    zeile("SCHRITT Sicherung des ganzen Speichers lesen");
    let sicherung = d.lesen(0, download::FLASH_GROESSE, &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    std::fs::create_dir_all(&ordner)?;
    let datei = ordner.join(format!("werk-sicherung-{}.bin", zeitstempel()));
    std::fs::write(&datei, &sicherung)?;
    eigentuemer(&ordner, uid, gid);
    eigentuemer(&datei, uid, gid);
    zeile(&format!("HINWEIS Sicherung gespeichert: {}", datei.display()));
    let soll = abbild::custom_abbild(&sicherung, &jl_isd)?;
    zeile("SCHRITT C2 Lite Custom schreiben");
    d.schreiben(0, &soll[..abbild::BEREICH_ENDE], &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    zeile("SCHRITT Prüfen");
    let zurueck = d.lesen(0, abbild::BEREICH_ENDE as u32, &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    if zurueck != soll[..abbild::BEREICH_ENDE] {
        return Err(Error::other(
            "Der geschriebene Speicher weicht ab. Controller NICHT abziehen und die Installation noch einmal starten.",
        ));
    }
    d.neustart();
    zeile("FERTIG C2 Lite Custom ist installiert und geprüft. Der Controller startet neu.");
    Ok(())
}

/// werk-zurueck-kern <werk-abbild.bin> <lader.bin> <sicherungsordner> <uid:gid>
pub fn werk_zurueck_kern(a: &[String]) -> Result<()> {
    let r = werk_zurueck_ablauf(a);
    if let Err(e) = &r {
        zeile(&format!("FEHLER {e}"));
    }
    r
}

fn werk_zurueck_ablauf(a: &[String]) -> Result<()> {
    if a.len() < 4 {
        return Err(Error::other("werk-zurueck-kern: Argumente fehlen"));
    }
    let soll = std::fs::read(&a[0])?;
    let lader = std::fs::read(&a[1])?;
    let ordner = PathBuf::from(&a[2]);
    let (uid, gid) = benutzer(&a[3]);
    if soll.len() != download::FLASH_GROESSE as usize {
        return Err(Error::other("Werks-Abbild hat die falsche Größe"));
    }

    zeile("SCHRITT Download-Modus starten");
    if download::warten(Duration::ZERO).is_none() {
        if crate::geraet::finden().is_none() {
            zeile("HINWEIS Controller mit C2 Lite Custom per USB-Kabel an den Rechner stecken.");
        }
        let bis = std::time::Instant::now() + Duration::from_secs(300);
        let pfad = loop {
            if let Some(p) = crate::geraet::finden() {
                break p;
            }
            if std::time::Instant::now() > bis {
                return Err(Error::other("Kein Controller mit C2 Lite Custom am USB gefunden (Kabel gesteckt? Datenkabel?)"));
            }
            std::thread::sleep(Duration::from_millis(300));
        };
        // Der Port ist exklusiv; hält ihn noch ein anderes Programm (Oberfläche, Terminal), kurz auf die Freigabe warten
        let bis = std::time::Instant::now() + Duration::from_secs(15);
        let mut g = loop {
            match crate::geraet::Geraet::oeffnen(&pfad) {
                Ok(g) => break g,
                Err(e) if std::time::Instant::now() > bis => {
                    eprintln!("{e}");
                    return Err(Error::other(
                        "Der Controller ist von einem anderen Programm belegt (zweite Oberfläche oder Terminal?). Bitte \
                         schließen und noch einmal versuchen. Nichts wurde verändert.",
                    ));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(500)),
            }
        };
        use crate::geraet::Kanal;
        // Ohne Antwort weiter: Springt der Controller schon, geht sie verloren; entscheidend ist der Download-Modus
        if let Ok(antwort) = g.befehl("download-modus", &["ok:", "fehler:"], Duration::from_secs(2)) {
            if antwort.starts_with("fehler:") {
                return Err(Error::other(format!("Die Firmware lehnt den Download-Modus ab: {antwort}")));
            }
        }
        drop(g);
    }
    let pfad = download::warten(Duration::from_secs(30))
        .ok_or_else(|| Error::other("Der Controller erscheint nicht im Download-Modus"))?;
    let mut d = download::Download::oeffnen(&pfad)?;
    d.lader_starten(&lader)?;
    zeile("SCHRITT Aktuellen Stand sichern (mit Kopplungen)");
    let jetzt = d.lesen(0, download::FLASH_GROESSE, &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    if jetzt[abbild::ADRESSE] != soll[abbild::ADRESSE] {
        d.neustart();
        return Err(Error::other(
            "Die Sicherung gehört zu einem anderen Controller (Bluetooth-Adresse passt nicht). Nichts wurde verändert.",
        ));
    }
    std::fs::create_dir_all(&ordner)?;
    let datei = ordner.join(format!("custom-sicherung-{}.bin", zeitstempel()));
    std::fs::write(&datei, &jetzt)?;
    eigentuemer(&ordner, uid, gid);
    eigentuemer(&datei, uid, gid);
    zeile(&format!("HINWEIS Sicherung gespeichert: {}", datei.display()));
    zeile("SCHRITT Werks-Firmware schreiben");
    d.schreiben(0, &soll[..abbild::RUECKWEG_ENDE], &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    zeile("SCHRITT Prüfen");
    let zurueck = d.lesen(0, abbild::RUECKWEG_ENDE as u32, &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    if zurueck != soll[..abbild::RUECKWEG_ENDE] {
        return Err(Error::other(
            "Der geschriebene Speicher weicht ab. Controller NICHT abziehen und den Rückweg noch einmal starten.",
        ));
    }
    d.neustart();
    zeile(
        "FERTIG Die Werks-Firmware ist zurück und geprüft. Der Controller startet neu; die Mobapad-App und die Kopplungen \
         von vorher funktionieren wieder.",
    );
    Ok(())
}

fn benutzer(t: &str) -> (u32, u32) {
    match t.split_once(':') {
        Some((u, g)) => (u.parse().unwrap_or(0), g.parse().unwrap_or(0)),
        None => (t.parse().unwrap_or(0), t.parse().unwrap_or(0)),
    }
}

fn zeitstempel() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{t}")
}

/// Dateien des root-Kerns dem Benutzer übergeben, der pkexec aufgerufen hat
fn eigentuemer(p: &Path, uid: u32, gid: u32) {
    if uid == 0 {
        return;
    }
    if let Ok(c) = std::ffi::CString::new(p.as_os_str().to_string_lossy().as_bytes()) {
        unsafe { libc::chown(c.as_ptr(), uid, gid) };
    }
}

/// Steckt eine C2 Lite mit Werks-Firmware am USB? (2345:e06e ohne „dazi“ im Produktnamen; das Easter Egg der eigenen Firmware)
fn werk_am_usb() -> bool {
    let Ok(rd) = std::fs::read_dir("/sys/bus/usb/devices") else { return false };
    rd.flatten().any(|e| {
        let lies = |n: &str| std::fs::read_to_string(e.path().join(n)).map(|t| t.trim().to_string()).unwrap_or_default();
        lies("idVendor") == "2345" && lies("idProduct") == "e06e" && !lies("product").contains("dazi")
    })
}

// ---------------------------------------------------------------- Benutzerseite

/// Vorbereiten (Downloads, Brücke) und den Geräte-Teil über pkexec ausführen; Meldungen gehen an `melden`
pub fn starten(firmware: &Path, probelauf: bool, melden: &mut dyn FnMut(Meldung)) -> Result<()> {
    melden(Meldung::Schritt("Controller prüfen".into()));
    if crate::geraet::finden().is_some() {
        return Err(Error::other(
            "Am USB hängt schon ein Controller mit C2 Lite Custom. Updates gehen über die Seite „Firmware“; die Erstinstallation ist \
             für Controller mit Werks-Firmware.",
        ));
    }
    let jl_isd = std::fs::read(firmware).map_err(|e| Error::other(format!("{}: {e}", firmware.display())))?;
    if jl_isd.len() % 0x1000 != 0 || jl_isd.len() > 0x7E000 || jl_isd.len() < 0x10000 {
        return Err(Error::other(format!("{} ist kein Abbild von C2 Lite Custom (jl_isd.bin)", firmware.display())));
    }
    melden(Meldung::Schritt("Werks-Firmware 0.04 und Lader laden (einmalig, aus dem Internet)".into()));
    let werk = abbild::holen(abbild::WERK_URL, abbild::WERK_SHA256, "C2-lite-0.04.bin")?;
    download::lader_holen()?;
    melden(Meldung::Schritt("Brücke bauen".into()));
    let bruecke = abbild::bruecke_bauen(&werk)?;
    let cache = abbild::zwischenspeicher();
    let bruecke_pfad = cache.join("bruecke2.bin");
    std::fs::write(&bruecke_pfad, &bruecke)?;
    let ordner = sicherungsordner();
    let mut args: Vec<std::ffi::OsString> = vec![
        "installieren-kern".into(),
        bruecke_pfad.into(),
        cache.join("br23loader.bin").into(),
        std::fs::canonicalize(firmware)?.into(),
        ordner.clone().into(),
        benutzer_text().into(),
    ];
    if probelauf {
        args.push("probelauf".into());
    }
    kern_ausfuehren(&args, "installation", melden)
}

/// Rückweg zur Werks-Firmware aus einer Sicherung der Installation (~/c2lite-sicherung/werk-sicherung-*.bin)
pub fn werk_zurueck(sicherung: &Path, melden: &mut dyn FnMut(Meldung)) -> Result<()> {
    melden(Meldung::Schritt("Sicherung prüfen".into()));
    let s = std::fs::read(sicherung).map_err(|e| Error::other(format!("{}: {e}", sicherung.display())))?;
    let soll = abbild::werk_abbild(&s)?;
    melden(Meldung::Schritt("Lader laden".into()));
    download::lader_holen()?;
    let cache = abbild::zwischenspeicher();
    let soll_pfad = cache.join("werk-rueckweg.bin");
    std::fs::write(&soll_pfad, &soll)?;
    let args: Vec<std::ffi::OsString> = vec![
        "werk-zurueck-kern".into(),
        soll_pfad.into(),
        cache.join("br23loader.bin").into(),
        sicherungsordner().into(),
        benutzer_text().into(),
    ];
    kern_ausfuehren(&args, "rueckweg", melden)
}

/// Neueste Sicherung aus „Installieren“, falls vorhanden
pub fn neueste_werk_sicherung() -> Option<PathBuf> {
    std::fs::read_dir(sicherungsordner())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            n.starts_with("werk-sicherung-") && n.ends_with(".bin")
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

fn benutzer_text() -> String {
    format!("{}:{}", unsafe { libc::getuid() }, unsafe { libc::getgid() })
}

/// Geräte-Teil über pkexec ausführen und seine Zeilen als Meldungen weitergeben; stderr geht ins Protokoll
fn kern_ausfuehren(args: &[std::ffi::OsString], name: &str, melden: &mut dyn FnMut(Meldung)) -> Result<()> {
    let ordner = sicherungsordner();
    std::fs::create_dir_all(&ordner)?;
    let protokoll_pfad = ordner.join(format!("{name}-{}.log", zeitstempel()));
    let protokoll = std::fs::File::create(&protokoll_pfad)?;

    melden(Meldung::Schritt("Rechte für Bluetooth und Download-Modus anfordern".into()));
    // Nach einem Neubau während das Programm läuft, meldet Linux „… (deleted)“: dann die neue Datei am selben Ort nehmen
    let exe = std::env::current_exe()?;
    let exe = match exe.to_str().and_then(|t| t.strip_suffix(" (deleted)")) {
        Some(t) if Path::new(t).exists() => PathBuf::from(t),
        _ => exe,
    };
    let mut kind = Command::new("pkexec").arg(exe).args(args).stdout(Stdio::piped()).stderr(Stdio::from(protokoll)).spawn()?;
    let mut ergebnis: Option<std::result::Result<String, String>> = None;
    for z in BufReader::new(kind.stdout.take().unwrap()).lines() {
        let z = z?;
        let (art, rest) = z.split_once(' ').unwrap_or((z.as_str(), ""));
        match art {
            "SCHRITT" => melden(Meldung::Schritt(rest.into())),
            "HINWEIS" => melden(Meldung::Hinweis(rest.into())),
            "FORTSCHRITT" => melden(Meldung::Fortschritt(rest.parse().unwrap_or(0.0))),
            "FERTIG" => ergebnis = Some(Ok(rest.into())),
            "FEHLER" => ergebnis = Some(Err(rest.into())),
            _ => {}
        }
    }
    let status = kind.wait()?;
    match ergebnis {
        Some(Ok(t)) => {
            melden(Meldung::Fertig(t));
            Ok(())
        }
        Some(Err(t)) => Err(Error::other(format!("{t} (Protokoll: {})", protokoll_pfad.display()))),
        None => {
            // Der Kern hat nichts gemeldet: Grund steht im Protokoll (z. B. Passwortabfrage abgebrochen, pkexec-Fehler)
            let grund = std::fs::read_to_string(&protokoll_pfad).unwrap_or_default();
            let grund = grund.lines().find(|z| !z.trim().is_empty()).unwrap_or("").to_string();
            Err(Error::other(match (status.code(), grund.is_empty()) {
                (Some(126), true) => "Rechte wurden nicht erteilt (Passwortabfrage abgebrochen).".to_string(),
                (_, false) => format!("Abgebrochen: {grund}"),
                _ => format!("Abgebrochen (Protokoll: {})", protokoll_pfad.display()),
            }))
        }
    }
}

/// ~/c2lite-sicherung: Sicherungen der Werks-Firmware und Protokolle
pub fn sicherungsordner() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("c2lite-sicherung")
}

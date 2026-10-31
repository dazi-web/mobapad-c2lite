//! Erstinstallation der eigenen Firmware (C2 Lite Custom) auf einer C2 Lite mit Werks-Firmware, ohne Zusatzhardware (Linux).
//! Ablauf (am Gerät erprobt 30.10.): Laptop-Bluetooth koppelt mit der Werks-Firmware (werk_ble) und spielt über den
//! offiziellen Update-Dienst die Brücke ein (abbild::bruecke_bauen); der Controller wartet danach dunkel auf das
//! USB-Kabel und springt dann in den ROM-Download-Modus. Dort: ganzer Flash als Sicherung, eigene Firmware schreiben, prüfen.
//! Die Benutzerseite lädt und baut alles; die Arbeit am Gerät braucht root (Bluetooth-Adapter exklusiv, /dev/sg) und
//! läuft als „installieren-kern“ über pkexec. Der Kern meldet sich zeilenweise auf stdout (SCHRITT/HINWEIS/
//! FORTSCHRITT/FERTIG/FEHLER), technische Einzelheiten gehen auf stderr ins Protokoll.
//! Rückweg („werk-zurueck-kern“): eigene Firmware per Befehl in den Download-Modus, aktuellen Speicher sichern, Werks-Abbild aus
//! der Sicherung der Installation (abbild::werk_abbild) schreiben und prüfen.

use crate::{abbild, download, werk_ble, werk_usb};
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
    zeile("SCHRITT Controller suchen");
    zeile("HINWEIS Controller ohne Kabel einschalten und in den Kopplungsmodus bringen.");
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
    zeile("SCHRITT Brücke übertragen (etwa 5 Minuten)");
    zeile("HINWEIS Controller liegen lassen.");
    let mut zuletzt = -1.0f32;
    s.update(&bruecke, &mut |f| {
        if f - zuletzt >= 0.005 {
            zuletzt = f;
            zeile(&format!("FORTSCHRITT {f:.3}"));
        }
    })?;
    drop(s);
    drop(w);
    zeile("SCHRITT Warte auf den Download-Modus");
    zeile("HINWEIS Jetzt das USB-Kabel anstecken.");
    let pfad = download::warten(Duration::from_secs(300))
        .ok_or_else(|| Error::other("Der Controller erscheint nicht im Download-Modus (Kabel gesteckt? Datenkabel?)"))?;
    im_download_modus(&pfad, &lader, &jl_isd, &ordner, (uid, gid), "werk-sicherung", false)
}

/// installieren-usb-kern <lader.bin> <jl_isd.bin> <sicherungsordner> <uid:gid> [probelauf]
pub fn usb_kern(a: &[String]) -> Result<()> {
    let r = usb_ablauf(a);
    if let Err(e) = &r {
        zeile(&format!("FEHLER {e}"));
    }
    r
}

/// USB-Weg (docs/WERKS-DOWNLOAD.md): Werks-Firmware am Kabel, Rumble-Impulse halten „USB aktiv“, der Mensch hält
/// Home + Y + X, die Werks-Firmware springt in den ROM-Download-Modus. Ohne Bluetooth und ohne Brücke.
fn usb_ablauf(a: &[String]) -> Result<()> {
    if a.len() < 4 {
        return Err(Error::other("installieren-usb-kern: Argumente fehlen"));
    }
    let lader = std::fs::read(&a[0])?;
    let jl_isd = std::fs::read(&a[1])?;
    let ordner = PathBuf::from(&a[2]);
    let benutzer = benutzer(&a[3]);
    let probelauf = a.get(4).map(String::as_str) == Some("probelauf");

    zeile("SCHRITT Controller am USB suchen");
    // Ohne Quirk fragt Linux den Device_Qualifier ab: Daran hängt sich die Werks-Firmware auf, und dieselbe Anfrage
    // sperrt Home + Y + X. Der Quirk gilt nur für 2345:e06e und nur bis zum nächsten Neustart des Rechners.
    let quirk_neu = quirk_setzen()?;
    if let Some(pfad) = download::warten(Duration::ZERO) {
        // Schon im Download-Modus (z. B. nach einem abgebrochenen Versuch): Inhalt unbekannt, eigener Name für die Sicherung,
        // damit sie nie als Werks-Sicherung für den Rückweg gilt
        zeile("SCHRITT Controller ist schon im Download-Modus");
        return im_download_modus(&pfad, &lader, &jl_isd, &ordner, benutzer, "download-sicherung", probelauf);
    }
    if quirk_neu && werk_am_usb() {
        // Schon ohne Quirk angemeldet: gilt erst nach neuem Einstecken
        zeile("HINWEIS USB-Kabel einmal abziehen.");
        let bis = std::time::Instant::now() + Duration::from_secs(120);
        while werk_am_usb() {
            if std::time::Instant::now() > bis {
                return Err(Error::other("Das USB-Kabel steckt noch. Bitte abziehen und noch einmal starten. Nichts wurde verändert."));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    if !werk_am_usb() {
        zeile("HINWEIS Kabel ab, Controller einschalten (Schieber auf PC, Home), dann Kabel anstecken.");
        let bis = std::time::Instant::now() + Duration::from_secs(300);
        while !werk_am_usb() {
            if std::time::Instant::now() > bis {
                return Err(Error::other("Kein Controller mit Werks-Firmware am USB gefunden (Kabel gesteckt? Datenkabel? Schieber auf PC?)"));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    // Das Eingabegerät kommt kurz nach dem USB-Gerät; nach dem Einschalten meldet sich die Werks-Firmware nach ≈ 4 s einmal
    // um, das fängt der Wachhalter ab
    let bis = std::time::Instant::now() + Duration::from_secs(15);
    let ev = loop {
        if let Some(p) = werk_usb::eingabegeraet() {
            break p;
        }
        if std::time::Instant::now() > bis {
            return Err(Error::other("Eingabegerät des Controllers nicht gefunden (Treiber xpad geladen? Schieber auf PC?)"));
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    zeile("SCHRITT USB-Verkehr starten");
    let wach = werk_usb::Wachhalter::starten(&ev).map_err(|e| Error::other(format!("Vibration an {} nicht möglich: {e}", ev.display())))?;
    zeile("SCHRITT Warte auf den Download-Modus");
    zeile("HINWEIS Home + Y + X halten, bis das Brummen aufhört.");
    let pfad = download::warten(Duration::from_secs(90));
    drop(wach);
    let pfad = pfad.ok_or_else(|| {
        Error::other(
            "Kein Download-Modus nach 90 s. Kabel abziehen, Controller ohne Kabel einschalten und noch einmal versuchen \
             (mit Kabel eingeschaltet bleibt die Werks-Firmware im Lademodus). Nichts wurde verändert.",
        )
    })?;
    im_download_modus(&pfad, &lader, &jl_isd, &ordner, benutzer, "werk-sicherung", probelauf)
}

/// Gemeinsamer Teil beider Wege, sobald der Controller im ROM-Download-Modus ist: Lader starten, ganzen Speicher sichern,
/// Abbild bauen; im Probelauf danach nur neu starten, sonst schreiben, zurücklesen, vergleichen, neu starten.
fn im_download_modus(
    pfad: &str,
    lader: &[u8],
    jl_isd: &[u8],
    ordner: &Path,
    (uid, gid): (u32, u32),
    sicherung_name: &str,
    probelauf: bool,
) -> Result<()> {
    let mut d = download::Download::oeffnen(pfad)?;
    d.lader_starten(lader)?;
    zeile("SCHRITT Sicherung des ganzen Speichers lesen");
    let sicherung = d.lesen(0, download::FLASH_GROESSE, &mut |f| zeile(&format!("FORTSCHRITT {f:.3}")))?;
    std::fs::create_dir_all(ordner)?;
    let datei = ordner.join(format!("{sicherung_name}-{}.bin", zeitstempel()));
    std::fs::write(&datei, &sicherung)?;
    eigentuemer(ordner, uid, gid);
    eigentuemer(&datei, uid, gid);
    zeile(&format!("SCHRITT Sicherung gespeichert: ~/c2lite-sicherung/{}", datei.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    let soll = abbild::custom_abbild(&sicherung, jl_isd)?;
    if probelauf {
        d.neustart();
        zeile(
            "FERTIG Probelauf erfolgreich: Download-Modus erreicht, Speicher gesichert, Abbild gebaut. Nichts wurde geschrieben; \
             der Controller startet mit der Werks-Firmware neu.",
        );
        return Ok(());
    }
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
            zeile("HINWEIS Controller per USB-Kabel anstecken.");
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
    zeile(&format!("SCHRITT Sicherung gespeichert: ~/c2lite-sicherung/{}", datei.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
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

/// USB-Quirk „Device_Qualifier nicht abfragen“ (i) für 2345:e06e zur Laufzeit setzen, falls er fehlt; true = neu gesetzt.
/// Andere Quirks bleiben erhalten. Wirkt auf neue Anmeldungen bis zum Neustart des Rechners.
fn quirk_setzen() -> Result<bool> {
    const PFAD: &str = "/sys/module/usbcore/parameters/quirks";
    let alt = std::fs::read_to_string(PFAD).unwrap_or_default().trim().to_string();
    if alt.split(',').any(|q| q.to_ascii_lowercase().starts_with("2345:e06e:")) {
        return Ok(false);
    }
    let neu = if alt.is_empty() { "2345:e06e:i".to_string() } else { format!("{alt},2345:e06e:i") };
    std::fs::write(PFAD, &neu).map_err(|e| Error::other(format!("USB-Einstellung {PFAD} nicht zu setzen: {e}")))?;
    eprintln!("usbcore.quirks: „{alt}“ → „{neu}“");
    Ok(true)
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

/// Beim Bauen eingebettete Firmware (crate::paket, release/jl_isd.bin des Repos)
const EINGEBAUT: &[u8] = crate::paket::JL_ISD;

/// Stand der eingebauten Firmware, z. B. „C2 Lite Custom, Stufe 11c“
pub fn eingebaut_stand() -> &'static str {
    crate::paket::stand()
}

/// Prüft, dass am USB kein C2 Lite Custom hängt, und liefert den Pfad der Firmware für den root-Kern: die gewählte Datei
/// oder (None) die eingebaute, in den Zwischenspeicher geschrieben
fn vorbereiten(firmware: Option<&Path>, melden: &mut dyn FnMut(Meldung)) -> Result<PathBuf> {
    melden(Meldung::Schritt("Controller prüfen".into()));
    if crate::geraet::finden().is_some() {
        return Err(Error::other(
            "Am USB hängt schon ein Controller mit C2 Lite Custom. Updates gehen über die Seite „Firmware“; die Erstinstallation ist \
             für Controller mit Werks-Firmware.",
        ));
    }
    let (pfad, jl_isd) = match firmware {
        Some(p) => (p.to_path_buf(), std::fs::read(p).map_err(|e| Error::other(format!("{}: {e}", p.display())))?),
        None => {
            let p = abbild::zwischenspeicher().join("jl_isd-eingebaut.bin");
            std::fs::create_dir_all(abbild::zwischenspeicher())?;
            std::fs::write(&p, EINGEBAUT)?;
            (p, EINGEBAUT.to_vec())
        }
    };
    if jl_isd.len() % 0x1000 != 0 || jl_isd.len() > 0x7E000 || jl_isd.len() < 0x10000 {
        return Err(Error::other(format!("{} ist kein Abbild von C2 Lite Custom (jl_isd.bin)", pfad.display())));
    }
    std::fs::canonicalize(&pfad)
}

/// Vorbereiten (Downloads, Brücke) und den Geräte-Teil über pkexec ausführen; Meldungen gehen an `melden`.
/// `firmware` = None: eingebaute Firmware
pub fn starten(firmware: Option<&Path>, probelauf: bool, melden: &mut dyn FnMut(Meldung)) -> Result<()> {
    let firmware = vorbereiten(firmware, melden)?;
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
        firmware.into(),
        ordner.clone().into(),
        benutzer_text().into(),
    ];
    if probelauf {
        args.push("probelauf".into());
    }
    kern_ausfuehren(&args, "installation", melden)
}

/// USB-Weg (Home + Y + X, docs/WERKS-DOWNLOAD.md): nur Lader laden und den Geräte-Teil über pkexec ausführen; keine
/// Werks-Firmware-Datei und keine Brücke nötig
pub fn starten_usb(firmware: Option<&Path>, probelauf: bool, melden: &mut dyn FnMut(Meldung)) -> Result<()> {
    let firmware = vorbereiten(firmware, melden)?;
    melden(Meldung::Schritt("Lader laden (einmalig, aus dem Internet)".into()));
    download::lader_holen()?;
    let mut args: Vec<std::ffi::OsString> = vec![
        "installieren-usb-kern".into(),
        abbild::zwischenspeicher().join("br23loader.bin").into(),
        firmware.into(),
        sicherungsordner().into(),
        benutzer_text().into(),
    ];
    if probelauf {
        args.push("probelauf".into());
    }
    kern_ausfuehren(&args, if probelauf { "probelauf-usb" } else { "installation-usb" }, melden)
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

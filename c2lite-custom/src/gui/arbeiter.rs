//! Hintergrund-Thread: hält die Verbindung (USB oder Bluetooth) und führt Befehle nacheinander aus, damit die
//! Oberfläche nie auf den Controller wartet. Bluetooth (btleplug/tokio) bleibt so ganz in diesem Thread.

use crate::geraet::{self, Geraet, Kanal, Status};
use eframe::egui;
use crate::wecken::{self, Kennung};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weg {
    Usb,
    Bluetooth,
}

/// Wofür eine Antwort gedacht ist (die Oberfläche ordnet sie damit zu)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zweck {
    Info,
    SwitchDiag,
    Einstellungen,
    Belegung,
    Diagnose,
    Still,
    Backup,
    Wiederherstellen,
    /// „pro2 status“ (ab Stufe 11), still: alte Firmware kennt den Befehl nicht
    Pro2,
}

pub enum Auftrag {
    Verbinden(Weg),
    Trennen,
    /// Befehle nacheinander; Antwortzeilen aller Befehle gesammelt
    Befehle(Zweck, Vec<String>),
    Live(bool),
    Update(Vec<u8>),
    /// Wecksignal eines Joy-Con 2 auffangen und die Kennung im Controller speichern; das Flag bricht ab
    WeckenEinrichten(Arc<AtomicBool>),
    /// Musik am Chitu: Befehle in die Warteschlange der Firmware; das Flag bricht ab (dann „chitu ton stop“)
    Musik(Vec<String>, Arc<AtomicBool>),
    /// Befehl ohne auf Antwort zu warten (Klavier, „chitu live …“)
    Schicken(String),
}

pub enum Meldung {
    Sucht(Weg),
    Verbunden(Weg, String),
    Getrennt(Option<String>),
    Antwort(Zweck, Result<Vec<String>, String>),
    Status(Status),
    Fortschritt(f32),
    UpdateFertig(Result<(), String>),
    WeckenFertig(Result<Kennung, String>),
    /// alle Töne angenommen (oder abgebrochen); die Musik selbst läuft noch
    MusikGesendet(Result<(), String>),
    /// Hinweis für die Meldungszeile (Text auf Deutsch, wird übersetzt); true = als Fehler zeigen
    Hinweis(String, bool),
}

/// Hat sich der Controller am USB neu angemeldet (anderes Gerät hinter dem Port, z. B. nach Watchdog-Neustart oder
/// Ruhezustand des PCs)? Dann ist die offene Verbindung tot, obwohl wieder ein Controller da ist.
fn usb_gewechselt(k: &dyn Kanal) -> bool {
    match (k.usb_ziel(), geraet::finden()) {
        (Some(alt), Some(p)) => geraet::serie(&p) == geraet::serie(&k.name()) && std::fs::canonicalize(p).ok().is_some_and(|neu| neu != alt),
        _ => false,
    }
}

/// Ist der verbundene Controller abgezogen, während ein anderer steckt? (finden() fällt dann auf den anderen zurück;
/// der Nutzer soll das sehen und selbst wählen, statt dass die Oberfläche still umschwenkt)
fn usb_anderer(k: &dyn Kanal) -> bool {
    k.usb_ziel().is_some() && geraet::finden().is_some_and(|p| geraet::serie(&p) != geraet::serie(&k.name()))
}

/// Meldung zu einem beim Verbinden gesicherten Nachlass (Controller war vorher abgestürzt oder neu gestartet)
fn nachlass_hinweis(k: &mut dyn Kanal, senden: &dyn Fn(Meldung)) {
    if let Some(d) = k.nachlass_datei() {
        senden(Meldung::Hinweis(
            crate::sprache::tr("Protokoll des letzten Laufs gesichert: {}", &[&d.display()]),
            false,
        ));
    }
}

/// USB neu öffnen (nach Gerätewechsel oder Verbindungsfehler); meldet Verbunden + Hinweis
fn usb_neu(kanal: &mut Option<Box<dyn Kanal>>, senden: &dyn Fn(Meldung)) -> bool {
    *kanal = None;
    match verbinden(Weg::Usb) {
        Ok(mut k) => {
            let name = k.name();
            senden(Meldung::Verbunden(Weg::Usb, name));
            senden(Meldung::Hinweis(
                "Der Controller hat sich am USB neu angemeldet (Neustart oder Ruhezustand des PCs). Automatisch neu verbunden.".into(),
                false,
            ));
            nachlass_hinweis(k.as_mut(), senden);
            *kanal = Some(k);
            true
        }
        Err(_) => false,
    }
}

/// Text für einen Verbindungsfehler, den auch Laien verstehen
const VERBINDUNG_WEG: &str =
    "Verbindung zum Controller unterbrochen (Neustart, Ruhezustand oder Kabel). Bitte Kabel prüfen und neu verbinden.";

const PRAEFIXE: &[&str] = &[
    "ok:", "fehler:", "info:", "status:", "einstellungen:", "akku:", "motor:", "switch:", "log:", "belegung:",
    "turbo:", "Befehle:",
];

pub fn starten(ctx: egui::Context, rx: Receiver<Auftrag>, tx: Sender<Meldung>) {
    std::thread::spawn(move || {
        let senden = |m: Meldung| {
            let _ = tx.send(m);
            ctx.request_repaint();
        };
        let mut kanal: Option<Box<dyn Kanal>> = None;
        let mut kanal_weg = Weg::Usb;
        let mut live = false;
        let mut fehler_in_folge = 0;
        let mut letzter_status = Instant::now();
        let mut letzte_pruefung = Instant::now();
        // USB-Controller weg (Kabel gezogen, Download-Modus, Schieber auf Switch)? Dann Kanal verwerfen und melden, statt
        // alte Werte weiter als „verbunden“ zu zeigen
        let usb_weg = |kanal: &Option<Box<dyn Kanal>>, weg: Weg| kanal.is_some() && weg == Weg::Usb && geraet::finden().is_none();
        loop {
            let warten = if live && kanal.is_some() { Duration::from_millis(15) } else { Duration::from_millis(250) };
            match rx.recv_timeout(warten) {
                Ok(Auftrag::Verbinden(weg)) => {
                    kanal = None;
                    senden(Meldung::Sucht(weg));
                    match verbinden(weg) {
                        Ok(mut k) => {
                            let name = k.name();
                            kanal_weg = weg;
                            fehler_in_folge = 0;
                            senden(Meldung::Verbunden(weg, name));
                            nachlass_hinweis(k.as_mut(), &senden);
                            kanal = Some(k);
                        }
                        Err(e) => senden(Meldung::Getrennt(Some(e))),
                    }
                }
                Ok(Auftrag::Trennen) => {
                    kanal = None;
                    senden(Meldung::Getrennt(None));
                }
                Ok(Auftrag::Live(an)) => live = an,
                Ok(Auftrag::Befehle(zweck, befehle)) => {
                    let ausfuehren = |k: &mut Box<dyn Kanal>| -> Result<Vec<String>, String> {
                        let mut alle = Vec::new();
                        for b in &befehle {
                            alle.extend(k.befehl_alle(b, PRAEFIXE, Duration::from_secs(3))?);
                        }
                        Ok(alle)
                    };
                    let Some(k) = kanal.as_mut() else {
                        senden(Meldung::Antwort(zweck, Err("Nicht verbunden".into())));
                        continue;
                    };
                    let mut ergebnis = ausfuehren(k);
                    // Port tot (Controller neu angemeldet)? Einmal neu verbinden und wiederholen
                    if kanal_weg == Weg::Usb && ergebnis.as_ref().is_err_and(|e| geraet::ist_verbindungsfehler(e)) {
                        ergebnis = if usb_neu(&mut kanal, &senden) {
                            ausfuehren(kanal.as_mut().unwrap())
                        } else {
                            Err(VERBINDUNG_WEG.into())
                        };
                    }
                    let fehlgeschlagen = ergebnis.is_err();
                    senden(Meldung::Antwort(zweck, ergebnis));
                    if fehlgeschlagen && usb_weg(&kanal, kanal_weg) {
                        kanal = None;
                        senden(Meldung::Getrennt(Some("Controller nicht mehr am USB".into())));
                    }
                }
                Ok(Auftrag::Update(daten)) => {
                    let Some(k) = kanal.as_mut() else {
                        senden(Meldung::UpdateFertig(Err("Nicht verbunden".into())));
                        continue;
                    };
                    let mut zuletzt = -1.0;
                    let r = crate::update_daten(k.as_mut(), &daten, &mut |a| {
                        if a - zuletzt >= 0.005 {
                            zuletzt = a;
                            senden(Meldung::Fortschritt(a));
                        }
                    });
                    let ok = r.is_ok();
                    senden(Meldung::UpdateFertig(r));
                    if ok {
                        // Der Controller startet neu: Verbindung ist weg, die Oberfläche verbindet erneut
                        kanal = None;
                        senden(Meldung::Getrennt(None));
                    }
                }
                Ok(Auftrag::WeckenEinrichten(abbruch)) => {
                    let Some(k) = kanal.as_mut() else {
                        senden(Meldung::WeckenFertig(Err("Nicht verbunden".into())));
                        continue;
                    };
                    let r = wecken::auffangen(Duration::from_secs(90), &abbruch).and_then(|kennung| {
                        let a = k.befehl(&kennung.befehl(), &["ok:", "fehler:"], Duration::from_secs(3))?;
                        match a.strip_prefix("fehler:") {
                            Some(f) => Err(f.trim().to_string()),
                            None => Ok(kennung),
                        }
                    });
                    senden(Meldung::WeckenFertig(r));
                }
                Ok(Auftrag::Schicken(b)) => {
                    if let Some(k) = kanal.as_mut() {
                        k.ablesen();
                        if let Err(e) = k.schicken(&b) {
                            if kanal_weg == Weg::Usb && geraet::ist_verbindungsfehler(&e) && !usb_neu(&mut kanal, &senden) {
                                senden(Meldung::Getrennt(Some(VERBINDUNG_WEG.into())));
                            }
                        }
                    }
                }
                Ok(Auftrag::Musik(befehle, abbruch)) => {
                    let Some(k) = kanal.as_mut() else {
                        senden(Meldung::MusikGesendet(Err("Nicht verbunden".into())));
                        continue;
                    };
                    let mut r = crate::musik::abspielen(k.as_mut(), &befehle, &abbruch);
                    if kanal_weg == Weg::Usb && r.as_ref().is_err_and(|e| geraet::ist_verbindungsfehler(e)) {
                        r = if usb_neu(&mut kanal, &senden) {
                            crate::musik::abspielen(kanal.as_mut().unwrap().as_mut(), &befehle, &abbruch)
                        } else {
                            Err(VERBINDUNG_WEG.into())
                        };
                    }
                    senden(Meldung::MusikGesendet(r));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !live {
                        if let Some(k) = kanal.as_mut() {
                            k.ablesen(); /* Port offen halten heißt auch abholen, sonst hängt die Firmware beim Senden */
                        }
                    }
                    if letzte_pruefung.elapsed() >= Duration::from_secs(2) {
                        letzte_pruefung = Instant::now();
                        if usb_weg(&kanal, kanal_weg) || (kanal_weg == Weg::Usb && kanal.as_deref().is_some_and(usb_anderer)) {
                            kanal = None;
                            senden(Meldung::Getrennt(Some("Controller nicht mehr am USB".into())));
                        } else if kanal_weg == Weg::Usb && kanal.as_deref().is_some_and(usb_gewechselt) && !usb_neu(&mut kanal, &senden) {
                            senden(Meldung::Getrennt(Some(VERBINDUNG_WEG.into())));
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if live && letzter_status.elapsed() >= Duration::from_millis(30) {
                if let Some(k) = kanal.as_mut() {
                    letzter_status = Instant::now();
                    match k.befehl("status", &["status:"], Duration::from_millis(600)) {
                        Ok(z) => {
                            fehler_in_folge = 0;
                            if let Some(s) = Status::lesen(&z) {
                                senden(Meldung::Status(s));
                            }
                        }
                        Err(_) => {
                            fehler_in_folge += 1;
                            if fehler_in_folge >= 4 {
                                kanal = None;
                                senden(Meldung::Getrennt(Some("Verbindung zum Controller verloren".into())));
                            }
                        }
                    }
                }
            }
        }
    });
}

fn verbinden(weg: Weg) -> Result<Box<dyn Kanal>, String> {
    match weg {
        Weg::Usb => {
            let pfad = geraet::finden().ok_or(
                "Kein Controller am USB gefunden. Kabel stecken und Schieber auf PC stellen (im Switch-Modus gibt es \
                 über USB keinen Befehlskanal, dann Bluetooth nutzen).",
            )?;
            let g = Geraet::oeffnen(&pfad).map_err(|e| {
                if cfg!(target_os = "linux") {
                    crate::sprache::tr("{}. Tipp: Benutzer in die Gruppe dialout aufnehmen und neu anmelden.", &[&e])
                } else {
                    e
                }
            })?;
            // Verbundenes Gerät merken: steckt später ein zweiter Controller an, bleibt die Verbindung bei diesem
            // (sonst hielte die Gerätewechsel-Prüfung den neuen, in der Liste vorderen für einen Wechsel)
            geraet::waehlen(Some(geraet::serie(&pfad)));
            Ok(Box::new(g))
        }
        Weg::Bluetooth => Ok(Box::new(crate::ble::BleGeraet::verbinden(None)?)),
    }
}

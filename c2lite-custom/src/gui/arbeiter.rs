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
}

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
                        Ok(k) => {
                            let name = k.name();
                            kanal = Some(k);
                            kanal_weg = weg;
                            fehler_in_folge = 0;
                            senden(Meldung::Verbunden(weg, name));
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
                    let Some(k) = kanal.as_mut() else {
                        senden(Meldung::Antwort(zweck, Err("Nicht verbunden".into())));
                        continue;
                    };
                    let mut alle = Vec::new();
                    let mut ergebnis = Ok(());
                    for b in &befehle {
                        match k.befehl_alle(b, PRAEFIXE, Duration::from_secs(3)) {
                            Ok(z) => alle.extend(z),
                            Err(e) => {
                                ergebnis = Err(e);
                                break;
                            }
                        }
                    }
                    let fehlgeschlagen = ergebnis.is_err();
                    senden(Meldung::Antwort(zweck, ergebnis.map(|_| alle)));
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
                Err(RecvTimeoutError::Timeout) => {
                    if letzte_pruefung.elapsed() >= Duration::from_secs(2) {
                        letzte_pruefung = Instant::now();
                        if usb_weg(&kanal, kanal_weg) {
                            kanal = None;
                            senden(Meldung::Getrennt(Some("Controller nicht mehr am USB".into())));
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
            Ok(Box::new(Geraet::oeffnen(&pfad).map_err(|e| {
                if cfg!(target_os = "linux") {
                    crate::sprache::tr("{}. Tipp: Benutzer in die Gruppe dialout aufnehmen und neu anmelden.", &[&e])
                } else {
                    e
                }
            })?))
        }
        Weg::Bluetooth => Ok(Box::new(crate::ble::BleGeraet::verbinden(None)?)),
    }
}

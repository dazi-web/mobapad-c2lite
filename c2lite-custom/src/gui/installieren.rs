//! Seite „Installieren“: Erstinstallation von C2 Lite Custom auf einer C2 Lite mit Werks-Firmware (crate::installieren), geführt
//! für Laien, und der Rückweg zur Werks-Firmware. Der Lauf passiert in einem eigenen Thread. Solange er läuft, steht oben eine
//! große Ablauf-Anzeige (Aufforderung, Bild, Fortschritt) und der Rest der Seite ist ausgeblendet; die Oberfläche holt die
//! Meldungen in jedem Bild ab (mod.rs), auch wenn eine andere Seite offen wäre.

use super::stil::{Farben, farben, hinweis, karte, knopf_haupt, titel};
use super::{App, bild};
use crate::installieren::Meldung;
use crate::sprache::{t, t_meldung};
use eframe::egui::{self, Align2, CornerRadius, FontId, Margin, Pos2, RichText, Stroke, Vec2};
use egui_phosphor::regular as icon;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum Auftrag {
    Probelauf,
    Installieren,
    WerkZurueck,
}

/// Weg zur Erstinstallation
#[derive(Clone, Copy, PartialEq, Default)]
enum Weg {
    /// Werks-Firmware am Kabel, Home + Y + X (docs/WERKS-DOWNLOAD.md)
    #[default]
    Usb,
    /// Bluetooth-Brücke über den offiziellen Update-Weg
    Bluetooth,
}

pub enum Ereignis {
    Meldung(Meldung),
    Ende(Result<(), String>),
}

#[derive(Default)]
pub struct Installation {
    weg: Weg,
    /// None = eingebaute Firmware
    firmware: Option<PathBuf>,
    rx: Option<Receiver<Ereignis>>,
    auftrag: Option<Auftrag>,
    werk_sicherung: Option<PathBuf>,
    /// Sicherung selbst gewählt: dann nicht automatisch durch eine neuere ersetzen
    sicherung_gewaehlt: bool,
    schritte: Vec<String>,
    hinweis: Option<String>,
    fortschritt: Option<f32>,
    ergebnis: Option<Result<String, String>>,
    start: Option<Instant>,
    ende: Option<Instant>,
}

impl Installation {
    pub fn laeuft(&self) -> bool {
        self.rx.is_some()
    }

    /// Vorführmodus für Screenshots (C2LITE_DEMO=tasten|uebertragung|fertig): zeigt die Ablauf-Anzeige, ohne dass
    /// etwas am Gerät passiert
    pub fn vorfuehren(&mut self, art: &str) {
        self.auftrag = Some(Auftrag::Installieren);
        self.start = Some(Instant::now());
        self.schritte = vec!["Controller prüfen".into(), "Lader laden".into(), "Controller am USB suchen".into(), "USB-Verkehr starten".into()];
        match art {
            "fertig" => {
                self.schritte.extend(["Sicherung des ganzen Speichers lesen", "C2 Lite Custom schreiben", "Prüfen"].map(String::from));
                self.ergebnis = Some(Ok("C2 Lite Custom ist installiert und geprüft. Der Controller startet neu.".into()));
                self.ende = Some(Instant::now());
                self.schritte = self.schritte.iter().map(|s| t(s).to_string()).collect();
                self.ergebnis = self.ergebnis.take().map(|e| e.map(|x| t(&x).to_string()));
                return;
            }
            "uebertragung" => {
                self.schritte.extend(["Warte auf den Download-Modus", "Sicherung des ganzen Speichers lesen", "C2 Lite Custom schreiben"].map(String::from));
                self.fortschritt = Some(0.62);
            }
            _ => {
                self.schritte.push("Warte auf den Download-Modus".into());
                self.hinweis = Some("Home + Y + X halten, bis das Brummen aufhört.".into());
            }
        }
        self.schritte = self.schritte.iter().map(|s| t(s).to_string()).collect();
        self.hinweis = self.hinweis.take().map(|h| t(&h).to_string());
        self.ergebnis = self.ergebnis.take().map(|e| e.map(|x| t(&x).to_string()));
        // Läuft scheinbar: ein Kanal, dessen Sender nie endet (kein Thread, kein Gerät)
        let (tx, rx) = mpsc::channel::<Ereignis>();
        std::mem::forget(tx);
        self.rx = Some(rx);
    }

    /// Letzter Lauf war ein erfolgreicher Rückweg (für den Text „Werks-Firmware läuft wieder“)
    pub fn zurueck_erfolgreich(&self) -> bool {
        !self.laeuft() && self.auftrag == Some(Auftrag::WerkZurueck) && matches!(self.ergebnis, Some(Ok(_)))
    }

    fn starten(&mut self, ctx: &egui::Context, auftrag: Auftrag) {
        let sicherung = self.werk_sicherung.clone();
        if auftrag == Auftrag::WerkZurueck && sicherung.is_none() {
            return;
        }
        let firmware = self.firmware.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.auftrag = Some(auftrag);
        self.schritte.clear();
        self.hinweis = None;
        self.fortschritt = None;
        self.ergebnis = None;
        self.start = Some(Instant::now());
        self.ende = None;
        let ctx = ctx.clone();
        let weg = self.weg;
        std::thread::spawn(move || {
            let senden = |e: Ereignis| {
                let _ = tx.send(e);
                ctx.request_repaint();
            };
            let mut melden = |m| senden(Ereignis::Meldung(m));
            let r = match auftrag {
                Auftrag::WerkZurueck => match &sicherung {
                    Some(s) => crate::installieren::werk_zurueck(s, &mut melden),
                    None => Ok(()),
                },
                a if weg == Weg::Usb => crate::installieren::starten_usb(firmware.as_deref(), a == Auftrag::Probelauf, &mut melden),
                a => crate::installieren::starten(firmware.as_deref(), a == Auftrag::Probelauf, &mut melden),
            };
            senden(Ereignis::Ende(r.map_err(|e| e.to_string())));
        });
    }

    /// Meldungen übernehmen; liefert beim Ende den Auftrag und ob er geklappt hat
    fn abholen(&mut self) -> Option<(Auftrag, bool)> {
        let rx = self.rx.as_ref()?;
        let mut ende = None;
        loop {
            match rx.try_recv() {
                Ok(Ereignis::Meldung(Meldung::Schritt(t))) => {
                    self.schritte.push(t);
                    self.fortschritt = None;
                    // Eine Aufforderung gilt nur für ihren Schritt (sonst stünde „Home + Y + X“ noch beim Schreiben da)
                    self.hinweis = None;
                }
                Ok(Ereignis::Meldung(Meldung::Hinweis(t))) => self.hinweis = Some(t),
                Ok(Ereignis::Meldung(Meldung::Fortschritt(f))) => self.fortschritt = Some(f),
                Ok(Ereignis::Meldung(Meldung::Fertig(t))) => self.ergebnis = Some(Ok(t)),
                Ok(Ereignis::Ende(r)) => {
                    // Fehler aus dem Hauptprozess (Kernzeilen sind schon übersetzt; übersetzt Gebliebenes bleibt)
                    ende = Some(r.map_err(|e| t_meldung(&e)));
                    break;
                }
                Err(TryRecvError::Empty) => break,
                // Thread ohne „Ende“ beendet (Absturz): nicht ewig „läuft“ anzeigen
                Err(TryRecvError::Disconnected) => {
                    ende = Some(Err(t("Abgebrochen (interner Fehler, siehe Protokoll in ~/c2lite-sicherung)").into()));
                    break;
                }
            }
        }
        let r = ende?;
        self.rx = None;
        self.fortschritt = None;
        self.hinweis = None;
        self.ende = Some(Instant::now());
        let ok = r.is_ok();
        if let Err(e) = r {
            self.ergebnis = Some(Err(e));
        }
        Some((self.auftrag?, ok))
    }
}

impl App {
    /// Jedes Bild aus `ui()` aufgerufen: Meldungen abholen, Folgen des Endes (neu verbinden, neueste Sicherung)
    pub(super) fn installation_abholen(&mut self, ctx: &egui::Context) {
        if self.installation.laeuft() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33)); // Animationen
        } else if self.installation.ende.is_some_and(|e| e.elapsed().as_secs_f32() < 2.0) {
            ctx.request_repaint_after(std::time::Duration::from_millis(33)); // Erfolgs-Effekt
        }
        let Some((auftrag, ok)) = self.installation.abholen() else { return };
        if ok && auftrag == Auftrag::Installieren {
            self.spaeter_verbinden(6);
        }
        if ok && auftrag != Auftrag::WerkZurueck && !self.installation.sicherung_gewaehlt {
            self.installation.werk_sicherung = crate::installieren::neueste_werk_sicherung();
        }
    }

    pub(super) fn installieren_seite(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.kopf(ui, "Installieren", "C2 Lite Custom auf einen Controller mit Werks-Firmware bringen.");

        let inst = &self.installation;
        if inst.laeuft() || inst.ergebnis.is_some() {
            self.lauf_anzeige(ui, c);
            if self.installation.laeuft() {
                return;
            }
            ui.add_space(16.0);
        }

        if self.installation.werk_sicherung.as_ref().is_none_or(|p| !p.exists()) {
            self.installation.werk_sicherung = crate::installieren::neueste_werk_sicherung();
            self.installation.sicherung_gewaehlt = false;
        }

        karte(ui, |ui| {
            // Weg als zwei große Kacheln
            ui.horizontal(|ui| {
                let breite = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(160.0);
                for (weg, ic, name, unter) in [
                    (Weg::Usb, icon::USB, "USB-Kabel", "etwa 1 Minute · empfohlen"),
                    (Weg::Bluetooth, icon::BLUETOOTH, "Bluetooth", "etwa 6 Minuten"),
                ] {
                    let gewaehlt = self.installation.weg == weg;
                    let (rect, antwort) = ui.allocate_exact_size(Vec2::new(breite, 64.0), egui::Sense::click());
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius::same(10), if gewaehlt { c.akzent_schwach } else { c.flaeche });
                    p.rect_stroke(
                        rect,
                        CornerRadius::same(10),
                        Stroke::new(if gewaehlt { 2.0 } else { 1.0 }, if gewaehlt { c.akzent } else { c.linie }),
                        egui::StrokeKind::Inside,
                    );
                    let farbe = if gewaehlt { c.akzent } else { c.text };
                    p.text(rect.left_center() + Vec2::new(18.0, 0.0), Align2::LEFT_CENTER, ic, FontId::proportional(26.0), farbe);
                    p.text(rect.left_center() + Vec2::new(56.0, -9.0), Align2::LEFT_CENTER, t(name), FontId::proportional(17.0), farbe);
                    p.text(rect.left_center() + Vec2::new(56.0, 12.0), Align2::LEFT_CENTER, t(unter), FontId::proportional(13.0), c.schwach);
                    if antwort.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.installation.weg = weg;
                    }
                }
            });
            ui.add_space(14.0);
            let schritte: &[(&str, &str)] = match self.installation.weg {
                Weg::Usb => &[
                    (icon::POWER, "Ohne Kabel einschalten, dann Kabel anstecken"),
                    (icon::VIBRATE, "Wenn er brummt: Home + Y + X halten"),
                    (icon::CHECK_CIRCLE, "Der Rest läuft von selbst"),
                ],
                Weg::Bluetooth => &[
                    (icon::BLUETOOTH, "Ohne Kabel in den Kopplungsmodus"),
                    (icon::HOURGLASS, "Etwa 5 Minuten liegen lassen"),
                    (icon::PLUG, "Auf Aufforderung Kabel anstecken"),
                ],
            };
            for (n, (ic, text)) in schritte.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}", n + 1)).strong().color(c.akzent));
                    ui.label(RichText::new(*ic).size(18.0).color(c.schwach));
                    ui.label(t(text));
                });
            }
            ui.add_space(12.0);

            // Firmware in einer Zeile
            ui.horizontal(|ui| {
                match &self.installation.firmware {
                    None => {
                        ui.label(RichText::new(t("Firmware:")).color(c.schwach));
                        ui.label(t_meldung(crate::installieren::eingebaut_stand()));
                        if ui.small_button(t("Andere Datei …")).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter(t("Firmware-Abbild"), &["bin"]).pick_file() {
                                self.installation.firmware = Some(p);
                            }
                        }
                    }
                    Some(p) => {
                        ui.label(RichText::new(t("Firmware:")).color(c.schwach));
                        ui.label(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                        if ui.small_button(t("Eingebaute nehmen")).clicked() {
                            self.installation.firmware = None;
                        }
                    }
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let ctx = ui.ctx().clone();
                if ui
                    .button(format!("{}  {}", icon::MAGNIFYING_GLASS, t("Probelauf")))
                    .on_hover_text(t("Prüft alles und sichert den Speicher. Schreibt nichts."))
                    .clicked()
                {
                    self.installation.starten(&ctx, Auftrag::Probelauf);
                }
                if knopf_haupt(ui, &format!("{}  {}", icon::DOWNLOAD_SIMPLE, t("Installieren"))).clicked() {
                    self.installation.starten(&ctx, Auftrag::Installieren);
                }
            });
            ui.add_space(6.0);
            egui::CollapsingHeader::new(RichText::new(t("Was man braucht")).color(c.schwach)).id_salt("braucht").show(ui, |ui| {
                hinweis(
                    ui,
                    match self.installation.weg {
                        Weg::Usb => "Linux, ein USB-Datenkabel, beim ersten Mal Internet (Lader) und dein Passwort.",
                        Weg::Bluetooth => {
                            "Linux, Bluetooth am Rechner, ein USB-Datenkabel, beim ersten Mal Internet und dein Passwort. \
                             Andere Bluetooth-Geräte sind kurz getrennt."
                        }
                    },
                );
            });
        });

        ui.add_space(12.0);
        // Rückweg: selten gebraucht, darum eingeklappt
        karte(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new(format!("{}  {}", icon::ARROW_COUNTER_CLOCKWISE, t("Zurück zur Werks-Firmware"))).strong())
                .id_salt("rueckweg")
                .show(ui, |ui| {
                    hinweis(ui, "Spielt die Sicherung von der Installation zurück. Controller mit C2 Lite Custom per Kabel anstecken.");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        match &self.installation.werk_sicherung {
                            Some(p) => ui.label(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                            None => ui.label(RichText::new(t("keine Sicherung gefunden")).color(c.schwach)),
                        };
                        if ui.small_button(t("Andere …")).clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_directory(crate::installieren::sicherungsordner())
                                .add_filter(t("Werks-Sicherung"), &["bin"])
                                .pick_file()
                            {
                                self.installation.werk_sicherung = Some(p);
                                self.installation.sicherung_gewaehlt = true;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    // Nicht, solange der eigene Arbeiter den Port noch für ein Update oder das Lauschen braucht
                    let beschaeftigt = self.update_fortschritt.is_some() || self.weck_lauscht.is_some();
                    let bereit = self.installation.werk_sicherung.is_some() && !beschaeftigt;
                    let ctx = ui.ctx().clone();
                    if ui.add_enabled(bereit, egui::Button::new(t("Werks-Firmware einspielen"))).clicked() {
                        // Die Oberfläche hält den seriellen Port exklusiv: freigeben, damit der Kern den Controller erreicht
                        let _ = self.tx.send(super::arbeiter::Auftrag::Trennen);
                        self.neu_verbinden_ab = None;
                        self.neu_verbinden_versuche = 0;
                        self.installation.starten(&ctx, Auftrag::WerkZurueck);
                    }
                    if beschaeftigt {
                        hinweis(ui, "Erst das laufende Update bzw. „Wecken einrichten“ abwarten.");
                    }
                });
        });
    }

    /// Große Ablauf-Anzeige oben: Bild (leuchtende Tasten bzw. Übertragung), Aufforderung, Schritte, Ergebnis
    fn lauf_anzeige(&mut self, ui: &mut egui::Ui, c: Farben) {
        let inst = &self.installation;
        let laeuft = inst.laeuft();
        let zeit = ui.input(|i| i.time);
        karte(ui, |ui| {
            ui.horizontal(|ui| {
                titel(
                    ui,
                    match inst.auftrag {
                        Some(Auftrag::Probelauf) => "Probelauf",
                        Some(Auftrag::WerkZurueck) => "Zurück zur Werks-Firmware",
                        _ => "Installation",
                    },
                );
                if let (true, Some(s)) = (laeuft, inst.start) {
                    let sek = s.elapsed().as_secs();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}:{:02}", sek / 60, sek % 60)).color(c.schwach).monospace());
                    });
                }
            });
            ui.add_space(6.0);

            let tasten = inst.hinweis.as_deref().is_some_and(|h| h.contains("Home + Y + X"));
            if laeuft {
                ui.vertical_centered(|ui| {
                    if tasten {
                        bild::kombination(ui, bild::HOME_Y_X, zeit, c);
                    } else {
                        uebertragung(ui, inst.fortschritt, zeit, c, inst.auftrag == Some(Auftrag::WerkZurueck));
                    }
                });
            } else if let Some(e) = &inst.ergebnis {
                ui.vertical_centered(|ui| ergebnis_bild(ui, e.is_ok(), inst.ende, c));
            }

            if let (true, Some(h)) = (laeuft, &inst.hinweis) {
                ui.add_space(8.0);
                egui::Frame::new()
                    .fill(c.erhoeht)
                    .stroke(Stroke::new(2.0, c.warnung))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{}  {h}", icon::HAND_POINTING)).size(19.0).strong().color(c.warnung));
                    });
            }

            ui.add_space(8.0);
            let n = inst.schritte.len();
            for (i, s) in inst.schritte.iter().enumerate() {
                let aktuell = laeuft && i + 1 == n;
                ui.horizontal(|ui| {
                    if aktuell {
                        ui.spinner();
                        ui.label(RichText::new(s).strong());
                    } else {
                        ui.label(RichText::new(icon::CHECK).color(c.akzent));
                        ui.label(RichText::new(s).color(c.schwach));
                    }
                });
            }
            if laeuft {
                ui.add_space(6.0);
                hinweis(ui, "Kabel und Controller nur bei einer Aufforderung anfassen.");
            }
            if let Some(e) = &inst.ergebnis {
                ui.add_space(8.0);
                let (farbe, ic, text) = match e {
                    Ok(x) => (c.akzent, icon::CHECK_CIRCLE, x.as_str()),
                    Err(x) => (c.fehler, icon::WARNING_CIRCLE, x.as_str()),
                };
                ui.label(RichText::new(format!("{ic}  {text}")).size(16.0).color(farbe));
            }
        });
    }
}

/// Laptop links, Controller rechts, dazwischen fließende Punkte; in der Mitte ein Fortschrittsring (sonst ein kreisender Bogen)
fn uebertragung(ui: &mut egui::Ui, fortschritt: Option<f32>, zeit: f64, c: Farben, zurueck: bool) {
    let breite = ui.available_width().min(520.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(breite, 150.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let mitte = rect.center();
    let links = Pos2::new(rect.left() + 50.0, mitte.y);
    let rechts = Pos2::new(rect.right() - 50.0, mitte.y);
    p.text(links, Align2::CENTER_CENTER, icon::LAPTOP, FontId::proportional(54.0), c.text);
    p.text(rechts, Align2::CENTER_CENTER, icon::GAME_CONTROLLER, FontId::proportional(54.0), c.akzent);

    // Punkte wandern vom Laptop zum Controller, außer am Ring vorbei; vorne heller
    let von = links.x + 44.0;
    let bis = rechts.x - 44.0;
    for i in 0..14 {
        let t = ((zeit * 0.55 + i as f64 / 14.0).fract()) as f32;
        let x = von + (bis - von) * t;
        if (x - mitte.x).abs() < 46.0 {
            continue;
        }
        let a = (std::f32::consts::PI * t).sin();
        p.circle_filled(Pos2::new(x, mitte.y), 2.5 + 1.5 * a, c.akzent.gamma_multiply(0.25 + 0.75 * a));
    }

    // Ring in der Mitte
    let r = 34.0;
    p.circle_stroke(mitte, r, Stroke::new(6.0, c.linie));
    let (anfang, laenge) = match fortschritt {
        Some(f) => (-std::f32::consts::FRAC_PI_2, f.clamp(0.0, 1.0) * std::f32::consts::TAU),
        None => ((zeit * 3.0) as f32, 1.6),
    };
    if laenge > 0.01 {
        let n = (laenge * 18.0).max(4.0) as usize;
        let punkte: Vec<Pos2> =
            (0..=n).map(|k| anfang + laenge * k as f32 / n as f32).map(|w| mitte + Vec2::new(w.cos(), w.sin()) * r).collect();
        p.add(egui::Shape::line(punkte, Stroke::new(6.0, c.akzent)));
    }
    let mitte_text = match fortschritt {
        Some(f) => format!("{:.0} %", f * 100.0),
        None if zurueck => icon::ARROW_COUNTER_CLOCKWISE.to_string(),
        None => icon::LIGHTNING.to_string(),
    };
    p.text(mitte, Align2::CENTER_CENTER, mitte_text, FontId::proportional(16.0), c.text);
}

/// Erfolg: Haken mit sich ausbreitendem Ring (1,5 s); Fehler: ruhiges Warnzeichen
fn ergebnis_bild(ui: &mut egui::Ui, ok: bool, ende: Option<Instant>, c: Farben) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(520.0), 110.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let mitte = rect.center();
    if !ok {
        p.text(mitte, Align2::CENTER_CENTER, icon::WARNING_CIRCLE, FontId::proportional(64.0), c.fehler);
        return;
    }
    let t = ende.map_or(1.0, |e| (e.elapsed().as_secs_f32() / 1.5).min(1.0));
    // Ring breitet sich aus und verblasst, der Haken wächst kurz über seine Größe hinaus
    for (versatz, staerke) in [(0.0, 3.0), (0.18, 2.0)] {
        let s = ((t - versatz) / (1.0 - versatz)).clamp(0.0, 1.0);
        if s > 0.0 && s < 1.0 {
            p.circle_stroke(mitte, 30.0 + 40.0 * s, Stroke::new(staerke, c.akzent.gamma_multiply(1.0 - s)));
        }
    }
    let wachsen = if t < 0.35 { 0.6 + 0.6 * (t / 0.35) } else { 1.2 - 0.2 * ((t - 0.35) / 0.65).min(1.0) };
    p.circle_filled(mitte, 30.0 * wachsen, c.akzent);
    p.text(mitte, Align2::CENTER_CENTER, icon::CHECK, FontId::proportional(34.0 * wachsen), c.akzent_text);
}

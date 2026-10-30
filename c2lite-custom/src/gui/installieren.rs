//! Seite „Installieren“: Erstinstallation von C2 Lite Custom auf einer C2 Lite mit Werks-Firmware (crate::installieren), geführt
//! für Laien. Läuft in einem eigenen Thread; die Seite zeigt den aktuellen Schritt, Aufforderungen groß und den
//! Fortschritt. Funktioniert ohne verbundenen Controller. Dazu der Rückweg zur Werks-Firmware aus der Sicherung.

use super::stil::{farben, hinweis, karte, knopf_haupt, titel};
use super::App;
use crate::installieren::Meldung;
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};
use egui_phosphor::regular as icon;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Copy, PartialEq)]
enum Auftrag {
    Probelauf,
    Installieren,
    WerkZurueck,
}

pub enum Ereignis {
    Meldung(Meldung),
    Ende(Result<(), String>),
}

#[derive(Default)]
pub struct Installation {
    firmware: Option<PathBuf>,
    rx: Option<Receiver<Ereignis>>,
    auftrag: Option<Auftrag>,
    werk_sicherung: Option<PathBuf>,
    schritte: Vec<String>,
    hinweis: Option<String>,
    fortschritt: Option<f32>,
    ergebnis: Option<Result<String, String>>,
}

impl Installation {
    fn laeuft(&self) -> bool {
        self.rx.is_some()
    }

    fn starten(&mut self, ctx: &egui::Context, auftrag: Auftrag) {
        let datei = match auftrag {
            Auftrag::WerkZurueck => self.werk_sicherung.clone(),
            _ => self.firmware.clone(),
        };
        let Some(datei) = datei else { return };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.auftrag = Some(auftrag);
        self.schritte.clear();
        self.hinweis = None;
        self.fortschritt = None;
        self.ergebnis = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let senden = |e: Ereignis| {
                let _ = tx.send(e);
                ctx.request_repaint();
            };
            let mut melden = |m| senden(Ereignis::Meldung(m));
            let r = match auftrag {
                Auftrag::WerkZurueck => crate::installieren::werk_zurueck(&datei, &mut melden),
                a => crate::installieren::starten(&datei, a == Auftrag::Probelauf, &mut melden),
            };
            senden(Ereignis::Ende(r.map_err(|e| e.to_string())));
        });
    }

    fn abholen(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut ende = None;
        while let Ok(e) = rx.try_recv() {
            match e {
                Ereignis::Meldung(Meldung::Schritt(t)) => {
                    self.schritte.push(t);
                    self.fortschritt = None;
                }
                Ereignis::Meldung(Meldung::Hinweis(t)) => self.hinweis = Some(t),
                Ereignis::Meldung(Meldung::Fortschritt(f)) => self.fortschritt = Some(f),
                Ereignis::Meldung(Meldung::Fertig(t)) => self.ergebnis = Some(Ok(t)),
                Ereignis::Ende(r) => ende = Some(r),
            }
        }
        if let Some(r) = ende {
            self.rx = None;
            self.fortschritt = None;
            if let Err(e) = r {
                self.ergebnis = Some(Err(e));
            }
        }
    }
}

impl App {
    pub(super) fn installieren_seite(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.installation.abholen();
        if self.installation.laeuft() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        self.kopf(
            ui,
            "Installieren",
            "C2 Lite Custom auf einen Controller mit Werks-Firmware bringen, ohne Zusatzhardware. Danach gehen Updates über die Seite „Firmware“.",
        );

        karte(ui, |ui| {
            titel(ui, "So läuft es");
            ui.add_space(4.0);
            for (n, t) in [
                ("1", "Erst den Probelauf machen: Er prüft alles und koppelt einmal, ohne etwas zu verändern."),
                ("2", "Controller ohne Kabel einschalten und in den Kopplungsmodus bringen."),
                ("3", "Das Programm überträgt per Bluetooth eine Brücke (4 bis 6 Minuten). Controller liegen lassen."),
                ("4", "Wenn es dazu auffordert: USB-Kabel vom Rechner an den Controller stecken."),
                ("5", "Sicherung des Werks-Speichers, dann wird C2 Lite Custom geschrieben und geprüft (unter einer Minute)."),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(n).strong().color(c.akzent));
                    ui.label(t);
                });
            }
            ui.add_space(4.0);
            hinweis(
                ui,
                "Braucht: Linux, Bluetooth am Rechner, ein USB-Datenkabel, einmalig Internet (Werks-Firmware und Lader \
                 werden geprüft heruntergeladen) und dein Passwort (für Bluetooth-Adapter und Download-Modus). \
                 Andere Bluetooth-Geräte am Rechner sind während der Übertragung kurz getrennt.",
            );
        });
        ui.add_space(12.0);

        let laeuft = self.installation.laeuft();
        karte(ui, |ui| {
            titel(ui, "Firmware C2 Lite Custom");
            hinweis(ui, "Die Datei jl_isd.bin aus dem Firmware-Paket.");
            ui.add_space(4.0);
            ui.add_enabled_ui(!laeuft, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(format!("{}  Datei wählen", icon::FLOPPY_DISK)).clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Firmware-Abbild", &["bin"]).pick_file() {
                            self.installation.firmware = Some(p);
                        }
                    }
                    if let Some(p) = &self.installation.firmware {
                        ui.label(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let bereit = self.installation.firmware.is_some();
                    let ctx = ui.ctx().clone();
                    if ui.add_enabled(bereit, egui::Button::new(format!("{}  Probelauf", icon::MAGNIFYING_GLASS))).clicked() {
                        self.installation.starten(&ctx, Auftrag::Probelauf);
                    }
                    ui.add_enabled_ui(bereit, |ui| {
                        if knopf_haupt(ui, &format!("{}  Installieren", icon::DOWNLOAD_SIMPLE)).clicked() {
                            self.installation.starten(&ctx, Auftrag::Installieren);
                        }
                    });
                });
            });
        });

        ui.add_space(12.0);
        if self.installation.werk_sicherung.is_none() {
            self.installation.werk_sicherung = crate::installieren::neueste_werk_sicherung();
        }
        karte(ui, |ui| {
            titel(ui, "Zurück zur Werks-Firmware");
            hinweis(
                ui,
                "Spielt die Sicherung ein, die beim Installieren angelegt wurde: Werks-Firmware, Mobapad-App und die \
                 Kopplungen von vorher gehen wieder. Controller mit C2 Lite Custom per USB-Kabel anstecken. Der aktuelle Stand wird \
                 vorher gesichert. Die Sicherung passt nur zu dem Controller, von dem sie stammt; das wird geprüft.",
            );
            ui.add_space(4.0);
            ui.add_enabled_ui(!laeuft, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(format!("{}  Sicherung wählen", icon::FOLDER_OPEN)).clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .set_directory(crate::installieren::sicherungsordner())
                            .add_filter("Werks-Sicherung", &["bin"])
                            .pick_file()
                        {
                            self.installation.werk_sicherung = Some(p);
                        }
                    }
                    match &self.installation.werk_sicherung {
                        Some(p) => ui.label(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                        None => ui.label(RichText::new("keine Sicherung in ~/c2lite-sicherung").color(c.schwach)),
                    };
                });
                ui.add_space(8.0);
                let ctx = ui.ctx().clone();
                let bereit = self.installation.werk_sicherung.is_some();
                if ui.add_enabled(bereit, egui::Button::new(format!("{}  Werks-Firmware einspielen", icon::ARROW_COUNTER_CLOCKWISE))).clicked() {
                    // Die Oberfläche hält den seriellen Port exklusiv: freigeben, damit der Kern den Controller erreicht
                    let _ = self.tx.send(super::arbeiter::Auftrag::Trennen);
                    self.neu_verbinden_ab = None;
                    self.installation.starten(&ctx, Auftrag::WerkZurueck);
                }
            });
        });

        let inst = &self.installation;
        if laeuft || !inst.schritte.is_empty() {
            ui.add_space(12.0);
            if let Some(h) = &inst.hinweis {
                if laeuft {
                    egui::Frame::new()
                        .fill(c.erhoeht)
                        .stroke(Stroke::new(2.0, c.warnung))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(format!("{}  {h}", icon::HAND_POINTING)).size(19.0).strong().color(c.warnung));
                        });
                    ui.add_space(12.0);
                }
            }
            karte(ui, |ui| {
                titel(
                    ui,
                    match inst.auftrag {
                        Some(Auftrag::Probelauf) => "Probelauf",
                        Some(Auftrag::WerkZurueck) => "Zurück zur Werks-Firmware",
                        _ => "Installation",
                    },
                );
                ui.add_space(4.0);
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
                if let Some(f) = inst.fortschritt {
                    ui.add_space(6.0);
                    ui.add(egui::ProgressBar::new(f).show_percentage().fill(c.akzent));
                }
                if laeuft {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Controller und Kabel nicht anfassen, außer das Programm fordert dazu auf.").color(c.schwach));
                }
            });
            if let Some(e) = &inst.ergebnis {
                ui.add_space(12.0);
                let (farbe, ic, text) = match e {
                    Ok(t) => (c.akzent, icon::CHECK_CIRCLE, t.as_str()),
                    Err(t) => (c.fehler, icon::WARNING_CIRCLE, t.as_str()),
                };
                egui::Frame::new()
                    .fill(c.erhoeht)
                    .stroke(Stroke::new(1.5, farbe))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{ic}  {text}")).size(16.0).color(farbe));
                    });
            }
        }
    }
}

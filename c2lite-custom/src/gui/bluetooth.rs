//! Karte „Bluetooth am PC“ auf der Seite Tastentest: Ist der C2 Lite per Bluetooth als Gamepad (Xbox Wireless Controller)
//! verbunden, und kommt die Vibration über diesen Weg an? Der Test geht über evdev wie ein Spiel, nicht über den
//! Befehlskanal.

use super::App;
use crate::sprache::{t, t_meldung, tr};
use super::stil::{farben, hinweis, karte, titel};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct BtPc {
    gamepad: Option<crate::bt_pc::BtGamepad>,
    geprueft: Option<Instant>,
    test: Option<mpsc::Receiver<Result<(), String>>>,
}

impl App {
    pub(super) fn bluetooth_karte(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        // Alle 2 s neu nachsehen (sysfs, billig); während eines Tests nicht
        if self.bt_pc.test.is_none() && self.bt_pc.geprueft.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.bt_pc.gamepad = crate::bt_pc::finden();
            self.bt_pc.geprueft = Some(Instant::now());
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        if let Some(rx) = &self.bt_pc.test {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.bt_pc.test = None;
                    self.melden("Vibration über Bluetooth gesendet: erst links, dann rechts.", false);
                }
                Ok(Err(e)) => {
                    self.bt_pc.test = None;
                    self.melden(tr("Vibration über Bluetooth: {}", &[&t_meldung(&e)]), true);
                }
                Err(mpsc::TryRecvError::Disconnected) => self.bt_pc.test = None,
                Err(mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(Duration::from_millis(200)),
            }
        }

        karte(ui, |ui| {
            titel(ui, "Bluetooth am PC");
            match self.bt_pc.gamepad.clone() {
                None => {
                    hinweis(
                        ui,
                        "Nicht per Bluetooth verbunden. Koppeln: Schieber auf PC, in den Bluetooth-Einstellungen „C2 Lite dazi“ wählen. \
                         Nach dem Update auf Stufe 13 einmal neu koppeln.",
                    );
                }
                Some(g) => {
                    let (ic, farbe, text) = if g.vibration {
                        (icon::CHECK_CIRCLE, c.akzent, "Verbunden als Xbox-Controller, Vibration möglich")
                    } else {
                        (icon::WARNING_CIRCLE, c.warnung, "Verbunden, aber ohne Vibration (Treiber prüfen)")
                    };
                    ui.label(RichText::new(format!("{ic}  {}", t(text))).color(farbe));
                    hinweis(ui, &tr("Treiber: {}", &[&if g.treiber.is_empty() { "?" } else { &g.treiber }]));
                    ui.add_space(6.0);
                    let laeuft = self.bt_pc.test.is_some();
                    ui.add_enabled_ui(g.vibration && !laeuft, |ui| {
                        if ui.button(format!("{}  {}", icon::VIBRATE, t("Vibration über Bluetooth testen"))).clicked() {
                            let (tx, rx) = mpsc::channel();
                            self.bt_pc.test = Some(rx);
                            std::thread::spawn(move || {
                                let _ = tx.send(crate::bt_pc::vibration_testen(&g));
                            });
                        }
                    });
                }
            }
        });
    }
}

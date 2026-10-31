//! Oberfläche von c2lite-custom (eframe/egui): ohne Befehl gestartet öffnet sich dieses Fenster.
//! Alle Geräte-Zugriffe laufen im Arbeiter-Thread (arbeiter.rs), die Oberfläche schickt nur Aufträge.

mod arbeiter;
mod bild;
mod daten;
#[cfg(target_os = "linux")]
mod installieren;
#[cfg(target_os = "linux")]
mod bluetooth;
mod stil;

use crate::geraet::{Status, TASTEN as ROLLEN};
use arbeiter::{Auftrag, Meldung, Weg, Zweck};
use daten::*;
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Vec2};
use egui_phosphor::regular as icon;
use crate::wecken::Kennung;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use stil::{farben, hinweis, karte, knopf_haupt, titel};

pub fn starten() -> Result<(), String> {
    let optionen = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("C2 Lite")
            .with_app_id("c2lite-custom")
            // Programmsymbol (assets/icon.svg, als PNG eingebettet)
            .with_icon(std::sync::Arc::new(eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")).unwrap_or_default()))
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([880.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("c2lite-custom", optionen, Box::new(|cc| Ok(Box::new(App::neu(cc))))).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Seite {
    Uebersicht,
    Test,
    Belegung,
    Licht,
    Firmware,
    Diagnose,
    Backup,
    Installieren,
}

const SEITEN: &[(Seite, &str, &str)] = &[
    (Seite::Uebersicht, icon::GAUGE, "Übersicht"),
    (Seite::Test, icon::GAME_CONTROLLER, "Tastentest"),
    (Seite::Belegung, icon::STACK, "Belegung & Turbo"),
    (Seite::Licht, icon::LIGHTBULB, "Licht & Vibration"),
    (Seite::Firmware, icon::CPU, "Firmware"),
    (Seite::Diagnose, icon::TERMINAL, "Diagnose"),
    (Seite::Backup, icon::ARCHIVE, "Backup"),
    (Seite::Installieren, icon::DOWNLOAD_SIMPLE, "Installieren"),
];

enum Verbindung {
    Getrennt,
    Sucht(Weg),
    Verbunden(Weg, String),
}

struct Meldungstext {
    text: String,
    fehler: bool,
    seit: Instant,
}

struct App {
    tx: Sender<Auftrag>,
    rx: Receiver<Meldung>,
    seite: Seite,
    verbindung: Verbindung,
    letzter_weg: Weg,
    fehler: Option<String>,
    info: Option<Info>,
    switch_modus: Option<bool>,
    status: Option<Status>,
    getestet: u64,
    live: bool,
    letzte_info: Instant,
    einst: Option<Einstellungen>,
    einst_entwurf: Option<Einstellungen>,
    einst_geaendert: Option<Instant>,
    belegung: Option<Belegung>,
    entwurf: [Platz; 3],
    platz: usize,
    meldung: Option<Meldungstext>,
    update_datei: Option<(PathBuf, Vec<u8>)>,
    update_fortschritt: Option<f32>,
    neu_verbinden_ab: Option<Instant>,
    /// weitere stille Verbindungsversuche nach Neustart, Update oder Installation (der Controller braucht unterschiedlich lange)
    neu_verbinden_versuche: u8,
    /// Platz und Name, deren Speichern noch auf die Antwort wartet (Meldung erst nach Erfolg)
    belegung_speichern: Option<(usize, String)>,
    /// „switch diag“ erneut lesen (nach dem Lernen am Controller)
    neu_laden_ab: Option<Instant>,
    diag_ausgabe: Vec<String>,
    diag_eingabe: String,
    frage_download: bool,
    frage_werkszustand: bool,
    #[cfg(target_os = "linux")]
    installation: installieren::Installation,
    #[cfg(target_os = "linux")]
    bt_pc: bluetooth::BtPc,
    /// Pro Controller 2 aus „pro2 status“: (klassisch, gekoppelt); None = unbekannt (alte Firmware)
    pro2: Option<(bool, bool)>,
    /// Weckkennung aus „switch diag“: None = noch nicht gelesen, Some(None) = nicht eingerichtet
    weck: Option<Option<Kennung>>,
    /// läuft „Wecken einrichten“: Abbruch-Flag und Start
    weck_lauscht: Option<(Arc<AtomicBool>, Instant)>,
}

impl App {
    fn neu(cc: &eframe::CreationContext<'_>) -> App {
        stil::einrichten(&cc.egui_ctx);
        // C2LITE_THEME=hell|dunkel erzwingt ein Theme (sonst folgt es dem System)
        match std::env::var("C2LITE_THEME").as_deref() {
            Ok("hell") => cc.egui_ctx.set_theme(egui::ThemePreference::Light),
            Ok("dunkel") => cc.egui_ctx.set_theme(egui::ThemePreference::Dark),
            _ => {}
        }
        let (tx, arbeiter_rx) = mpsc::channel();
        let (arbeiter_tx, rx) = mpsc::channel();
        arbeiter::starten(cc.egui_ctx.clone(), arbeiter_rx, arbeiter_tx);
        // Beim Start einmal leise über USB versuchen (nicht im Vorführmodus: der zeigt nur Bilder)
        let demo = std::env::var("C2LITE_DEMO").ok();
        if demo.is_none() {
            let _ = tx.send(Auftrag::Verbinden(Weg::Usb));
        }
        #[allow(unused_mut)]
        let mut app = App {
            tx,
            rx,
            // C2LITE_SEITE=test|belegung|licht|firmware|diagnose|backup: direkt auf dieser Seite starten
            seite: match std::env::var("C2LITE_SEITE").as_deref() {
                Ok("test") => Seite::Test,
                Ok("belegung") => Seite::Belegung,
                Ok("licht") => Seite::Licht,
                Ok("firmware") => Seite::Firmware,
                Ok("diagnose") => Seite::Diagnose,
                Ok("backup") => Seite::Backup,
                Ok("installieren") => Seite::Installieren,
                _ => Seite::Uebersicht,
            },
            verbindung: Verbindung::Sucht(Weg::Usb),
            letzter_weg: Weg::Usb,
            fehler: None,
            info: None,
            switch_modus: None,
            status: None,
            getestet: 0,
            live: false,
            letzte_info: Instant::now(),
            einst: None,
            einst_entwurf: None,
            einst_geaendert: None,
            belegung: None,
            entwurf: Default::default(),
            platz: 0,
            meldung: None,
            update_datei: None,
            update_fortschritt: None,
            neu_verbinden_ab: None,
            neu_verbinden_versuche: 0,
            belegung_speichern: None,
            neu_laden_ab: None,
            diag_ausgabe: Vec::new(),
            diag_eingabe: String::new(),
            frage_download: false,
            frage_werkszustand: false,
            #[cfg(target_os = "linux")]
            installation: Default::default(),
            #[cfg(target_os = "linux")]
            bt_pc: Default::default(),
            pro2: None,
            weck: None,
            weck_lauscht: None,
        };
        // C2LITE_DEMO=tasten|uebertragung|fertig: Ablauf-Anzeige der Installation für Screenshots, ohne Gerät
        #[cfg(target_os = "linux")]
        if let Some(d) = demo {
            if d != "seite" {
                app.installation.vorfuehren(&d);
            }
            app.seite = Seite::Installieren;
            app.verbindung = Verbindung::Getrennt;
        }
        app
    }

    fn verbunden(&self) -> bool {
        matches!(self.verbindung, Verbindung::Verbunden(..))
    }

    fn senden(&self, zweck: Zweck, befehle: Vec<String>) {
        let _ = self.tx.send(Auftrag::Befehle(zweck, befehle));
    }

    fn melden(&mut self, text: impl Into<String>, fehler: bool) {
        self.meldung = Some(Meldungstext { text: text.into(), fehler, seit: Instant::now() });
    }

    fn alles_laden(&self) {
        self.senden(Zweck::Info, vec!["info".into()]);
        self.senden(Zweck::SwitchDiag, vec!["switch diag".into()]);
        self.senden(Zweck::Pro2, vec!["pro2 status".into()]);
        self.senden(Zweck::Einstellungen, vec!["einstellungen".into()]);
        self.senden(Zweck::Belegung, vec!["belegung".into()]);
    }

    fn verbinden(&mut self, weg: Weg) {
        self.letzter_weg = weg;
        self.fehler = None;
        let _ = self.tx.send(Auftrag::Verbinden(weg));
    }

    /// Nach Neustart, Update oder Installation: ab `in_s` Sekunden still neu verbinden, bis zu ~20 s lang
    fn spaeter_verbinden(&mut self, in_s: u64) {
        self.neu_verbinden_ab = Some(Instant::now() + Duration::from_secs(in_s));
        self.neu_verbinden_versuche = 8;
    }

    /// Läuft gerade eine Installation oder der Rückweg? (dann keine eigene Verbindung und keine Seitenwechsel)
    fn installation_laeuft(&self) -> bool {
        #[cfg(target_os = "linux")]
        return self.installation.laeuft();
        #[cfg(not(target_os = "linux"))]
        false
    }

    /// War der letzte Lauf ein erfolgreicher Rückweg zur Werks-Firmware?
    fn zurueck_erfolgreich(&self) -> bool {
        #[cfg(target_os = "linux")]
        return self.installation.zurueck_erfolgreich();
        #[cfg(not(target_os = "linux"))]
        false
    }

    fn meldungen_verarbeiten(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Meldung::Sucht(w) => self.verbindung = Verbindung::Sucht(w),
                Meldung::Verbunden(w, name) => {
                    self.verbindung = Verbindung::Verbunden(w, name);
                    self.fehler = None;
                    self.neu_verbinden_versuche = 0;
                    self.live = false;
                    self.alles_laden();
                }
                Meldung::Getrennt(grund) => {
                    let war_verbunden = self.verbunden();
                    self.verbindung = Verbindung::Getrennt;
                    self.live = false;
                    self.status = None;
                    if grund.is_some() && !war_verbunden && self.neu_verbinden_versuche > 0 {
                        // Controller startet noch: still noch einmal versuchen statt einen Fehler zu zeigen
                        self.neu_verbinden_versuche -= 1;
                        self.neu_verbinden_ab = Some(Instant::now() + Duration::from_millis(2500));
                    } else if let Some(g) = grund {
                        // Der stille Versuch beim Start soll keinen Fehler zeigen
                        if war_verbunden || self.letzte_info.elapsed() > Duration::from_secs(2) {
                            self.fehler = Some(g);
                        }
                    }
                }
                Meldung::Antwort(zweck, ergebnis) => self.antwort(zweck, ergebnis),
                Meldung::Status(s) => {
                    self.getestet |= s.tasten;
                    self.status = Some(s);
                }
                Meldung::Fortschritt(a) => self.update_fortschritt = Some(a),
                Meldung::UpdateFertig(r) => {
                    self.update_fortschritt = None;
                    match r {
                        Ok(()) => {
                            self.melden("Update übertragen und geprüft. Der Controller startet neu.", false);
                            self.update_datei = None;
                            self.spaeter_verbinden(7);
                        }
                        Err(e) => self.melden(format!("Update abgebrochen: {e}. Die bisherige Firmware läuft weiter."), true),
                    }
                }
                Meldung::WeckenFertig(r) => {
                    let abgebrochen = self.weck_lauscht.take().is_some_and(|(a, _)| a.load(Ordering::Relaxed));
                    match r {
                        Ok(k) => {
                            self.melden(format!("Wecken eingerichtet mit {} {}.", k.name(), k.adresse), false);
                            self.weck = Some(Some(k));
                        }
                        Err(_) if abgebrochen => {}
                        Err(e) => self.melden(format!("Wecken einrichten: {e}"), true),
                    }
                    self.senden(Zweck::SwitchDiag, vec!["switch diag".into()]);
                }
            }
        }
    }

    fn antwort(&mut self, zweck: Zweck, ergebnis: Result<Vec<String>, String>) {
        let zeilen = match ergebnis {
            Ok(z) => z,
            Err(e) => {
                if zweck == Zweck::Belegung {
                    self.belegung_speichern = None;
                }
                if zweck != Zweck::Info && zweck != Zweck::Pro2 {
                    self.melden(e, true);
                }
                return;
            }
        };
        let fehler = zeilen.iter().find(|z| z.starts_with("fehler:")).cloned();
        if let (Some(f), false) = (&fehler, zweck == Zweck::Pro2) {
            self.melden(f.trim_start_matches("fehler:").trim().to_string(), true);
        }
        match zweck {
            Zweck::Info => {
                if let Some(i) = zeilen.iter().find_map(|z| Info::lesen(z)) {
                    self.info = Some(i);
                }
            }
            Zweck::SwitchDiag => {
                self.switch_modus = zeilen
                    .iter()
                    .find_map(|z| z.split_whitespace().find_map(|t| t.strip_prefix("modus=")).map(|m| m == "1"));
                if let Some(k) = zeilen.iter().find_map(|z| Kennung::aus_diag(z)) {
                    self.weck = Some(k);
                }
            }
            Zweck::Einstellungen => {
                if let Some(e) = zeilen.iter().find_map(|z| Einstellungen::lesen(z)) {
                    self.einst = Some(e.clone());
                    self.einst_entwurf = Some(e);
                }
            }
            Zweck::Belegung => {
                let b = Belegung::lesen(&zeilen);
                let gespeichert = self.belegung_speichern.take();
                for k in 0..b.plaetze.len() {
                    // Einen Entwurf nur ersetzen, wenn er unverändert war oder gerade gespeichert wurde; sonst ginge
                    // Bearbeitetes verloren (z. B. Makro auf Rück 2 offen, dann ein Turbo-Knopf gedrückt)
                    let unveraendert = self.belegung.as_ref().is_none_or(|alt| self.entwurf[k] == alt.plaetze[k]);
                    if unveraendert || gespeichert.as_ref().is_some_and(|(i, _)| *i == k) {
                        self.entwurf[k] = b.plaetze[k].clone();
                    }
                }
                if let Some((_, name)) = gespeichert {
                    if fehler.is_none() {
                        self.melden(format!("{name} gespeichert."), false);
                    }
                }
                self.belegung = Some(b);
            }
            Zweck::Diagnose => {
                self.diag_ausgabe.extend(zeilen);
                let n = self.diag_ausgabe.len();
                if n > 2000 {
                    self.diag_ausgabe.drain(..n - 2000);
                }
            }
            Zweck::Pro2 => {
                // „ok: pro2 aktiv=1 gewaehlt=0 klassisch=0 gekoppelt=1 konsole=… …“
                let wert = |schluessel: &str| {
                    zeilen.iter().find(|z| z.contains("pro2 aktiv=")).and_then(|z| {
                        z.split_whitespace().find_map(|t| t.strip_prefix(schluessel)).map(|v| v == "1")
                    })
                };
                self.pro2 = match (wert("klassisch="), wert("gekoppelt=")) {
                    (Some(k), Some(g)) => Some((k, g)),
                    _ => None,
                };
            }
            Zweck::Still => {}
            Zweck::Backup => self.backup_speichern(&zeilen),
            Zweck::Wiederherstellen => {
                let fehler = zeilen.iter().filter(|z| z.starts_with("fehler:")).count();
                if fehler == 0 {
                    self.melden("Backup eingespielt.", false);
                }
                self.alles_laden();
            }
        }
    }

    fn backup_speichern(&mut self, zeilen: &[String]) {
        let Some(e) = zeilen.iter().find_map(|z| Einstellungen::lesen(z)) else {
            self.melden("Backup: keine Einstellungen gelesen", true);
            return;
        };
        let b = Belegung::lesen(zeilen);
        let stand = self.info.as_ref().map(|i| i.stand.clone()).unwrap_or_default();
        let mut text = format!(
            "# C2 Lite Backup (c2lite-custom), Firmware-Stand {stand}\n# Jede Zeile ist ein Firmware-Befehl. Einspielen: c2lite-custom, Seite Backup.\n"
        );
        for z in e.befehle().into_iter().chain(b.befehle()) {
            text.push_str(&z);
            text.push('\n');
        }
        if let Some(pfad) = rfd::FileDialog::new()
            .set_title("Backup speichern")
            .set_file_name("c2lite-backup.txt")
            .add_filter("Backup", &["txt"])
            .save_file()
        {
            match std::fs::write(&pfad, text) {
                Ok(()) => self.melden(format!("Backup gespeichert: {}", pfad.display()), false),
                Err(e) => self.melden(format!("Backup nicht gespeichert: {e}"), true),
            }
        }
    }

    fn backup_laden(&mut self) {
        let Some(pfad) = rfd::FileDialog::new().set_title("Backup laden").add_filter("Backup", &["txt"]).pick_file() else {
            return;
        };
        let text = match std::fs::read_to_string(&pfad) {
            Ok(t) => t,
            Err(e) => return self.melden(format!("{}: {e}", pfad.display()), true),
        };
        let befehle: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|z| !z.is_empty() && !z.starts_with('#'))
            .map(str::to_string)
            .collect();
        // Nur Befehle, die Einstellungen ändern: ein Backup darf kein Update oder den Download-Modus auslösen
        if let Some(z) = befehle.iter().find(|z| !["setze ", "belegung ", "turbo "].iter().any(|p| z.starts_with(p))) {
            return self.melden(format!("Kein gültiges Backup (unerwartete Zeile „{z}“)"), true);
        }
        if befehle.is_empty() {
            return self.melden("Das Backup ist leer", true);
        }
        self.senden(Zweck::Wiederherstellen, befehle);
    }

    /// Geänderte Licht-/Vibrationswerte kurz nach der letzten Änderung schicken (nicht bei jedem Schieberschritt)
    fn einstellungen_abgleichen(&mut self, ctx: &egui::Context) {
        let (Some(alt), Some(neu), Some(seit)) = (&self.einst, &self.einst_entwurf, self.einst_geaendert) else {
            return;
        };
        if seit.elapsed() < Duration::from_millis(250) {
            ctx.request_repaint_after(Duration::from_millis(260));
            return;
        }
        let mut b = Vec::new();
        let paare = [
            ("anzeige", alt.anzeige as u32, neu.anzeige as u32),
            ("vibration", alt.vibration as u32, neu.vibration as u32),
            ("totzone", alt.totzone as u32, neu.totzone as u32),
            ("licht", alt.licht as u32, neu.licht as u32),
            ("hell", alt.hell as u32, neu.hell as u32),
            ("logo", alt.logo as u32, neu.logo as u32),
            ("tempo", alt.tempo as u32, neu.tempo.max(1) as u32),
            ("farbe", alt.farbe_zahl(), neu.farbe_zahl()),
        ];
        for (name, a, n) in paare {
            if a != n {
                b.push(format!("setze {name} {n}"));
            }
        }
        self.einst = Some(neu.clone());
        self.einst_geaendert = None;
        if !b.is_empty() {
            self.senden(Zweck::Still, b);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.meldungen_verarbeiten();
        // Installation/Rückweg: Meldungen jedes Bild abholen (nicht nur auf der Seite), und solange es läuft, dort bleiben,
        // damit keine Aufforderung („Home + Y + X“, „Kabel anstecken“) unbemerkt bleibt
        #[cfg(target_os = "linux")]
        self.installation_abholen(&ctx);
        if self.installation_laeuft() {
            self.seite = Seite::Installieren;
        }
        self.einstellungen_abgleichen(&ctx);
        if let Some(ab) = self.neu_verbinden_ab {
            if Instant::now() >= ab {
                self.neu_verbinden_ab = None;
                self.verbinden(self.letzter_weg);
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        if let Some(ab) = self.neu_laden_ab {
            if Instant::now() >= ab {
                self.neu_laden_ab = None;
                self.senden(Zweck::SwitchDiag, vec!["switch diag".into()]);
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        let live = self.verbunden() && self.seite == Seite::Test;
        if live != self.live {
            self.live = live;
            let _ = self.tx.send(Auftrag::Live(live));
        }
        if self.verbunden() && self.seite == Seite::Uebersicht && self.letzte_info.elapsed() > Duration::from_secs(5) {
            self.letzte_info = Instant::now();
            self.senden(Zweck::Info, vec!["info".into()]);
        }
        if self.verbunden() {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
        let c = farben(&ctx);

        egui::Panel::left("navigation")
            .exact_size(236.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(c.flaeche).inner_margin(Margin { left: 14, right: 14, top: 22, bottom: 16 }))
            .show(ui, |ui| self.seitenleiste(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c.grund).inner_margin(Margin::same(0)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::new().inner_margin(Margin { left: 36, right: 36, top: 30, bottom: 36 }).show(ui, |ui| {
                        ui.set_max_width(900.0);
                        if !self.verbunden() && !matches!(self.seite, Seite::Diagnose | Seite::Installieren) {
                            self.leer(ui);
                        } else {
                            match self.seite {
                                Seite::Uebersicht => self.uebersicht(ui),
                                Seite::Test => self.tastentest(ui),
                                Seite::Belegung => self.belegung_seite(ui),
                                Seite::Licht => self.licht_seite(ui),
                                Seite::Firmware => self.firmware_seite(ui),
                                Seite::Diagnose => self.diagnose_seite(ui),
                                Seite::Backup => self.backup_seite(ui),
                                #[cfg(target_os = "linux")]
                                Seite::Installieren => self.installieren_seite(ui),
                                #[cfg(not(target_os = "linux"))]
                                Seite::Installieren => {
                                    self.kopf(ui, "Installieren", "Die Erstinstallation gibt es bisher nur unter Linux.");
                                }
                            }
                        }
                    });
                });
            });

        self.meldung_zeigen(&ctx);
    }
}

// ---------- Seitenleiste und Rahmen ----------

impl App {
    fn seitenleiste(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::GAME_CONTROLLER).size(26.0).color(c.akzent));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("C2 Lite").font(egui::FontId::new(19.0, egui::FontFamily::Name(stil::FETT.into()))));
                ui.label(RichText::new("c2lite-custom").small().color(c.schwach));
            });
        });
        ui.add_space(22.0);
        for (seite, ic, name) in SEITEN {
            let gewaehlt = self.seite == *seite;
            let (rect, antwort) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());
            let fill = if gewaehlt {
                c.akzent_schwach
            } else if antwort.hovered() {
                c.erhoeht
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::same(stil::RADIUS), fill);
            let farbe = if gewaehlt { c.akzent } else { c.text };
            ui.painter().text(
                rect.left_center() + Vec2::new(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{ic}   {name}"),
                egui::FontId::proportional(15.0),
                farbe,
            );
            if antwort.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !self.installation_laeuft() {
                self.seite = *seite;
            }
        }
        let gesperrt = self.installation_laeuft();
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            match &self.verbindung {
                Verbindung::Verbunden(weg, name) => {
                    let weg = *weg;
                    let name = name.clone();
                    if ui.add_sized([ui.available_width(), 32.0], egui::Button::new("Trennen")).clicked() {
                        let _ = self.tx.send(Auftrag::Trennen);
                    }
                    let _ = name;
                    ui.label(
                        RichText::new(format!(
                            "{}  Verbunden über {}",
                            if weg == Weg::Usb { icon::USB } else { icon::BLUETOOTH },
                            if weg == Weg::Usb { "USB" } else { "Bluetooth" }
                        ))
                        .color(c.akzent),
                    );
                }
                Verbindung::Sucht(weg) => {
                    let text = if *weg == Weg::Usb { "Suche am USB" } else { "Suche per Bluetooth (bis 15 s)" };
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(text).color(c.schwach));
                    });
                }
                Verbindung::Getrennt => {
                    // Während Installation/Rückweg gehört der Controller dem Installer: nicht dazwischen verbinden
                    ui.add_enabled_ui(!gesperrt, |ui| {
                        if ui.add_sized([ui.available_width(), 32.0], egui::Button::new(format!("{}  Bluetooth", icon::BLUETOOTH))).clicked() {
                            self.verbinden(Weg::Bluetooth);
                        }
                        let w = ui.available_width();
                        let c2 = farben(ui.ctx());
                        if ui
                            .add_sized(
                                [w, 34.0],
                                egui::Button::new(RichText::new(format!("{}  USB verbinden", icon::USB)).color(c2.akzent_text)).fill(c2.akzent),
                            )
                            .clicked()
                        {
                            self.verbinden(Weg::Usb);
                        }
                    });
                    ui.label(RichText::new(if gesperrt { "Installation läuft" } else { "Nicht verbunden" }).color(c.schwach));
                }
            }
        });
    }

    fn kopf(&self, ui: &mut egui::Ui, text: &str, unterzeile: &str) {
        ui.heading(text);
        if !unterzeile.is_empty() {
            hinweis(ui, unterzeile);
        }
        ui.add_space(14.0);
    }

    fn leer(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(icon::GAME_CONTROLLER).size(64.0).color(c.schwach));
            ui.add_space(8.0);
            if let Some(art) = usb_ohne_custom() {
                // Werks-Firmware oder Download-Modus am USB: Laien direkt zur Installation führen
                let (titel_text, text) = match art {
                    UsbArt::Werk if self.zurueck_erfolgreich() => {
                        ("Werks-Firmware läuft wieder", "Mobapad-App und alte Kopplungen gehen wieder. Installieren geht jederzeit erneut.")
                    }
                    UsbArt::Werk => ("Werks-Firmware erkannt", "Auf der Seite „Installieren“ kommt C2 Lite Custom drauf."),
                    // Läuft keine Installation, wartet der Controller vergeblich: zum erneuten Start führen statt „abwarten“
                    UsbArt::Download => (
                        "Controller im Download-Modus",
                        "Er wartet auf Firmware. Auf „Installieren“ die Installation oder den Rückweg neu starten. Kabel stecken lassen.",
                    ),
                };
                ui.heading(titel_text);
                ui.add_space(4.0);
                ui.label(RichText::new(text).color(c.schwach));
                ui.add_space(16.0);
                if ui
                    .add_sized([220.0, 38.0], egui::Button::new(RichText::new(format!("{}  Zur Installation", icon::DOWNLOAD_SIMPLE)).color(c.akzent_text)).fill(c.akzent))
                    .clicked()
                {
                    self.seite = Seite::Installieren;
                }
                ui.ctx().request_repaint_after(Duration::from_secs(1));
                return;
            }
            ui.heading("Kein Controller verbunden");
            ui.add_space(4.0);
            ui.label(
                RichText::new("USB-Kabel anstecken (Schieber auf PC) oder per Bluetooth verbinden.").color(c.schwach),
            );
            ui.add_space(16.0);
            match self.verbindung {
                Verbindung::Sucht(_) => {
                    ui.spinner();
                }
                _ => {
                    ui.horizontal(|ui| {
                        let breite = 2.0 * 190.0 + ui.spacing().item_spacing.x;
                        ui.add_space(((ui.available_width() - breite) / 2.0).max(0.0));
                        if ui
                            .add_sized([190.0, 38.0], egui::Button::new(RichText::new(format!("{}  USB verbinden", icon::USB)).color(c.akzent_text)).fill(c.akzent))
                            .clicked()
                        {
                            self.verbinden(Weg::Usb);
                        }
                        if ui.add_sized([190.0, 38.0], egui::Button::new(format!("{}  Bluetooth", icon::BLUETOOTH))).clicked() {
                            self.verbinden(Weg::Bluetooth);
                        }
                    });
                }
            }
            if let Some(f) = &self.fehler {
                ui.add_space(14.0);
                ui.label(RichText::new(format!("{}  {f}", icon::WARNING)).color(c.fehler));
            }
        });
    }

    fn meldung_zeigen(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.meldung else { return };
        let dauer = if m.fehler { 7 } else { 4 };
        if m.seit.elapsed() > Duration::from_secs(dauer) {
            self.meldung = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let c = farben(ctx);
        egui::Area::new(egui::Id::new("meldung"))
            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -24.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(c.flaeche)
                    .stroke(Stroke::new(1.0, if m.fehler { c.fehler } else { c.akzent }))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        let (ic, farbe) = if m.fehler { (icon::WARNING, c.fehler) } else { (icon::CHECK_CIRCLE, c.akzent) };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(ic).color(farbe).size(18.0));
                            ui.label(&m.text);
                        });
                    });
            });
    }
}

// ---------- Seiten ----------

impl App {
    fn uebersicht(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.kopf(ui, "Übersicht", "Zustand des Controllers. Werte aktualisieren sich alle paar Sekunden.");
        let info = self.info.clone();
        ui.columns(3, |sp| {
            karte(&mut sp[0], |ui| {
                hinweis(ui, "Modus");
                let (ic, text, unter) = match self.switch_modus {
                    Some(true) => (icon::GAME_CONTROLLER, "Switch", "als Pro Controller"),
                    Some(false) => (icon::CPU, "PC", "USB und Bluetooth LE"),
                    None => (icon::CIRCLE_NOTCH, "...", ""),
                };
                ui.label(RichText::new(format!("{ic}  {text}")).size(26.0).strong());
                hinweis(ui, unter);
            });
            karte(&mut sp[1], |ui| {
                hinweis(ui, "Akku");
                match &info {
                    Some(i) if i.akku_prozent > 0 => {
                        let ic = if i.usb5v { icon::BATTERY_CHARGING } else { icon::BATTERY_FULL };
                        ui.label(RichText::new(format!("{ic}  {} %", i.akku_prozent)).size(26.0).strong());
                        hinweis(ui, &format!("{:.2} V, {}", i.akku_mv as f32 / 1000.0, if i.usb5v { "am USB" } else { "Akkubetrieb" }));
                    }
                    _ => {
                        ui.label(RichText::new("...").size(26.0));
                    }
                }
            });
            karte(&mut sp[2], |ui| {
                hinweis(ui, "Firmware");
                match &info {
                    Some(i) => {
                        ui.label(RichText::new(format!("Stand {}", i.stand)).size(26.0).strong());
                        hinweis(ui, &format!("gebaut {}", i.gebaut));
                    }
                    None => {
                        ui.label(RichText::new("...").size(26.0));
                    }
                }
            });
        });
        ui.add_space(12.0);
        karte(ui, |ui| {
            titel(ui, "Verbindung");
            ui.add_space(4.0);
            egui::Grid::new("verbindung").num_columns(2).spacing([28.0, 8.0]).show(ui, |ui| {
                if let Verbindung::Verbunden(weg, name) = &self.verbindung {
                    hinweis(ui, "Weg");
                    ui.label(if *weg == Weg::Usb { "USB-Kabel" } else { "Bluetooth" });
                    ui.end_row();
                    hinweis(ui, "Gerät");
                    ui.label(name);
                    ui.end_row();
                }
                if let Some(i) = &info {
                    hinweis(ui, "Bluetooth LE");
                    ui.label(if i.ble { "mit einem Gerät verbunden" } else { "nicht verbunden" });
                    ui.end_row();
                    hinweis(ui, "Läuft seit");
                    ui.label(format!("{} min {} s", i.laufzeit_s / 60, i.laufzeit_s % 60));
                    ui.end_row();
                    hinweis(ui, "Letzter Neustart");
                    ui.label(match i.reset.as_str() {
                        "0x01" => "Einschalten",
                        "0x04" => "Watchdog (Firmware hing)",
                        "0x40" => "Software (Update, Neustart, Moduswechsel)",
                        "0x10" => "Weckeingang",
                        r => r,
                    });
                    ui.end_row();
                }
            });
        });
        ui.add_space(12.0);
        self.wecken_karte(ui);
        ui.add_space(12.0);
        karte(ui, |ui| {
            titel(ui, "Aktionen");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(format!("{}  Neu starten", icon::ARROWS_CLOCKWISE)).clicked() {
                    self.senden(Zweck::Still, vec!["neustart".into()]);
                    self.spaeter_verbinden(6);
                    self.melden("Controller startet neu.", false);
                }
                if ui.button(format!("{}  Download-Modus", icon::DOWNLOAD_SIMPLE)).clicked() {
                    self.frage_download = true;
                }
            });
            if self.frage_download {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Der Controller springt in den ROM-Download-Modus und ist danach nur noch für ein Flash-Werkzeug \
                         erreichbar. Zurück: Kabel ab und wieder an.",
                    )
                    .color(c.warnung),
                );
                ui.horizontal(|ui| {
                    if ui.button("Abbrechen").clicked() {
                        self.frage_download = false;
                    }
                    if knopf_haupt(ui, "Download-Modus starten").clicked() {
                        self.frage_download = false;
                        self.senden(Zweck::Still, vec!["download-modus".into()]);
                        // Danach gibt es keinen Befehlskanal mehr: sofort trennen statt alte Werte weiter anzuzeigen
                        let _ = self.tx.send(Auftrag::Trennen);
                    }
                });
            }
        });
        ui.add_space(12.0);
        karte(ui, |ui| {
            titel(ui, "Bedienung am Controller");
            ui.add_space(4.0);
            egui::Grid::new("bedienung").num_columns(2).spacing([28.0, 6.0]).show(ui, |ui| {
                for (k, t) in [
                    ("Licht kurz", "nächster Lichtmodus"),
                    ("Licht 3 s halten", "Tasten aufnehmen, dann Rück 1, Rück 2 oder M drücken"),
                    ("Licht + Taste", "Turbo dieser Taste: beim Halten, automatisch, aus"),
                    ("Licht + linker Stick", "links/rechts Turbo-Tempo, hoch/runter Vibration"),
                    ("Licht + rechter Stick", "hoch/runter Helligkeit"),
                    ("Licht + Home", "nur klassischer Pro Controller: Joy-Con-2-Weckkennung lernen (Logo orange)"),
                    ("Home + Minus 5 s", "Download-Modus zum Flashen"),
                    ("Schieber", "PC, Aus, Switch"),
                ] {
                    ui.label(RichText::new(k).strong());
                    hinweis(ui, t);
                    ui.end_row();
                }
            });
        });
    }

    /// Home am Controller weckt die Switch 2: dafür braucht der Controller die Kennung eines Joy-Con 2
    fn wecken_karte(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        // Ab Stufe 11 (Standard): Pro Controller 2, weckt mit der eigenen Adresse; keine Joy-Con-Kennung nötig
        if let Some((false, gekoppelt)) = self.pro2 {
            karte(ui, |ui| {
                titel(ui, "Switch 2");
                ui.add_space(4.0);
                if gekoppelt {
                    ui.label(
                        RichText::new(format!("{}  Als Pro Controller 2 gekoppelt: Home weckt die Switch 2", icon::CHECK_CIRCLE))
                            .color(c.akzent),
                    );
                    hinweis(ui, "Mit der eigenen Adresse, ohne Joy-Con.");
                } else {
                    ui.label("Noch nicht mit einer Switch 2 gekoppelt.");
                    hinweis(
                        ui,
                        "Schieber auf Switch, an der Switch 2 „Griff/Reihenfolge ändern“ öffnen: Sie koppelt von selbst (kein \
                         Kabel nötig). Danach weckt Home die Konsole.",
                    );
                }
            });
            return;
        }
        karte(ui, |ui| {
            titel(ui, "Switch 2 aufwecken (klassischer Pro Controller)");
            ui.add_space(4.0);
            match &self.weck {
                Some(Some(k)) => {
                    ui.label(
                        RichText::new(format!("{}  Eingerichtet: Home am Controller weckt die Switch 2", icon::CHECK_CIRCLE))
                            .color(c.akzent),
                    );
                    hinweis(ui, &format!("Wecksignal mit der Kennung von {} {}", k.name(), k.adresse));
                }
                Some(None) => {
                    ui.label("Noch nicht eingerichtet.");
                    hinweis(
                        ui,
                        "Die Switch 2 wacht nur bei Controllern auf, die sie selbst kennt. Der C2 Lite leiht sich dafür \
                         die Kennung eines Joy-Con 2, das Werkzeug fängt sie einmal auf.",
                    );
                }
                None => hinweis(ui, "Firmware ab Stand 9d nötig."),
            }
            ui.add_space(6.0);
            if let Some((abbruch, seit)) = &self.weck_lauscht {
                let rest = 90u64.saturating_sub(seit.elapsed().as_secs());
                ui.label(RichText::new(format!(
                    "{}  Lausche … ({rest} s). Switch 2 in den Standby schalten, einen Joy-Con 2 abnehmen und eine Taste \
                     daran drücken. Die Switch wacht dabei auf, das ist normal.",
                    icon::BROADCAST
                )));
                if ui.button("Abbrechen").clicked() {
                    abbruch.store(true, Ordering::Relaxed);
                }
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else {
                ui.horizontal(|ui| {
                    let text = if matches!(self.weck, Some(Some(_))) { "Neu einrichten" } else { "Wecken einrichten" };
                    if knopf_haupt(ui, &format!("{}  {text}", icon::BROADCAST)).clicked() {
                        let abbruch = Arc::new(AtomicBool::new(false));
                        let _ = self.tx.send(Auftrag::WeckenEinrichten(abbruch.clone()));
                        self.weck_lauscht = Some((abbruch, Instant::now()));
                    }
                    if ui.button(format!("{}  Am Controller lernen", icon::GAME_CONTROLLER)).clicked() {
                        self.senden(Zweck::Still, vec!["switch weck lernen".into()]);
                        self.melden(
                            "Der Controller lauscht 30 s (Logo orange): Joy-Con 2 abnehmen und eine Taste daran drücken.",
                            false,
                        );
                        self.neu_laden_ab = Some(Instant::now() + Duration::from_secs(31));
                    }
                    if matches!(self.weck, Some(Some(_))) && ui.button(format!("{}  Jetzt wecken", icon::SUN)).clicked() {
                        self.senden(Zweck::Still, vec!["switch weck".into()]);
                        self.melden("Wecksignal gesendet (2 s).", false);
                    }
                });
            }
        });
    }

    fn tastentest(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.kopf(ui, "Tastentest", "Drück jede Taste einmal und bewege beide Sticks. Getestete Tasten bekommen einen grünen Rand.");
        karte(ui, |ui| {
            bild::zeichnen(ui, self.status.as_ref(), self.getestet, c);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let n = ROLLEN.iter().filter(|(_, b)| self.getestet & (1u64 << b) != 0).count();
                ui.label(RichText::new(format!("{n} von {} Tasten getestet", ROLLEN.len())).strong());
                if n == ROLLEN.len() {
                    ui.label(RichText::new(format!("{}  alle in Ordnung", icon::CHECK_CIRCLE)).color(c.akzent));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Zurücksetzen").clicked() {
                        self.getestet = 0;
                    }
                    if let Some(s) = &self.status {
                        ui.label(
                            RichText::new(format!(
                                "links {:4} {:4}   rechts {:4} {:4}",
                                s.achsen[0], s.achsen[1], s.achsen[2], s.achsen[3]
                            ))
                            .monospace()
                            .color(c.schwach),
                        );
                    }
                });
            });
        });
        ui.add_space(12.0);
        ui.columns(2, |sp| {
            karte(&mut sp[0], |ui| {
                titel(ui, "Spieler-LEDs");
                hinweis(ui, "Leuchten die vier LEDs zwischen den Sticks?");
                ui.horizontal_wrapped(|ui| {
                    for n in 1..=4u8 {
                        if ui.button(format!("{n}")).clicked() {
                            self.senden(Zweck::Still, vec![format!("led {}", crate::spieler_bit(n))]);
                        }
                    }
                    if ui.button("Alle").clicked() {
                        self.senden(Zweck::Still, vec!["led 60".into()]);
                    }
                    if ui.button("Automatisch").clicked() {
                        self.senden(Zweck::Still, vec!["led auto".into()]);
                    }
                });
            });
            karte(&mut sp[1], |ui| {
                titel(ui, "Vibration");
                hinweis(ui, "Kurzer Stoß, links, rechts oder beide Motoren.");
                ui.horizontal_wrapped(|ui| {
                    for (name, l, r) in [("Links", 70, 0), ("Rechts", 0, 70), ("Beide", 70, 70)] {
                        if ui.button(format!("{}  {name}", icon::VIBRATE)).clicked() {
                            self.senden(Zweck::Still, vec![format!("motor {l} {r} 400")]);
                        }
                    }
                });
            });
        });
        #[cfg(target_os = "linux")]
        {
            ui.add_space(12.0);
            self.bluetooth_karte(ui);
        }
    }

    fn belegung_seite(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.kopf(
            ui,
            "Belegung & Turbo",
            "Rücktasten und M senden, was hier eingestellt ist. Gilt in allen Modi und wird im Controller gespeichert.",
        );
        let Some(beleg) = self.belegung.clone() else {
            ui.spinner();
            return;
        };
        ui.horizontal(|ui| {
            for (i, (_, name)) in PLAETZE.iter().enumerate() {
                if ui.add(stil::wahl(self.platz == i, *name).min_size(Vec2::new(150.0, 34.0))).clicked() {
                    self.platz = i;
                }
            }
        });
        ui.add_space(10.0);
        let i = self.platz;
        let (pname, plabel) = PLAETZE[i];
        let mut speichern = false;
        let mut verwerfen = false;
        karte(ui, |ui| {
            let p = &mut self.entwurf[i];
            titel(ui, plabel);
            ui.horizontal(|ui| {
                for (art, text) in [(Art::Aus, "Aus"), (Art::Taste, "Taste"), (Art::Makro, "Makro")] {
                    if ui.add(stil::wahl(p.art == art, text).min_size(Vec2::new(96.0, 30.0))).clicked() {
                        p.art = art;
                        if art == Art::Taste && p.tasten.is_empty() {
                            p.tasten = vec!["a".into()];
                        }
                        if art == Art::Makro && p.schritte.is_empty() {
                            p.schritte.push(Schritt::default());
                        }
                    }
                }
            });
            ui.add_space(6.0);
            match p.art {
                Art::Aus => hinweis(ui, "Die Taste sendet nichts."),
                Art::Taste => {
                    hinweis(ui, "Solange die Taste gehalten wird, gelten diese Tasten als gedrückt (mehrere möglich).");
                    tasten_auswahl(ui, &mut p.tasten, c);
                }
                Art::Makro => makro_editor(ui, p, c),
            }
            ui.add_space(10.0);
            let geaendert = *p != beleg.plaetze[i];
            ui.horizontal(|ui| {
                ui.add_enabled_ui(geaendert, |ui| {
                    if knopf_haupt(ui, "Speichern").clicked() {
                        speichern = true;
                    }
                    if ui.button("Verwerfen").clicked() {
                        verwerfen = true;
                    }
                });
                if !geaendert {
                    hinweis(ui, "Gespeichert");
                }
            });
        });
        if speichern {
            let mut b = self.entwurf[i].befehle(pname);
            b.push("belegung".into());
            self.senden(Zweck::Belegung, b);
            self.belegung_speichern = Some((i, plabel.to_string()));
        }
        if verwerfen {
            self.entwurf[i] = beleg.plaetze[i].clone();
        }

        ui.add_space(12.0);
        karte(ui, |ui| {
            titel(ui, "Turbo");
            hinweis(ui, "Beim Halten: feuert, solange die Taste gedrückt ist. Automatisch: ein Druck startet, der nächste stoppt.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Tempo");
                for (t, text) in [(0u8, "5 Hz"), (1, "8 Hz"), (2, "12 Hz")] {
                    if ui.add(stil::wahl(beleg.turbo_tempo == t, text)).clicked() && beleg.turbo_tempo != t {
                        self.senden(Zweck::Belegung, vec![format!("turbo tempo {t}"), "belegung".into()]);
                    }
                }
            });
            ui.add_space(6.0);
            let mut liste: Vec<(&str, &str)> = TASTEN.to_vec();
            for (k, (n, l)) in PLAETZE.iter().enumerate() {
                if beleg.plaetze[k].art == Art::Taste {
                    liste.push((n, l));
                }
            }
            egui::Grid::new("turbo").num_columns(4).spacing([16.0, 6.0]).show(ui, |ui| {
                for (k, (taste, name)) in liste.iter().enumerate() {
                    ui.add_sized([96.0, 30.0], egui::Label::new(RichText::new(*name).strong()));
                    let jetzt = beleg.turbo_von(taste);
                    ui.horizontal(|ui| {
                        for (m, text) in [(0u8, "Aus"), (1, "Halten"), (2, "Auto")] {
                            if ui.add(stil::wahl(jetzt == m, text)).clicked() && jetzt != m {
                                self.senden(Zweck::Belegung, vec![format!("turbo {taste} {}", TURBO_MODI[m as usize]), "belegung".into()]);
                            }
                        }
                    });
                    if k % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        });
        ui.add_space(12.0);
        karte(ui, |ui| {
            titel(ui, "Werkszustand");
            hinweis(ui, "Rück 1 = B, Rück 2 = A, M = Makro wie bei Mobapad, kein Turbo.");
            if !self.frage_werkszustand {
                if ui.button("Auf Werkszustand setzen").clicked() {
                    self.frage_werkszustand = true;
                }
            } else {
                ui.horizontal(|ui| {
                    if ui.button("Abbrechen").clicked() {
                        self.frage_werkszustand = false;
                    }
                    if knopf_haupt(ui, "Ja, zurücksetzen").clicked() {
                        self.frage_werkszustand = false;
                        self.senden(Zweck::Belegung, vec!["belegung standard".into(), "belegung".into()]);
                    }
                });
            }
        });
    }

    fn licht_seite(&mut self, ui: &mut egui::Ui) {
        self.kopf(ui, "Licht & Vibration", "Änderungen gehen sofort an den Controller und bleiben gespeichert.");
        let Some(mut e) = self.einst_entwurf.clone() else {
            ui.spinner();
            return;
        };
        karte(ui, |ui| {
            titel(ui, "Lichtmodus");
            ui.horizontal_wrapped(|ui| {
                for (m, name) in LICHTMODI {
                    if ui.add(stil::wahl(e.licht == *m, *name).min_size(Vec2::new(116.0, 34.0))).clicked() {
                        e.licht = *m;
                    }
                }
            });
            if e.licht == 0 || e.licht == 1 {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Farbe");
                    ui.color_edit_button_srgb(&mut e.farbe);
                    for f in [[255, 255, 255], [255, 40, 40], [255, 120, 0], [255, 200, 0], [40, 220, 90], [0, 200, 255], [40, 80, 255], [255, 0, 160]] {
                        let (rect, a) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                        ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(f[0], f[1], f[2]));
                        if e.farbe == f {
                            ui.painter().rect_stroke(rect.expand(2.0), CornerRadius::same(7), Stroke::new(2.0, farben(ui.ctx()).akzent), egui::StrokeKind::Outside);
                        }
                        if a.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            e.farbe = f;
                        }
                    }
                });
            }
            ui.add_space(8.0);
            egui::Grid::new("licht").num_columns(2).spacing([24.0, 10.0]).show(ui, |ui| {
                ui.label("Streifen und Seiten");
                stil::regler(ui, &mut e.hell, 0..=100, " %");
                ui.end_row();
                ui.label("Logo (Statusanzeige)");
                stil::regler(ui, &mut e.logo, 0..=100, " %");
                ui.end_row();
                ui.label("Tempo der Effekte");
                stil::regler(ui, &mut e.tempo, 1..=100, "");
                ui.end_row();
            });
        });
        ui.add_space(12.0);
        karte(ui, |ui| {
            titel(ui, "Vibration und Sticks");
            egui::Grid::new("vib").num_columns(2).spacing([24.0, 10.0]).show(ui, |ui| {
                ui.label("Vibrationsstärke");
                ui.horizontal(|ui| {
                    stil::regler(ui, &mut e.vibration, 0..=100, " %");
                    if ui.button(format!("{}  Testen", icon::VIBRATE)).clicked() {
                        let v = e.vibration;
                        self.senden(Zweck::Still, vec![format!("motor {v} {v} 400")]);
                    }
                });
                ui.end_row();
                ui.label("Stick-Totzone");
                stil::regler(ui, &mut e.totzone, 0..=100, "");
                ui.end_row();
                ui.label("Spieler-LEDs");
                ui.horizontal(|ui| {
                    for (an, text) in [(true, "An"), (false, "Aus")] {
                        if ui.add(stil::wahl(e.anzeige == an, text).min_size(Vec2::new(64.0, 30.0))).clicked() {
                            e.anzeige = an;
                        }
                    }
                    hinweis(ui, "zeigen Verbindung, Laden und Spielernummer");
                });
                ui.end_row();
            });
        });
        if Some(&e) != self.einst_entwurf.as_ref() {
            self.einst_entwurf = Some(e);
            self.einst_geaendert = Some(Instant::now());
        }
    }

    fn firmware_seite(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.kopf(ui, "Firmware", "Updates für C2 Lite Custom. Bricht etwas ab, läuft die alte Version weiter.");
        let installiert = self.info.as_ref().map(|i| i.stand.clone());
        let im_programm = crate::paket::stufe();
        karte(ui, |ui| {
            egui::Grid::new("staende").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("Auf dem Controller").color(c.schwach));
                match &installiert {
                    Some(s) => ui.label(RichText::new(format!("Stufe {s}")).strong()),
                    None => ui.spinner(),
                };
                ui.end_row();
                ui.label(RichText::new("Im Programm").color(c.schwach));
                ui.label(RichText::new(format!("Stufe {im_programm}")).strong());
                ui.end_row();
            });
            ui.add_space(10.0);

            let laeuft = self.update_fortschritt.is_some();
            if let Some(a) = self.update_fortschritt {
                ui.add(egui::ProgressBar::new(a).show_percentage().fill(c.akzent));
                ui.label(RichText::new("Nicht trennen, solange das Update läuft.").color(c.warnung));
            } else if let Some((p, d)) = self.update_datei.clone() {
                // Eigene Datei gewählt
                ui.horizontal(|ui| {
                    ui.label(format!("Datei: {}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                    if ui.small_button("Eingebaute nehmen").clicked() {
                        self.update_datei = None;
                    }
                });
                ui.add_space(6.0);
                if knopf_haupt(ui, &format!("{}  Update einspielen", icon::DOWNLOAD_SIMPLE)).clicked() {
                    self.update_fortschritt = Some(0.0);
                    let _ = self.tx.send(Auftrag::Update(d));
                }
            } else {
                match installiert.as_deref().map(|s| crate::paket::stufe_vergleich(s, im_programm)) {
                    Some(std::cmp::Ordering::Equal) => {
                        ui.label(RichText::new(format!("{}  Aktuell", icon::CHECK_CIRCLE)).color(c.akzent).size(16.0));
                    }
                    // Controller neuer als das Programm: kein Knopf, sonst würde man versehentlich zurückstufen
                    Some(std::cmp::Ordering::Greater) => {
                        ui.label(RichText::new(format!("{}  Neuer als im Programm", icon::CHECK_CIRCLE)).color(c.akzent).size(16.0));
                    }
                    Some(std::cmp::Ordering::Less) => {
                        if knopf_haupt(ui, &format!("{}  Auf Stufe {im_programm} aktualisieren", icon::DOWNLOAD_SIMPLE)).clicked() {
                            self.update_fortschritt = Some(0.0);
                            let _ = self.tx.send(Auftrag::Update(crate::paket::UPDATE.to_vec()));
                        }
                    }
                    None => {}
                }
            }
            ui.add_space(8.0);
            ui.add_enabled_ui(!laeuft, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("Andere Datei …").on_hover_text("db_update_data.bin, z. B. selbst gebaut").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Firmware-Update", &["bin"]).pick_file() {
                            match std::fs::read(&p) {
                                Ok(d) if !d.is_empty() => self.update_datei = Some((p, d)),
                                Ok(_) => self.melden("Die Datei ist leer", true),
                                Err(e) => self.melden(format!("{}: {e}", p.display()), true),
                            }
                        }
                    }
                    hinweis(ui, "Über USB etwa 15 s, über Bluetooth 1 bis 2 min.");
                });
            });
        });
        ui.add_space(8.0);
        hinweis(ui, "Controller noch mit Werks-Firmware? Dann zuerst die Seite „Installieren“.");
    }

    fn diagnose_seite(&mut self, ui: &mut egui::Ui) {
        let c = farben(ui.ctx());
        self.kopf(ui, "Diagnose", "Für die Fehlersuche: Firmware-Befehle direkt senden und Protokolle lesen.");
        let verbunden = self.verbunden();
        karte(ui, |ui| {
            ui.add_enabled_ui(verbunden, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (text, befehl) in [
                        ("Info", "info"),
                        ("Switch-Status", "switch diag"),
                        ("Akku", "akku diag"),
                        ("Belegung", "belegung"),
                        ("Protokoll", "log 1200"),
                        ("Bluetooth-Mitschnitt", "btlog"),
                        ("Hilfe", "hilfe"),
                    ] {
                        if ui.button(text).clicked() {
                            self.diag_ausgabe.push(format!("> {befehl}"));
                            self.senden(Zweck::Diagnose, vec![befehl.into()]);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let feld = ui.add(
                        egui::TextEdit::singleline(&mut self.diag_eingabe)
                            .hint_text("Befehl, z. B. status")
                            .font(egui::TextStyle::Monospace)
                            .margin(Margin::symmetric(10, 8))
                            .desired_width(ui.available_width() - 110.0),
                    );
                    let enter = feld.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.button("Senden").clicked() || enter) && !self.diag_eingabe.trim().is_empty() {
                        let b = self.diag_eingabe.trim().to_string();
                        self.diag_ausgabe.push(format!("> {b}"));
                        self.senden(Zweck::Diagnose, vec![b]);
                        self.diag_eingabe.clear();
                        feld.request_focus();
                    }
                });
            });
            if !verbunden {
                hinweis(ui, "Zum Senden zuerst den Controller verbinden.");
            }
        });
        ui.add_space(12.0);
        karte(ui, |ui| {
            ui.horizontal(|ui| {
                titel(ui, "Ausgabe");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Leeren").clicked() {
                        self.diag_ausgabe.clear();
                    }
                    if ui.button(format!("{}  Kopieren", icon::COPY)).clicked() {
                        ui.ctx().copy_text(self.diag_ausgabe.join("\n"));
                        self.melden("In die Zwischenablage kopiert.", false);
                    }
                });
            });
            egui::Frame::new().fill(c.grund).corner_radius(CornerRadius::same(stil::RADIUS)).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(380.0).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    if self.diag_ausgabe.is_empty() {
                        hinweis(ui, "Noch keine Ausgabe.");
                    }
                    for z in &self.diag_ausgabe {
                        let farbe = if z.starts_with("fehler:") {
                            c.fehler
                        } else if z.starts_with("> ") {
                            c.akzent
                        } else {
                            c.text
                        };
                        ui.label(RichText::new(z).monospace().color(farbe));
                    }
                });
            });
        });
    }

    fn backup_seite(&mut self, ui: &mut egui::Ui) {
        self.kopf(ui, "Backup", "Licht, Vibration, Belegung und Turbo als Textdatei sichern und wieder einspielen.");
        ui.columns(2, |sp| {
            karte(&mut sp[0], |ui| {
                titel(ui, "Sichern");
                hinweis(ui, "Liest die aktuellen Einstellungen aus dem Controller und speichert sie als Datei.");
                ui.add_space(6.0);
                if knopf_haupt(ui, &format!("{}  Backup speichern", icon::FLOPPY_DISK)).clicked() {
                    self.senden(Zweck::Backup, vec!["einstellungen".into(), "belegung".into()]);
                }
            });
            karte(&mut sp[1], |ui| {
                titel(ui, "Wiederherstellen");
                hinweis(ui, "Spielt eine Backup-Datei ein. Die aktuellen Einstellungen werden überschrieben.");
                ui.add_space(6.0);
                if ui.button(format!("{}  Backup laden", icon::UPLOAD_SIMPLE)).clicked() {
                    self.backup_laden();
                }
            });
        });
    }
}

/// Mehrfachauswahl von Tasten als Schaltflächen
fn tasten_auswahl(ui: &mut egui::Ui, gewaehlt: &mut Vec<String>, c: stil::Farben) {
    for (gruppe, liste) in [("Tasten", TASTEN), ("Stick-Richtungen", RICHTUNGEN)] {
        ui.label(RichText::new(gruppe).small().color(c.schwach));
        ui.horizontal_wrapped(|ui| {
            for (name, anzeige) in liste {
                let an = gewaehlt.iter().any(|t| t == name);
                if ui.add(stil::wahl(an, *anzeige).min_size(Vec2::new(52.0, 30.0))).clicked() {
                    if an {
                        gewaehlt.retain(|t| t != name);
                    } else {
                        gewaehlt.push(name.to_string());
                    }
                }
            }
        });
    }
}

fn makro_editor(ui: &mut egui::Ui, p: &mut Platz, c: stil::Farben) {
    hinweis(ui, "Schritte laufen nacheinander ab: Tasten halten, dann Pause. Zeiten in Millisekunden.");
    ui.add_space(4.0);
    let mut weg = None;
    let mut hoch = None;
    egui::Grid::new("makro").num_columns(5).spacing([14.0, 6.0]).striped(false).show(ui, |ui| {
        hinweis(ui, "#");
        hinweis(ui, "Tasten");
        hinweis(ui, "halten");
        hinweis(ui, "Pause");
        ui.label("");
        ui.end_row();
        let n = p.schritte.len();
        for (k, s) in p.schritte.iter_mut().enumerate() {
            ui.label(RichText::new(format!("{}", k + 1)).color(c.schwach));
            let text = if s.tasten.is_empty() {
                "keine".to_string()
            } else {
                s.tasten.iter().map(|t| anzeigename(t)).collect::<Vec<_>>().join(" + ")
            };
            ui.menu_button(text, |ui| {
                ui.set_max_width(420.0);
                tasten_auswahl(ui, &mut s.tasten, c);
            });
            ui.add(egui::DragValue::new(&mut s.halten).range(10..=30000).suffix(" ms").speed(5));
            ui.add(egui::DragValue::new(&mut s.pause).range(0..=30000).suffix(" ms").speed(5));
            ui.horizontal(|ui| {
                if ui.add_enabled(k > 0, egui::Button::new(icon::ARROW_UP)).on_hover_text("nach oben").clicked() {
                    hoch = Some(k);
                }
                if ui.add_enabled(k + 1 < n, egui::Button::new(icon::ARROW_DOWN)).on_hover_text("nach unten").clicked() {
                    hoch = Some(k + 1);
                }
                if ui.button(icon::TRASH).on_hover_text("entfernen").clicked() {
                    weg = Some(k);
                }
            });
            ui.end_row();
        }
    });
    if let Some(k) = hoch {
        p.schritte.swap(k - 1, k);
    }
    if let Some(k) = weg {
        p.schritte.remove(k);
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(p.schritte.len() < MAX_SCHRITTE, egui::Button::new(format!("{}  Schritt", icon::PLUS))).clicked() {
            p.schritte.push(Schritt::default());
        }
        hinweis(ui, &format!("{} von {MAX_SCHRITTE}", p.schritte.len()));
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Ablauf");
        for (w, text) in [("einmal", "einmal pro Druck"), ("endlos", "endlos, Druck startet/stoppt"), ("halten", "solange gehalten")] {
            if ui.add(stil::wahl(p.wiederholen == w, text)).clicked() {
                p.wiederholen = w.to_string();
            }
        }
    });
    if p.wiederholen != "einmal" {
        ui.horizontal(|ui| {
            ui.label("Pause zwischen Durchläufen");
            ui.add(egui::DragValue::new(&mut p.intervall).range(0..=30000).suffix(" ms").speed(10));
        });
    }
}

enum UsbArt {
    Werk,
    Download,
}

/// C2 Lite am USB, auf der (noch) nicht die eigene Firmware läuft: Werks-Firmware (2345:e06e ohne „dazi“ im Produktnamen) oder
/// ROM-Download-Modus (4c4a:2342). Liest nur /sys (Linux); sonst None.
fn usb_ohne_custom() -> Option<UsbArt> {
    let eintraege = std::fs::read_dir("/sys/bus/usb/devices").ok()?;
    for e in eintraege.flatten() {
        let lies = |n: &str| std::fs::read_to_string(e.path().join(n)).map(|t| t.trim().to_string()).unwrap_or_default();
        match (lies("idVendor").as_str(), lies("idProduct").as_str()) {
            ("2345", "e06e") if !lies("product").contains("dazi") => return Some(UsbArt::Werk),
            ("4c4a", "2342") => return Some(UsbArt::Download),
            _ => {}
        }
    }
    None
}

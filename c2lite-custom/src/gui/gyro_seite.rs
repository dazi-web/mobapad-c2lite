//! Seite „Gyro“ (nur Chitu 2 HD, Firmware ab chitu-CG3): Live-Werte des Bewegungssensors so, wie sie an die Switch
//! gehen, und ein geführter Test (Lagen und Drehachsen werden abgehakt). Logik in crate::gyro.

use super::arbeiter::Auftrag;
use super::stil::{farben, hinweis, karte, titel};
use super::App;
use crate::gyro::{Lage, Probe, Pruefung};
use crate::sprache::{t, tr};
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Vec2};
use egui_phosphor::regular as icon;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Verlauf der Drehrate: etwa 4 s bei ~30 Abfragen je Sekunde
const VERLAUF: usize = 120;
/// Skala der Balken und des Verlaufs (°/s)
const SKALA_DPS: f32 = 500.0;

/// Achsfarben X, Y, Z (rot, grün, blau wie üblich; in hell und dunkel lesbar)
const ACHSFARBEN: [Color32; 3] =
    [Color32::from_rgb(0xe0, 0x5a, 0x5a), Color32::from_rgb(0x3f, 0xb3, 0x68), Color32::from_rgb(0x4a, 0x8f, 0xe0)];

#[derive(Default)]
pub struct GyroZustand {
    /// Antwort beim Öffnen der Seite: 1 bereit, 0 wird eingestellt, -1 kein Sensor; Err = kein Chitu/zu alte Firmware
    pub sensor: Option<Result<i32, String>>,
    pub probe: Option<Probe>,
    pub pruefung: Pruefung,
    verlauf: VecDeque<[f32; 3]>,
    letzte: Option<Instant>,
    /// Lage im Raum für die 3D-Ansicht
    lage: Lage,
}

impl GyroZustand {
    pub fn neu(&mut self, p: Probe) {
        let jetzt = Instant::now();
        let dt = self.letzte.map_or(0.0, |l| jetzt.duration_since(l).as_secs_f32());
        // Nullpunkt erst abziehen, wenn er in Ruhe gemessen ist
        let null = if self.pruefung.ruhe { self.pruefung.ruhe_mittel } else { [0.0; 3] };
        self.lage.neu(&p, dt, null);
        self.pruefung.neu(&p, jetzt);
        if p.gueltig {
            if self.verlauf.len() >= VERLAUF {
                self.verlauf.pop_front();
            }
            self.verlauf.push_back(p.g_dps());
        }
        self.probe = Some(p);
        self.letzte = Some(jetzt);
    }
}

impl App {
    pub(super) fn gyro_seite(&mut self, ui: &mut egui::Ui) {
        self.kopf(
            ui,
            "Gyro",
            "Bewegungssensor des Chitu 2 HD, Werte und Achsen so, wie sie an die Switch gehen. Liest nur, solange diese Seite offen ist.",
        );
        if !self.ist_chitu() {
            hinweis(ui, "Nur am Chitu 2 HD: dieser Controller meldet sich nicht als Chitu.");
            return;
        }
        let c = farben(ui.ctx());
        match &self.gyro.sensor {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t("Sensor wird abgefragt …"));
                });
                return;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(crate::sprache::t_meldung(e)).color(c.fehler));
                return;
            }
            Some(Ok(1)) => {}
            Some(Ok(stand)) => {
                let text = if *stand == 0 {
                    "Der Sensor wird gerade eingestellt. In ein paar Sekunden erneut versuchen."
                } else {
                    "Kein Bewegungssensor gefunden (oder aufgegeben). Controller neu starten und erneut versuchen."
                };
                ui.label(RichText::new(t(text)).color(c.warnung));
                ui.add_space(8.0);
                if ui.button(format!("{}  {}", icon::ARROWS_CLOCKWISE, t("Erneut versuchen"))).clicked() {
                    self.gyro.sensor = None;
                    let _ = self.tx.send(Auftrag::GyroLive(true));
                }
                return;
            }
        }
        // Daten veraltet? (Abfrage hängt, z. B. Schieber auf Switch gestellt)
        if self.gyro.letzte.is_some_and(|l| l.elapsed() > Duration::from_secs(1)) {
            hinweis(ui, "Keine neuen Werte seit über 1 s.");
        }
        let probe = self.gyro.probe.unwrap_or_default();
        karte(ui, |ui| {
            ui.horizontal(|ui| {
                titel(ui, "Lage in 3D");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(format!("{}  {}", icon::CROSSHAIR, t("Ausrichten"))).clicked() {
                        self.gyro.lage.ausrichten(&probe);
                    }
                });
            });
            hinweis(ui, "Folgt dem Controller. Neigen und Rollen hält die Schwerkraft fest, das Drehen um die Hochachse driftet langsam: „Ausrichten“ mit den Schultertasten vom Körper weg.");
            ui.add_space(4.0);
            dreidee(ui, &self.gyro.lage, c.linie, c.akzent, c.schwach);
        });
        ui.add_space(12.0);
        ui.columns(2, |sp| {
            karte(&mut sp[0], |ui| {
                titel(ui, "Neigung");
                hinweis(ui, "Blase zeigt die hohe Seite. Oben = Schultertasten.");
                ui.add_space(6.0);
                wasserwaage(ui, &probe, c.linie, c.akzent, c.schwach);
                let a = probe.a_g();
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "X {:+5.2}   Y {:+5.2}   Z {:+5.2} g   |a| {:4.2}",
                        a[0], a[1], a[2], probe.betrag_g()
                    ))
                    .monospace()
                    .color(c.schwach),
                );
            });
            karte(&mut sp[1], |ui| {
                titel(ui, "Drehrate");
                hinweis(ui, "Balken bis ±500 °/s, darunter die letzten 4 Sekunden.");
                ui.add_space(6.0);
                let d = probe.g_dps();
                for i in 0..3 {
                    balken(ui, ["X", "Y", "Z"][i], d[i], ACHSFARBEN[i], c.linie, c.erhoeht);
                }
                ui.add_space(8.0);
                verlauf(ui, &self.gyro.verlauf, c.linie);
            });
        });
        ui.add_space(12.0);
        karte(ui, |ui| {
            let schritte = self.gyro.pruefung.schritte();
            let n = schritte.iter().filter(|s| s.1).count();
            let fertig = self.gyro.pruefung.fertig();
            ui.horizontal(|ui| {
                titel(ui, "Test");
                ui.label(RichText::new(tr("{} von {} erledigt", &[&n, &schritte.len()])).strong());
                if fertig {
                    ui.label(RichText::new(format!("{}  {}", icon::CHECK_CIRCLE, t("Gyro in Ordnung"))).color(c.akzent));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t("Zurücksetzen")).clicked() {
                        self.gyro.pruefung = Pruefung::default();
                    }
                });
            });
            hinweis(ui, "In beliebiger Reihenfolge. Drehen zählt ab 90 °/s, also zügig.");
            ui.add_space(6.0);
            let pr = &self.gyro.pruefung;
            for (i, (text, ok)) in pr.schritte().iter().enumerate() {
                ui.horizontal(|ui| {
                    let (zeichen, farbe) = if *ok { (icon::CHECK_CIRCLE, c.akzent) } else { (icon::CIRCLE, c.schwach) };
                    ui.label(RichText::new(zeichen).size(18.0).color(farbe));
                    ui.label(t(text));
                    // Drehachsen: Farbe der Achse und größter Wert
                    if i >= 3 {
                        let a = i - 3;
                        ui.label(
                            RichText::new(format!("{}  {:.0} °/s", ["X", "Y", "Z"][a], pr.max_dps[a]))
                                .monospace()
                                .color(ACHSFARBEN[a]),
                        );
                    }
                });
            }
            ui.add_space(6.0);
            let m = pr.ruhe_mittel;
            ui.label(
                RichText::new(tr(
                    "Proben {}, ungültig {}, Nullpunkt in Ruhe {} °/s",
                    &[&pr.proben, &pr.ungueltig, &format!("{:+.1} {:+.1} {:+.1}", m[0], m[1], m[2])],
                ))
                .small()
                .color(c.schwach),
            );
        });
        ui.add_space(8.0);
        hinweis(ui, "Die Drehachsen: X = Rollen (wie ein Lenkrad), Y = Kippen nach vorn/hinten, Z = Drehen flach auf dem Tisch.");
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

/// Kreis mit Blase: Blase wandert zur hohen Seite (wie eine Dosenlibelle); Bild von oben, Schultertasten oben
fn wasserwaage(ui: &mut egui::Ui, p: &Probe, linie: Color32, blase: Color32, kreuz: Color32) {
    let groesse = 150.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), groesse), Sense::hover());
    let mitte = rect.center();
    let r = groesse / 2.0 - 4.0;
    let painter = ui.painter_at(rect);
    painter.circle_stroke(mitte, r, Stroke::new(1.5, linie));
    painter.circle_stroke(mitte, r * 0.25, Stroke::new(1.0, linie));
    painter.line_segment([mitte - Vec2::new(r, 0.0), mitte + Vec2::new(r, 0.0)], Stroke::new(1.0, kreuz.gamma_multiply(0.4)));
    painter.line_segment([mitte - Vec2::new(0.0, r), mitte + Vec2::new(0.0, r)], Stroke::new(1.0, kreuz.gamma_multiply(0.4)));
    if !p.gueltig {
        return;
    }
    let a = p.a_g();
    // hohe Seite: Schultertasten unten → a_x < 0 → Blase nach unten; linke Seite unten → a_y < 0 → Blase nach rechts
    let mut v = Vec2::new(-a[1], -a[0]);
    if v.length() > 1.0 {
        v = v.normalized();
    }
    let liegt_oben = a[2] >= 0.0;
    let farbe = if liegt_oben { blase } else { blase.gamma_multiply(0.4) };
    painter.circle_filled(mitte + v * (r - 10.0), 10.0, farbe);
}

/// Waagrechter Balken um die Mitte, ±SKALA_DPS
fn balken(ui: &mut egui::Ui, name: &str, dps: f32, farbe: Color32, linie: Color32, grund: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).monospace().color(farbe));
        let breite = (ui.available_width() - 90.0).max(60.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(breite, 14.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, grund);
        let mitte = rect.center().x;
        let anteil = (dps / SKALA_DPS).clamp(-1.0, 1.0);
        let ende = mitte + anteil * rect.width() / 2.0;
        let fuell = egui::Rect::from_x_y_ranges(mitte.min(ende)..=mitte.max(ende), rect.y_range());
        painter.rect_filled(fuell, 2.0, farbe);
        painter.line_segment([egui::pos2(mitte, rect.top()), egui::pos2(mitte, rect.bottom())], Stroke::new(1.0, linie));
        ui.label(RichText::new(format!("{dps:+7.1}")).monospace());
    });
}

/// Linien der drei Achsen über die letzten Proben
fn verlauf(ui: &mut egui::Ui, werte: &VecDeque<[f32; 3]>, linie: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 90.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, linie), egui::StrokeKind::Inside);
    painter.line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, linie));
    if werte.len() < 2 {
        return;
    }
    let schritt = rect.width() / (VERLAUF - 1) as f32;
    let start = rect.right() - schritt * (werte.len() - 1) as f32;
    for achse in 0..3 {
        let punkte: Vec<egui::Pos2> = werte
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let y = rect.center().y - (w[achse] / SKALA_DPS).clamp(-1.0, 1.0) * (rect.height() / 2.0 - 2.0);
                egui::pos2(start + i as f32 * schritt, y)
            })
            .collect();
        painter.add(egui::Shape::line(punkte, Stroke::new(1.5, ACHSFARBEN[achse])));
    }
}

/// Umriss des Controllers von oben (X = Schultertasten, Y links), geglättet; einmal berechnet samt Dreiecken
struct Umriss {
    punkte: Vec<[f32; 2]>,
    dreiecke: Vec<[usize; 3]>,
}

/// Stützpunkte im Uhrzeigersinn um einen Pro-Controller-artigen Körper mit zwei Griffen
const STUETZ: [[f32; 2]; 20] = [
    [0.30, 0.45], [0.315, 0.0], [0.30, -0.45], [0.22, -0.72], [0.02, -0.83], [-0.32, -0.81], [-0.60, -0.72],
    [-0.72, -0.56], [-0.66, -0.40], [-0.42, -0.30], [-0.25, -0.16], [-0.22, 0.0], [-0.25, 0.16], [-0.42, 0.30],
    [-0.66, 0.40], [-0.72, 0.56], [-0.60, 0.72], [-0.32, 0.81], [0.02, 0.83], [0.22, 0.72],
];

fn umriss() -> &'static Umriss {
    static U: std::sync::OnceLock<Umriss> = std::sync::OnceLock::new();
    U.get_or_init(|| {
        // geschlossene Catmull-Rom-Kurve durch die Stützpunkte
        let n = STUETZ.len();
        let mut punkte = Vec::new();
        for i in 0..n {
            let [p0, p1, p2, p3] = [STUETZ[(i + n - 1) % n], STUETZ[i], STUETZ[(i + 1) % n], STUETZ[(i + 2) % n]];
            for k in 0..6 {
                let t = k as f32 / 6.0;
                let (t2, t3) = (t * t, t * t * t);
                let f = |a: usize| {
                    0.5 * (2.0 * p1[a]
                        + (-p0[a] + p2[a]) * t
                        + (2.0 * p0[a] - 5.0 * p1[a] + 4.0 * p2[a] - p3[a]) * t2
                        + (-p0[a] + 3.0 * p1[a] - 3.0 * p2[a] + p3[a]) * t3)
                };
                punkte.push([f(0), f(1)]);
            }
        }
        // gegen den Uhrzeigersinn (von oben, +Z) ordnen, dann Ohren abschneiden
        if flaeche(&punkte) < 0.0 {
            punkte.reverse();
        }
        let dreiecke = ohren(&punkte);
        Umriss { punkte, dreiecke }
    })
}

fn flaeche(p: &[[f32; 2]]) -> f32 {
    (0..p.len()).map(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum::<f32>() / 2.0
}

/// Dreiecke eines einfachen Vielecks gegen den Uhrzeigersinn (Ohrenschneiden)
fn ohren(p: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let kreuz = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut rest: Vec<usize> = (0..p.len()).collect();
    let mut aus = Vec::new();
    let mut runden = 0;
    while rest.len() > 3 && runden < 10_000 {
        runden += 1;
        let n = rest.len();
        let mut gefunden = false;
        for i in 0..n {
            let (ia, ib, ic) = (rest[(i + n - 1) % n], rest[i], rest[(i + 1) % n]);
            let (a, b, c) = (p[ia], p[ib], p[ic]);
            if kreuz(a, b, c) <= 1e-9 {
                continue; // nach innen geknickt
            }
            let drin = rest.iter().any(|&j| {
                j != ia && j != ib && j != ic && kreuz(a, b, p[j]) > 0.0 && kreuz(b, c, p[j]) > 0.0 && kreuz(c, a, p[j]) > 0.0
            });
            if !drin {
                aus.push([ia, ib, ic]);
                rest.remove(i);
                gefunden = true;
                break;
            }
        }
        if !gefunden {
            break;
        }
    }
    if rest.len() == 3 {
        aus.push([rest[0], rest[1], rest[2]]);
    }
    aus
}

/// Blickrichtung: Spieler hinter dem Controller, leicht von oben und etwas seitlich
const HOEHE: f32 = 0.55; // rad, ≈ 32°
const SEITE: f32 = 0.35; // rad, ≈ 20°

/// Welt → Bildschirm (rechts, oben) und Tiefe (größer = weiter weg)
fn projizieren(w: [f32; 3]) -> (f32, f32, f32) {
    let (sa, ca) = SEITE.sin_cos();
    let (se, ce) = HOEHE.sin_cos();
    let x = w[0] * ca - w[1] * sa;
    let y = w[0] * sa + w[1] * ca;
    (-y, w[2] * ce + x * se, x * ce - w[2] * se)
}

fn schattieren(f: Color32, n_welt: [f32; 3]) -> Color32 {
    let l = [-0.35f32, 0.3, 0.88];
    let ln = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
    let d = (n_welt[0] * l[0] + n_welt[1] * l[1] + n_welt[2] * l[2]) / ln;
    let hell = 0.42 + 0.58 * d.max(0.0);
    Color32::from_rgb((f.r() as f32 * hell) as u8, (f.g() as f32 * hell) as u8, (f.b() as f32 * hell) as u8)
}

/// Unterkante und Oberseite des Körpers
const Z_UNTEN: f32 = -0.13;
const Z_OBEN: f32 = 0.10;

/// Controller als Körper (Umriss mit Dicke), Schultertasten, Bedienelemente auf der Oberseite; Achsen klein in der Ecke
fn dreidee(ui: &mut egui::Ui, lage: &Lage, linie: Color32, akzent: Color32, schwach: Color32) {
    let hoehe = 270.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), hoehe), Sense::hover());
    let painter = ui.painter_at(rect);
    let mitte = rect.center() - Vec2::new(0.0, 4.0);
    let mass = hoehe * 0.36;
    let bild_um = |w: [f32; 3], m: egui::Pos2, s: f32| {
        let (x, y, _) = projizieren(w);
        egui::pos2(m.x + x * s, m.y - y * s)
    };
    let bild = |w: [f32; 3]| bild_um(w, mitte, mass);
    let geraet = |p: [f32; 3]| bild(lage.welt(p));
    let sichtbar = |n: [f32; 3]| projizieren(lage.welt(n)).2 < 0.0;

    // Boden als Bezug
    let boden: Vec<egui::Pos2> = (0..64)
        .map(|i| {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            bild([a.cos() * 1.15, a.sin() * 1.15, -0.62])
        })
        .collect();
    painter.add(egui::Shape::closed_line(boden, Stroke::new(1.0, linie)));

    let koerper = Color32::from_rgb(0x74, 0x7a, 0x83);
    let u = umriss();
    let rand = Stroke::new(1.0, Color32::from_black_alpha(70));

    // Schultertasten: kleine Quader vorn, getrennt vom Körper sortiert
    let schulter = |y: f32| -> Vec<(f32, Vec<egui::Pos2>, Color32)> {
        let (m, h) = ([0.30f32, y, 0.03], [0.07f32, 0.2, 0.065]);
        let mut f = Vec::new();
        for achse in 0..3 {
            for vz in [-1.0f32, 1.0] {
                let mut n = [0.0f32; 3];
                n[achse] = vz;
                if !sichtbar(n) {
                    continue;
                }
                let (a1, a2) = ((achse + 1) % 3, (achse + 2) % 3);
                let ecken: Vec<[f32; 3]> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .iter()
                    .map(|(a, b)| {
                        let mut p = m;
                        p[achse] += vz * h[achse];
                        p[a1] += a * h[a1];
                        p[a2] += b * h[a2];
                        p
                    })
                    .collect();
                let tiefe = ecken.iter().map(|e| projizieren(lage.welt(*e)).2).sum::<f32>() / 4.0;
                f.push((tiefe, ecken.iter().map(|e| geraet(*e)).collect(), schattieren(akzent, lage.welt(n))));
            }
        }
        f
    };
    let schultern_tiefe = projizieren(lage.welt([0.30, 0.0, 0.03])).2;
    let koerper_tiefe = projizieren(lage.welt([-0.2, 0.0, 0.0])).2;
    let zeichne_schultern = |painter: &egui::Painter| {
        let mut f = schulter(0.5);
        f.extend(schulter(-0.5));
        f.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, e, c) in f {
            painter.add(egui::Shape::convex_polygon(e, c, rand));
        }
    };
    if schultern_tiefe > koerper_tiefe {
        zeichne_schultern(&painter);
    }

    // Seitenwände: je Kante ein Viereck, von hinten nach vorn
    let n = u.punkte.len();
    let mut waende: Vec<(f32, Vec<egui::Pos2>, Color32)> = Vec::new();
    for i in 0..n {
        let (a, b) = (u.punkte[i], u.punkte[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        let nrm = [dy / l, -dx / l, 0.0]; // außen (Umriss gegen den Uhrzeigersinn)
        if !sichtbar(nrm) {
            continue;
        }
        let ecken = [[a[0], a[1], Z_UNTEN], [b[0], b[1], Z_UNTEN], [b[0], b[1], Z_OBEN], [a[0], a[1], Z_OBEN]];
        let tiefe = ecken.iter().map(|e| projizieren(lage.welt(*e)).2).sum::<f32>() / 4.0;
        let farbe = schattieren(koerper, lage.welt(nrm));
        waende.push((tiefe, ecken.iter().map(|e| geraet(*e)).collect(), farbe));
    }
    waende.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, e, c) in waende {
        // ohne Rand: sonst zeichnet jede der vielen Kanten eine Linie
        painter.add(egui::Shape::convex_polygon(e, c, Stroke::NONE));
    }

    // Deckel (Ober- oder Unterseite) als Dreiecksnetz
    let oben = sichtbar([0.0, 0.0, 1.0]);
    let z = if oben { Z_OBEN } else { Z_UNTEN };
    let n_deckel = lage.welt([0.0, 0.0, if oben { 1.0 } else { -1.0 }]);
    let farbe = if oben {
        let c = schattieren(koerper, n_deckel);
        Color32::from_rgb(c.r().saturating_add(18), c.g().saturating_add(18), c.b().saturating_add(18))
    } else {
        schattieren(koerper, n_deckel)
    };
    let mut netz = egui::Mesh::default();
    for p in &u.punkte {
        netz.colored_vertex(geraet([p[0], p[1], z]), farbe);
    }
    for d in &u.dreiecke {
        netz.add_triangle(d[0] as u32, d[1] as u32, d[2] as u32);
    }
    painter.add(egui::Shape::mesh(netz));
    let umrandung: Vec<egui::Pos2> = u.punkte.iter().map(|p| geraet([p[0], p[1], z])).collect();
    painter.add(egui::Shape::closed_line(umrandung, Stroke::new(1.0, Color32::from_black_alpha(60))));

    if oben {
        let zo = Z_OBEN + 0.002;
        // Kreis auf der Oberseite (projiziert zur Ellipse)
        let scheibe = |c: [f32; 2], r: f32, h: f32, farbe: Color32| {
            let pkt: Vec<egui::Pos2> = (0..28)
                .map(|i| {
                    let a = i as f32 / 28.0 * std::f32::consts::TAU;
                    geraet([c[0] + r * a.cos(), c[1] + r * a.sin(), zo + h])
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(pkt, farbe, Stroke::NONE));
        };
        let rechteck = |c: [f32; 2], hx: f32, hy: f32, farbe: Color32| {
            let pkt: Vec<egui::Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(a, b)| geraet([c[0] + a * hx, c[1] + b * hy, zo]))
                .collect();
            painter.add(egui::Shape::convex_polygon(pkt, farbe, Stroke::NONE));
        };
        let mulde = Color32::from_rgb(0x2a, 0x2d, 0x32);
        let kappe = Color32::from_rgb(0x44, 0x48, 0x4f);
        // Sticks: Mulde, darüber die Kappe etwas erhöht
        for c in [[0.10, 0.44], [-0.22, -0.22]] {
            scheibe(c, 0.13, 0.0, mulde);
            scheibe(c, 0.085, 0.05, kappe);
        }
        // Steuerkreuz
        rechteck([-0.22, 0.22], 0.085, 0.028, mulde);
        rechteck([-0.22, 0.22], 0.028, 0.085, mulde);
        // A B X Y (Switch-Anordnung: X oben, A rechts, B unten, Y links)
        for (c, f) in [
            ([0.19, -0.44], Color32::from_rgb(0x4a, 0x8f, 0xe0)), // X
            ([0.10, -0.53], Color32::from_rgb(0xe0, 0x5a, 0x5a)), // A
            ([0.01, -0.44], Color32::from_rgb(0xe8, 0xc4, 0x4a)), // B
            ([0.10, -0.35], Color32::from_rgb(0x3f, 0xb3, 0x68)), // Y
        ] {
            scheibe(c, 0.038, 0.0, f);
        }
        // − und +, Aufnahme und Home
        rechteck([0.20, 0.17], 0.012, 0.035, schwach);
        rechteck([0.20, -0.17], 0.012, 0.035, schwach);
        rechteck([0.20, -0.17], 0.035, 0.012, schwach);
        rechteck([0.05, 0.10], 0.025, 0.025, mulde);
        scheibe([0.05, -0.10], 0.032, 0.0, mulde);
    }
    if schultern_tiefe <= koerper_tiefe {
        zeichne_schultern(&painter);
    }

    // Achsen klein unten links (gleiche Drehung wie der Controller)
    let ecke = egui::pos2(rect.left() + 46.0, rect.bottom() - 46.0);
    let null = bild_um(lage.welt([0.0; 3]), ecke, 32.0);
    for (i, name) in ["X", "Y", "Z"].iter().enumerate() {
        let mut e = [0.0f32; 3];
        e[i] = 1.0;
        let ende = bild_um(lage.welt(e), ecke, 32.0);
        painter.line_segment([null, ende], Stroke::new(2.0, ACHSFARBEN[i]));
        let schild = ende + (ende - null).normalized() * 8.0;
        painter.text(schild, egui::Align2::CENTER_CENTER, *name, egui::FontId::monospace(12.0), ACHSFARBEN[i]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn umriss_dreiecke() {
        let u = super::umriss();
        assert_eq!(u.punkte.len(), 120);
        assert_eq!(u.dreiecke.len(), u.punkte.len() - 2, "Ohrenschneiden unvollständig");
        assert!(super::flaeche(&u.punkte) > 0.0);
    }
}

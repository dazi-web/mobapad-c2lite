//! Controller-Bild für den Tastentest: Grundriss des C2 Lite (Switch-Anordnung), jede Taste zeigt ihren Zustand.
//! gedrückt = Akzentfläche, schon getestet = Akzentrand, sonst neutral. Koordinaten in einem 600 x 340-Raster.

use super::stil::{FETT, Farben};
use crate::geraet::Status;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

enum Form {
    Kreis(f32),
    Rechteck(f32, f32, u8),
}

/// (Rollen-Bit, Mitte, Form, Beschriftung)
const TEILE: &[(u8, (f32, f32), Form, &str)] = &[
    (8, (170.0, 25.0), Form::Rechteck(92.0, 22.0, 10), "ZL"),
    (9, (430.0, 25.0), Form::Rechteck(92.0, 22.0, 10), "ZR"),
    (6, (170.0, 52.0), Form::Rechteck(120.0, 22.0, 10), "L"),
    (7, (430.0, 52.0), Form::Rechteck(120.0, 22.0, 10), "R"),
    (10, (262.0, 104.0), Form::Rechteck(24.0, 12.0, 6), "−"),
    (11, (338.0, 104.0), Form::Rechteck(24.0, 12.0, 6), "+"),
    (15, (270.0, 142.0), Form::Rechteck(18.0, 18.0, 4), ""),
    (12, (330.0, 142.0), Form::Kreis(11.0), ""),
    (3, (420.0, 96.0), Form::Kreis(14.0), "X"),
    (1, (449.0, 125.0), Form::Kreis(14.0), "A"),
    (0, (420.0, 154.0), Form::Kreis(14.0), "B"),
    (4, (391.0, 125.0), Form::Kreis(14.0), "Y"),
    (32, (235.0, 172.0), Form::Rechteck(20.0, 22.0, 4), ""),
    (34, (235.0, 218.0), Form::Rechteck(20.0, 22.0, 4), ""),
    (35, (212.0, 195.0), Form::Rechteck(22.0, 20.0, 4), ""),
    (33, (258.0, 195.0), Form::Rechteck(22.0, 20.0, 4), ""),
    // Rückseite und Sondertasten, unter dem Gehäuse
    (18, (150.0, 312.0), Form::Rechteck(96.0, 26.0, 8), "Rück 1"),
    (19, (250.0, 312.0), Form::Rechteck(96.0, 26.0, 8), "Rück 2"),
    (16, (350.0, 312.0), Form::Rechteck(96.0, 26.0, 8), "M"),
    (17, (450.0, 312.0), Form::Rechteck(96.0, 26.0, 8), "Licht"),
];

pub fn zeichnen(ui: &mut egui::Ui, s: Option<&Status>, getestet: u64, c: Farben) {
    let breite = ui.available_width().min(760.0);
    let k = breite / 600.0;
    let (antwort, painter) = ui.allocate_painter(Vec2::new(breite, 340.0 * k), egui::Sense::hover());
    let o = antwort.rect.min;
    let p = |x: f32, y: f32| Pos2::new(o.x + x * k, o.y + y * k);
    let gedrueckt = |bit: u8| s.is_some_and(|s| s.gedrueckt(bit));
    let getestet_bit = |bit: u8| getestet & (1u64 << bit) != 0;

    // Gehäuse: zwei Griffe und der Körper in derselben Fläche, dünner Rand
    let koerper = c.erhoeht;
    for (x, y) in [(165.0, 232.0), (435.0, 232.0)] {
        painter.circle_filled(p(x, y), 66.0 * k, koerper);
    }
    let rumpf = Rect::from_min_max(p(92.0, 70.0), p(508.0, 238.0));
    painter.rect_filled(rumpf, CornerRadius::same((64.0 * k) as u8), koerper);
    let beschriftung = |pos: Pos2, text: &str, farbe: Color32, groesse: f32| {
        painter.text(pos, Align2::CENTER_CENTER, text, FontId::new(groesse * k, FontFamily::Name(FETT.into())), farbe);
    };
    beschriftung(p(300.0, 288.0), "Rückseite", c.schwach, 11.0);

    for (bit, (x, y), form, text) in TEILE {
        let an = gedrueckt(*bit);
        let fill = if an { c.akzent } else { c.flaeche };
        let rand = if an || getestet_bit(*bit) { Stroke::new(2.0, c.akzent) } else { Stroke::new(1.0, c.linie) };
        let mitte = p(*x, *y);
        match form {
            Form::Kreis(r) => {
                painter.circle_filled(mitte, r * k, fill);
                painter.circle_stroke(mitte, r * k, rand);
            }
            Form::Rechteck(w, h, r) => {
                let rect = Rect::from_center_size(mitte, Vec2::new(w * k, h * k));
                painter.rect_filled(rect, CornerRadius::same(*r), fill);
                painter.rect_stroke(rect, CornerRadius::same(*r), rand, StrokeKind::Inside);
            }
        }
        if !text.is_empty() {
            beschriftung(mitte, text, if an { c.akzent_text } else { c.text }, 13.0);
        }
    }

    // Sticks: Ring, Kappe an der Live-Position, gedrückt (L3/R3) = Akzent
    for (bit, (x, y), ax) in [(13u8, (180.0, 125.0), 0usize), (14u8, (365.0, 195.0), 2usize)] {
        let mitte = p(x, y);
        let an = gedrueckt(bit);
        let rand = if an || getestet_bit(bit) { Stroke::new(2.0, c.akzent) } else { Stroke::new(1.0, c.linie) };
        painter.circle_filled(mitte, 36.0 * k, c.flaeche);
        painter.circle_stroke(mitte, 36.0 * k, rand);
        let (dx, dy) = s.map_or((0.0, 0.0), |s| {
            ((s.achsen[ax] as f32 - 512.0) / 512.0, (s.achsen[ax + 1] as f32 - 512.0) / 512.0)
        });
        let kappe = mitte + Vec2::new(dx.clamp(-1.0, 1.0), dy.clamp(-1.0, 1.0)) * 20.0 * k;
        painter.circle_filled(kappe, 15.0 * k, if an { c.akzent } else { c.linie });
    }
}

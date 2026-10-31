//! Aussehen: Geist (OFL, assets/fonts) und Phosphor-Icons, helles und dunkles Theme nach Systemeinstellung,
//! ein Akzent (Smaragd), überall derselbe Radius.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme};
use std::sync::Arc;

pub const RADIUS: u8 = 8;
pub const FETT: &str = "geist-fett";

/// Farben eines Themes, damit eigene Zeichnungen (Controller-Bild) zu den Widgets passen
#[derive(Clone, Copy)]
pub struct Farben {
    pub grund: Color32,
    pub flaeche: Color32,
    pub erhoeht: Color32,
    pub linie: Color32,
    pub text: Color32,
    pub schwach: Color32,
    pub akzent: Color32,
    pub akzent_text: Color32,
    pub akzent_schwach: Color32,
    pub warnung: Color32,
    pub fehler: Color32,
}

pub const DUNKEL: Farben = Farben {
    grund: Color32::from_rgb(0x12, 0x14, 0x16),
    flaeche: Color32::from_rgb(0x19, 0x1c, 0x1f),
    erhoeht: Color32::from_rgb(0x22, 0x26, 0x2a),
    linie: Color32::from_rgb(0x2f, 0x34, 0x39),
    text: Color32::from_rgb(0xe8, 0xea, 0xec),
    schwach: Color32::from_rgb(0x9a, 0xa1, 0xa8),
    akzent: Color32::from_rgb(0x3c, 0xc8, 0x92),
    akzent_text: Color32::from_rgb(0x08, 0x1c, 0x14),
    akzent_schwach: Color32::from_rgb(0x1d, 0x3b, 0x30),
    warnung: Color32::from_rgb(0xe8, 0xb3, 0x4a),
    fehler: Color32::from_rgb(0xef, 0x6b, 0x6b),
};

pub const HELL: Farben = Farben {
    grund: Color32::from_rgb(0xf3, 0xf4, 0xf5),
    flaeche: Color32::from_rgb(0xfb, 0xfb, 0xfc),
    erhoeht: Color32::from_rgb(0xe9, 0xeb, 0xed),
    linie: Color32::from_rgb(0xd6, 0xda, 0xde),
    text: Color32::from_rgb(0x17, 0x1a, 0x1d),
    schwach: Color32::from_rgb(0x58, 0x60, 0x68),
    akzent: Color32::from_rgb(0x12, 0x80, 0x58),
    akzent_text: Color32::from_rgb(0xfb, 0xfb, 0xfc),
    akzent_schwach: Color32::from_rgb(0xd2, 0xee, 0xe2),
    warnung: Color32::from_rgb(0x9a, 0x62, 0x00),
    fehler: Color32::from_rgb(0xc0, 0x2a, 0x2a),
};

pub fn farben(ctx: &egui::Context) -> Farben {
    if ctx.global_style().visuals.dark_mode { DUNKEL } else { HELL }
}

pub fn einrichten(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();
    f.font_data.insert("geist".into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Geist-Regular.ttf"))));
    f.font_data.insert(FETT.into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Geist-SemiBold.ttf"))));
    f.font_data.insert("geist-mono".into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/GeistMono-Regular.ttf"))));
    f.families.entry(FontFamily::Proportional).or_default().insert(0, "geist".into());
    f.families.entry(FontFamily::Monospace).or_default().insert(0, "geist-mono".into());
    egui_phosphor::add_to_fonts(&mut f, egui_phosphor::Variant::Regular);
    // Fette Familie: Geist SemiBold, dahinter dieselben Ersatzschriften (Icons, Sonderzeichen)
    let mut fett = vec![FETT.to_string()];
    fett.extend(f.families[&FontFamily::Proportional].iter().skip(1).cloned());
    f.families.insert(FontFamily::Name(FETT.into()), fett);
    ctx.set_fonts(f);

    ctx.set_visuals_of(Theme::Dark, visuals(DUNKEL, true));
    ctx.set_visuals_of(Theme::Light, visuals(HELL, false));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(10.0, 10.0);
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.slider_width = 260.0;
        s.text_styles.insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(15.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(12.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace));
        s.text_styles.insert(TextStyle::Heading, FontId::new(24.0, FontFamily::Name(FETT.into())));
    });
}

fn visuals(c: Farben, dunkel: bool) -> egui::Visuals {
    let mut v = if dunkel { egui::Visuals::dark() } else { egui::Visuals::light() };
    let r = CornerRadius::same(RADIUS);
    v.panel_fill = c.grund;
    v.window_fill = c.flaeche;
    v.window_stroke = Stroke::new(1.0, c.linie);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = r;
    v.extreme_bg_color = c.flaeche;
    v.faint_bg_color = c.erhoeht;
    v.code_bg_color = c.erhoeht;
    v.text_edit_bg_color = Some(c.grund);
    v.weak_text_color = Some(c.schwach);
    v.hyperlink_color = c.akzent;
    v.warn_fg_color = c.warnung;
    v.error_fg_color = c.fehler;
    // Ausgewähltes: dezente Akzentfläche, Schrift in Akzentfarbe (gut lesbar in beiden Themes)
    v.selection.bg_fill = c.akzent_schwach;
    v.selection.stroke = Stroke::new(1.0, c.akzent);
    v.slider_trailing_fill = true;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c.flaeche;
    w.noninteractive.weak_bg_fill = c.flaeche;
    w.noninteractive.bg_stroke = Stroke::new(1.0, c.linie);
    w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    w.noninteractive.corner_radius = r;
    for (wv, fill, rand) in [
        (&mut w.inactive, c.erhoeht, c.linie),
        (&mut w.hovered, c.linie, c.schwach),
        (&mut w.active, c.akzent_schwach, c.akzent),
        (&mut w.open, c.erhoeht, c.linie),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(1.0, rand);
        wv.fg_stroke = Stroke::new(1.0, c.text);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    v
}

/// Auswahl-Schaltfläche (Segmente, Chips): immer mit Rahmen, ausgewählt in Akzentfarbe
pub fn wahl(an: bool, text: &str) -> egui::Button<'_> {
    egui::Button::new(crate::sprache::t(text)).selected(an).frame_when_inactive(true)
}

/// Schieberegler mit voller Akzentfarbe auf der gefüllten Strecke (die Auswahlfarbe ist bewusst dezent)
pub fn regler<N: egui::emath::Numeric>(ui: &mut egui::Ui, wert: &mut N, bereich: std::ops::RangeInclusive<N>, einheit: &str) -> egui::Response {
    let c = farben(ui.ctx());
    ui.scope(|ui| {
        ui.visuals_mut().selection.bg_fill = c.akzent;
        ui.add(egui::Slider::new(wert, bereich).suffix(einheit))
    })
    .inner
}

/// Hauptschaltfläche (Akzent)
pub fn knopf_haupt(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let c = farben(ui.ctx());
    ui.add(egui::Button::new(egui::RichText::new(crate::sprache::t(text)).color(c.akzent_text)).fill(c.akzent).corner_radius(RADIUS))
}

/// Karte: Fläche mit Rand, für zusammengehörige Einstellungen
pub fn karte<R>(ui: &mut egui::Ui, inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let c = farben(ui.ctx());
    egui::Frame::new()
        .fill(c.flaeche)
        .stroke(Stroke::new(1.0, c.linie))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            inhalt(ui)
        })
        .inner
}

pub fn titel(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(crate::sprache::t(text)).font(FontId::new(17.0, FontFamily::Name(FETT.into()))));
}

pub fn hinweis(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(crate::sprache::t(text)).color(farben(ui.ctx()).schwach));
}

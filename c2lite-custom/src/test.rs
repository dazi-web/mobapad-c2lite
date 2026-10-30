//! Live-Ansicht zum Testen: Tasten, Sticks, Schieber, Akku; LEDs und Vibration per Tastatur.

use crate::geraet::{Kanal, Status, SCHIEBER_PA3, SCHIEBER_PC3, TASTEN};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Points};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;
use std::time::{Duration, Instant};

struct Zustand {
    status: Status,
    getestet: u64, /* jede Taste, die schon einmal gedrückt war */
    meldung: String,
    lauflicht: Option<(Instant, u8)>,
    port: String,
}

pub fn starten(g: &mut dyn Kanal) -> Result<(), String> {
    let mut z = Zustand {
        status: Status::default(),
        getestet: 0,
        meldung: "Tasten drücken, Sticks bewegen. 1–4 Spieler-LED umschalten, a alle, l Lauflicht, v/b Vibration, q Ende"
            .into(),
        lauflicht: None,
        port: g.name(),
    };
    let mut terminal = ratatui::init();
    let ergebnis = schleife(g, &mut z, &mut terminal);
    ratatui::restore();
    let n = TASTEN.iter().filter(|(_, b)| z.getestet & (1u64 << b) != 0).count();
    println!("Getestet: {n} von {} Tasten.", TASTEN.len());
    ergebnis
}

fn schleife(g: &mut dyn Kanal, z: &mut Zustand, terminal: &mut ratatui::DefaultTerminal) -> Result<(), String> {
    loop {
        if let Ok(zeile) = g.befehl("status", &["status:"], Duration::from_millis(400)) {
            if let Some(s) = Status::lesen(&zeile) {
                z.getestet |= s.tasten;
                z.status = s;
            }
        }
        if let Some((start, schritt)) = z.lauflicht {
            let soll = (start.elapsed().as_millis() / 700) as u8;
            if soll != schritt {
                if soll >= 4 {
                    z.lauflicht = None;
                    let _ = g.befehl("led auto", &["ok:"], Duration::from_millis(400));
                    z.meldung = "Lauflicht fertig: Spieler-LED 1 bis 4 nacheinander.".into();
                } else {
                    let m = crate::spieler_bit(soll + 1);
                    let _ = g.befehl(&format!("led {m}"), &["ok:"], Duration::from_millis(400));
                    z.lauflicht = Some((start, soll));
                    z.meldung = format!("Lauflicht: jetzt leuchtet Spieler-LED {}", soll + 1);
                }
            }
        }
        terminal.draw(|f| zeichnen(f, z)).map_err(|e| e.to_string())?;
        while event::poll(Duration::from_millis(0)).map_err(|e| e.to_string())? {
            let Event::Key(k) = event::read().map_err(|e| e.to_string())? else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let led = |g: &mut dyn Kanal, m: u8| g.befehl(&format!("led {m}"), &["ok:"], Duration::from_millis(400));
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    let _ = g.befehl("led auto", &["ok:"], Duration::from_millis(400));
                    return Ok(());
                }
                KeyCode::Char(c @ '1'..='4') => {
                    let m = z.status.led ^ crate::spieler_bit(c as u8 - b'0');
                    let _ = led(g, m);
                    z.meldung = format!("Spieler-LED {c} umgeschaltet (Maske 0x{m:02x})");
                }
                KeyCode::Char('a') => {
                    let m = if z.status.led & 0x3c == 0x3c { 0 } else { 0x3c };
                    let _ = led(g, m);
                    z.meldung = if m != 0 { "Alle LEDs an".into() } else { "Alle LEDs aus".into() };
                }
                KeyCode::Char('l') => z.lauflicht = Some((Instant::now(), u8::MAX)),
                KeyCode::Char('v') => {
                    let _ = g.befehl("motor 60 0 500", &["ok:"], Duration::from_millis(400));
                    z.meldung = "Linker Motor 60 % für 0,5 s".into();
                }
                KeyCode::Char('b') => {
                    let _ = g.befehl("motor 0 60 500", &["ok:"], Duration::from_millis(400));
                    z.meldung = "Rechter Motor 60 % für 0,5 s".into();
                }
                _ => {}
            }
        }
    }
}

fn zeichnen(f: &mut Frame, z: &Zustand) {
    let [kopf, mitte, fuss] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(12), Constraint::Length(3)]).areas(f.area());
    let s = &z.status;
    let schieber = if s.gedrueckt(SCHIEBER_PC3) {
        "PC3"
    } else if s.gedrueckt(SCHIEBER_PA3) {
        "PA3"
    } else {
        "keine Stellung"
    };
    let n = TASTEN.iter().filter(|(_, b)| z.getestet & (1u64 << b) != 0).count();
    let kopfzeile = format!(
        "Akku {} %   USB {}   Schieber {}   LEDs 0x{:02x}   getestet {}/{}   ({})",
        s.akku,
        if s.usb { "an" } else { "aus" },
        schieber,
        s.led,
        n,
        TASTEN.len(),
        z.port
    );
    f.render_widget(Paragraph::new(kopfzeile).block(Block::bordered().title(" c2lite-custom – Controller-Test ")), kopf);

    let [links, tasten, rechts] =
        Layout::horizontal([Constraint::Length(26), Constraint::Min(30), Constraint::Length(26)]).areas(mitte);
    stick(f, links, " linker Stick ", s.achsen[0], s.achsen[1], s.gedrueckt(13));
    stick(f, rechts, " rechter Stick ", s.achsen[2], s.achsen[3], s.gedrueckt(14));

    let mut zeilen: Vec<Line> = Vec::new();
    for reihe in TASTEN.chunks(4) {
        let spans: Vec<Span> = reihe
            .iter()
            .flat_map(|(name, bit)| {
                let stil = if s.gedrueckt(*bit) {
                    Style::new().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                } else if z.getestet & (1u64 << bit) != 0 {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                [Span::styled(format!(" {name:^8} "), stil), Span::raw(" ")]
            })
            .collect();
        zeilen.push(Line::from(spans));
        zeilen.push(Line::raw(""));
    }
    f.render_widget(
        Paragraph::new(zeilen).block(Block::bordered().title(" Tasten (grün = schon getestet) ")),
        tasten,
    );
    f.render_widget(Paragraph::new(z.meldung.as_str()).wrap(Wrap { trim: true }).block(Block::bordered()), fuss);
}

fn stick(f: &mut Frame, flaeche: Rect, titel: &str, x: u16, y: u16, gedrueckt: bool) {
    // Achsen 0..1023, Mitte 512; „hoch“ liefert kleine Werte, daher y umdrehen
    let (px, py) = (x as f64 - 512.0, 512.0 - y as f64);
    let farbe = if gedrueckt { Color::Green } else { Color::Yellow };
    let canvas = Canvas::default()
        .block(Block::bordered().title(format!("{titel}{x:4} {y:4} ")))
        .x_bounds([-560.0, 560.0])
        .y_bounds([-560.0, 560.0])
        .paint(move |ctx| {
            ctx.draw(&Circle { x: 0.0, y: 0.0, radius: 512.0, color: Color::DarkGray });
            ctx.draw(&Points { coords: &[(px, py)], color: farbe });
            ctx.draw(&Circle { x: px, y: py, radius: 30.0, color: farbe });
        });
    f.render_widget(canvas, flaeche);
}

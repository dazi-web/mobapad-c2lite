//! Was die Oberfläche vom Controller weiß, gelesen aus den Textantworten der Firmware.

/// Tasten-Namen, wie die Firmware sie in `belegung` und `turbo` erwartet (c2_belegung.c, CODE_NAME)
pub const TASTEN: &[(&str, &str)] = &[
    ("a", "A"), ("b", "B"), ("x", "X"), ("y", "Y"),
    ("l", "L"), ("r", "R"), ("zl", "ZL"), ("zr", "ZR"),
    ("minus", "−"), ("plus", "+"), ("home", "Home"), ("kamera", "Kamera"),
    ("l3", "L3"), ("r3", "R3"),
    ("hoch", "↑"), ("rechts", "→"), ("runter", "↓"), ("links", "←"),
];
/// Stick-Richtungen (nur für Belegung und Makros)
pub const RICHTUNGEN: &[(&str, &str)] = &[
    ("lhoch", "L ↑"), ("lrechts", "L →"), ("lrunter", "L ↓"), ("llinks", "L ←"),
    ("rhoch", "R ↑"), ("rrechts", "R →"), ("rrunter", "R ↓"), ("rlinks", "R ←"),
];
pub const PLAETZE: [(&str, &str); 3] = [("rueck1", "Rücktaste 1"), ("rueck2", "Rücktaste 2"), ("m", "M-Taste")];
pub const LICHTMODI: &[(u8, &str)] =
    &[(2, "Regenbogen"), (3, "Farbwechsel"), (1, "Atmen"), (0, "Statisch"), (5, "Reaktiv"), (4, "Aus")];

pub fn anzeigename(taste: &str) -> &str {
    TASTEN.iter().chain(RICHTUNGEN).find(|(n, _)| *n == taste).map(|(_, a)| crate::sprache::t(a)).unwrap_or(taste)
}

/// `info:`-Zeile
#[derive(Clone, Default)]
pub struct Info {
    pub stand: String,
    pub gebaut: String,
    pub laufzeit_s: u32,
    pub reset: String,
    pub usb5v: bool,
    pub ble: bool,
    pub akku_mv: u32,
    pub akku_prozent: u8,
}

fn wert<'a>(z: &'a str, k: &str) -> Option<&'a str> {
    z.split_whitespace().find_map(|t| t.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
}

impl Info {
    pub fn lesen(z: &str) -> Option<Info> {
        let z = z.strip_prefix("info:")?;
        let gebaut = z
            .find("gebaut=")
            .map(|i| {
                let r = &z[i + 7..];
                r[..r.find(" laufzeit=").unwrap_or(r.len())].trim().to_string()
            })
            .unwrap_or_default();
        let (mv, pr) = wert(z, "akku")
            .and_then(|a| a.split_once("mV/"))
            .map(|(m, p)| (m.parse().unwrap_or(0), p.trim_end_matches('%').parse().unwrap_or(0)))
            .unwrap_or((0, 0));
        Some(Info {
            stand: wert(z, "stand").unwrap_or("?").to_string(),
            gebaut,
            laufzeit_s: wert(z, "laufzeit").and_then(|l| l.trim_end_matches('s').parse().ok()).unwrap_or(0),
            reset: wert(z, "reset").unwrap_or("?").to_string(),
            usb5v: wert(z, "usb5v") == Some("1"),
            ble: wert(z, "ble") == Some("1"),
            akku_mv: mv,
            akku_prozent: pr,
        })
    }
}

/// `einstellungen:`-Zeile
#[derive(Clone, PartialEq)]
pub struct Einstellungen {
    pub anzeige: bool,
    pub vibration: u8,
    pub totzone: u8,
    pub licht: u8,
    pub hell: u8,
    pub logo: u8,
    pub tempo: u8,
    pub farbe: [u8; 3],
}

impl Einstellungen {
    pub fn lesen(z: &str) -> Option<Einstellungen> {
        let z = z.strip_prefix("einstellungen:")?;
        let n = |k: &str| wert(z, k).and_then(|v| v.parse::<u8>().ok());
        let f = u32::from_str_radix(wert(z, "farbe")?, 16).ok()?;
        Some(Einstellungen {
            anzeige: n("anzeige")? == 1,
            vibration: n("vibration")?,
            totzone: n("totzone")?,
            licht: n("licht")?,
            hell: n("hell")?,
            logo: n("logo")?,
            tempo: n("tempo")?,
            farbe: [(f >> 16) as u8, (f >> 8) as u8, f as u8],
        })
    }

    pub fn farbe_zahl(&self) -> u32 {
        (self.farbe[0] as u32) << 16 | (self.farbe[1] as u32) << 8 | self.farbe[2] as u32
    }

    /// Befehle, die genau diesen Stand wiederherstellen (für Backups)
    pub fn befehle(&self) -> Vec<String> {
        vec![
            format!("setze anzeige {}", self.anzeige as u8),
            format!("setze vibration {}", self.vibration),
            format!("setze totzone {}", self.totzone),
            format!("setze licht {}", self.licht),
            format!("setze hell {}", self.hell),
            format!("setze logo {}", self.logo),
            format!("setze tempo {}", self.tempo.max(1)),
            format!("setze farbe {}", self.farbe_zahl()),
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Art {
    #[default]
    Aus,
    Taste,
    Makro,
}

#[derive(Clone, PartialEq)]
pub struct Schritt {
    pub tasten: Vec<String>,
    pub halten: u32,
    pub pause: u32,
}

impl Default for Schritt {
    fn default() -> Self {
        Schritt { tasten: vec!["a".into()], halten: 100, pause: 100 }
    }
}

#[derive(Clone, PartialEq)]
pub struct Platz {
    pub art: Art,
    pub tasten: Vec<String>,
    pub schritte: Vec<Schritt>,
    pub wiederholen: String, /* einmal | endlos | halten */
    pub intervall: u32,
}

impl Default for Platz {
    fn default() -> Self {
        Platz { art: Art::Aus, tasten: vec![], schritte: vec![], wiederholen: "einmal".into(), intervall: 800 }
    }
}

pub const MAX_SCHRITTE: usize = 80;

fn tasten_text(t: &[String]) -> String {
    if t.is_empty() { "a".into() } else { t.join("+") }
}

impl Platz {
    /// Befehle, die diesen Platz in der Firmware setzen; Makros in Zeilen unter 500 Zeichen (Grenze 600)
    pub fn befehle(&self, name: &str) -> Vec<String> {
        match self.art {
            Art::Aus => vec![format!("belegung {name} aus")],
            Art::Taste => vec![format!("belegung {name} taste {}", tasten_text(&self.tasten))],
            Art::Makro => {
                let mut v = Vec::new();
                let mut zeile = format!("belegung {name} makro");
                let mut leer = true;
                for s in &self.schritte {
                    let t = format!(" {}:{}:{}", tasten_text(&s.tasten), s.halten, s.pause);
                    if zeile.len() + t.len() > 480 {
                        v.push(zeile);
                        zeile = format!("belegung {name} makro+");
                    }
                    zeile.push_str(&t);
                    leer = false;
                }
                if leer {
                    zeile.push_str(" a:100:100");
                }
                v.push(zeile);
                v.push(format!("belegung {name} loop {}", self.wiederholen));
                v.push(format!("belegung {name} intervall {}", self.intervall));
                v
            }
        }
    }
}

/// Belegung aller drei Plätze und Turbo, aus den Zeilen von `belegung`
#[derive(Clone, Default)]
pub struct Belegung {
    pub plaetze: [Platz; 3],
    pub turbo_tempo: u8,
    pub turbo: Vec<(String, u8)>, /* Taste oder Platz -> 1 halten, 2 auto */
}

pub const TURBO_MODI: [&str; 3] = ["aus", "halten", "auto"];

impl Belegung {
    pub fn lesen(zeilen: &[String]) -> Belegung {
        let mut b = Belegung::default();
        for z in zeilen {
            if let Some(r) = z.strip_prefix("belegung:") {
                let r = r.trim();
                let Some((name, rest)) = r.split_once(' ') else { continue };
                let Some(i) = PLAETZE.iter().position(|(n, _)| *n == name) else { continue };
                let p = &mut b.plaetze[i];
                if let Some(w) = rest.strip_prefix("= ") {
                    if w == "aus" {
                        *p = Platz::default();
                    } else if let Some(m) = w.strip_prefix("makro") {
                        p.art = Art::Makro;
                        p.schritte.clear();
                        p.wiederholen = wert(m, "loop").unwrap_or("einmal").to_string();
                        p.intervall = wert(m, "intervall").and_then(|v| v.parse().ok()).unwrap_or(800);
                    } else {
                        p.art = Art::Taste;
                        p.tasten = w.split('+').map(str::to_string).collect();
                    }
                } else if let Some((_, schritte)) = rest.split_once(' ') {
                    // „<erster Schritt> a+b:200:100 x:50:50 ...“
                    for schritt in schritte.split_whitespace() {
                        let mut teile = schritt.split(':');
                        let tasten = teile.next().unwrap_or("").split('+').map(str::to_string).collect();
                        let halten = teile.next().and_then(|v| v.parse().ok()).unwrap_or(100);
                        let pause = teile.next().and_then(|v| v.parse().ok()).unwrap_or(100);
                        p.schritte.push(Schritt { tasten, halten, pause });
                    }
                }
            } else if let Some(r) = z.strip_prefix("turbo:") {
                for t in r.split_whitespace() {
                    if let Some((k, v)) = t.split_once('=') {
                        if k == "tempo" {
                            b.turbo_tempo = v.parse().unwrap_or(1);
                        } else if let Some(m) = TURBO_MODI.iter().position(|x| *x == v) {
                            b.turbo.push((k.to_string(), m as u8));
                        }
                    }
                }
            }
        }
        b
    }

    pub fn turbo_von(&self, taste: &str) -> u8 {
        self.turbo.iter().find(|(t, _)| t == taste).map(|(_, m)| *m).unwrap_or(0)
    }

    pub fn befehle(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (i, (name, _)) in PLAETZE.iter().enumerate() {
            v.extend(self.plaetze[i].befehle(name));
        }
        v.push(format!("turbo tempo {}", self.turbo_tempo));
        for (taste, _) in TASTEN.iter().map(|(n, a)| (*n, *a)).chain(PLAETZE.iter().copied()) {
            v.push(format!("turbo {taste} {}", TURBO_MODI[self.turbo_von(taste) as usize]));
        }
        v
    }
}

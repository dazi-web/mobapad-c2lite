//! Sprache der Oberfläche und der Meldungen: Deutsch (Vorlage) oder Englisch. Die deutschen Texte stehen im Code, die
//! englischen in `uebersetzung.rs`. Fehlt eine Übersetzung, bleibt der deutsche Text (nichts geht verloren).
//! Wahl: C2LITE_SPRACHE=de|en, sonst die in der Oberfläche gespeicherte Wahl (~/.config/c2lite-custom/sprache), sonst
//! Deutsch nur bei LC_ALL/LC_MESSAGES/LANG „de…“, sonst Englisch. C2LITE_SPRACHE_PRUEFEN=1 schreibt jeden Text ohne Übersetzung einmal auf stderr (Fehlersuche).

use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// 0 = noch nicht bestimmt, 1 = Deutsch, 2 = Englisch
static WAHL: AtomicU8 = AtomicU8::new(0);

fn aus_umgebung() -> bool {
    if let Ok(s) = std::env::var("C2LITE_SPRACHE") {
        return !s.to_ascii_lowercase().starts_with("de");
    }
    if let Some(s) = datei().and_then(|p| std::fs::read_to_string(p).ok()) {
        match s.trim() {
            "de" => return false,
            "en" => return true,
            _ => {}
        }
    }
    for v in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(s) = std::env::var(v) {
            if !s.is_empty() {
                return !s.to_ascii_lowercase().starts_with("de");
            }
        }
    }
    true
}

/// Englisch aktiv?
pub fn englisch() -> bool {
    match WAHL.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let e = aus_umgebung();
            WAHL.store(if e { 2 } else { 1 }, Ordering::Relaxed);
            e
        }
    }
}

/// Umschalten zur Laufzeit (Oberfläche)
pub fn setzen(englisch: bool) {
    WAHL.store(if englisch { 2 } else { 1 }, Ordering::Relaxed);
}

/// Gespeicherte Wahl: $XDG_CONFIG_HOME bzw. ~/.config/c2lite-custom/sprache
fn datei() -> Option<std::path::PathBuf> {
    let basis = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(basis.join("c2lite-custom").join("sprache"))
}

/// Umschalten und für den nächsten Start merken (Fehler beim Speichern sind egal)
pub fn waehlen(englisch: bool) {
    setzen(englisch);
    if let Some(p) = datei() {
        if let Some(o) = p.parent() {
            let _ = std::fs::create_dir_all(o);
        }
        let _ = std::fs::write(p, if englisch { "en\n" } else { "de\n" });
    }
}

fn tabelle() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| crate::uebersetzung::TEXTE.iter().copied().collect())
}

/// Schon englisch (z. B. ein übersetzter Text, der noch einmal durch eine Hilfsfunktion läuft)?
fn schon_englisch(text: &str) -> bool {
    static W: OnceLock<HashSet<&'static str>> = OnceLock::new();
    W.get_or_init(|| crate::uebersetzung::TEXTE.iter().map(|(_, en)| *en).collect()).contains(text)
}

/// Fehlende Übersetzung melden (nur mit C2LITE_SPRACHE_PRUEFEN); ignoriert Texte ohne Wort mit Kleinbuchstaben (Zahlen,
/// Symbole, „A“) und Texte mit einem schon übersetzten Teil hinter einem Symbol („⬇  Install“)
fn fehlt(de: &str) {
    let ohne_symbol = de.split("  ").last().unwrap_or(de);
    if schon_englisch(de) || schon_englisch(ohne_symbol) || !de.chars().any(|c| c.is_lowercase()) {
        return;
    }
    static PRUEFEN: OnceLock<bool> = OnceLock::new();
    if !*PRUEFEN.get_or_init(|| std::env::var_os("C2LITE_SPRACHE_PRUEFEN").is_some()) {
        return;
    }
    static GEMELDET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut g = GEMELDET.get_or_init(Default::default).lock().unwrap();
    if g.insert(de.to_string()) {
        eprintln!("UEBERSETZUNG FEHLT: {de:?}");
    }
}

/// Text in der aktiven Sprache (Schlüssel = deutscher Text)
pub fn t(de: &str) -> &str {
    if !englisch() || de.trim().is_empty() {
        return de;
    }
    match tabelle().get(de) {
        Some(en) => en,
        None => {
            fehlt(de);
            de
        }
    }
}

/// Wie `t`, mit Platzhaltern „{}“ in der Reihenfolge von `werte`
pub fn tr(de: &str, werte: &[&dyn Display]) -> String {
    let vorlage = t(de);
    let mut aus = String::with_capacity(vorlage.len() + 16);
    let mut rest = vorlage;
    let mut i = 0;
    while let Some(p) = rest.find("{}") {
        aus.push_str(&rest[..p]);
        if let Some(w) = werte.get(i) {
            aus.push_str(&w.to_string());
        }
        i += 1;
        rest = &rest[p + 2..];
    }
    aus.push_str(rest);
    aus
}

/// Passt `de` auf ein Muster mit „{}“? Liefert die eingesetzten Werte (der letzte Platzhalter nimmt den Rest, die übrigen
/// so wenig wie möglich)
fn muster(vorlage: &str, de: &str) -> Option<Vec<String>> {
    let teile: Vec<&str> = vorlage.split("{}").collect();
    if teile.len() < 2 {
        return None;
    }
    let mut rest = de.strip_prefix(teile[0])?;
    let mut werte = Vec::new();
    for (i, teil) in teile[1..].iter().enumerate() {
        let letzter = i == teile.len() - 2;
        let p = if letzter {
            if !rest.ends_with(teil) {
                return None;
            }
            rest.len() - teil.len()
        } else if teil.is_empty() {
            return None;
        } else {
            rest.find(teil)?
        };
        if p == 0 {
            return None;
        }
        werte.push(rest[..p].to_string());
        rest = &rest[p + teil.len()..];
    }
    Some(werte)
}

/// Meldung mit eingesetzten Werten (Installer-Kern, Fehler): erst genau, dann ein Muster mit „{}“ (die Werte werden ihrerseits
/// übersetzt, falls möglich), dann der längste Tabellen-Anfang, der mit „: “ oder einem Leerzeichen endet
/// (z. B. „Sicherung gespeichert: …“)
pub fn t_meldung(de: &str) -> String {
    t_meldung_innen(de, true)
}

fn t_meldung_innen(de: &str, melden: bool) -> String {
    if !englisch() {
        return de.to_string();
    }
    if let Some(en) = tabelle().get(de) {
        return en.to_string();
    }
    let mut treffer: Option<(&str, Vec<String>)> = None;
    for k in tabelle().keys() {
        if k.contains("{}") && treffer.as_ref().is_none_or(|(b, _)| k.len() > b.len()) {
            if let Some(w) = muster(k, de) {
                treffer = Some((k, w));
            }
        }
    }
    if let Some((k, werte)) = treffer {
        // Werte sind kürzer als der Satz: die Rekursion endet
        let werte: Vec<String> = werte.iter().map(|w| t_meldung_innen(w, false)).collect();
        let anzeige: Vec<&dyn Display> = werte.iter().map(|w| w as &dyn Display).collect();
        return tr(k, &anzeige);
    }
    let mut best: Option<(&str, &str)> = None;
    for (k, v) in tabelle() {
        if (k.ends_with(": ") || k.ends_with(' ')) && de.starts_with(k) && best.is_none_or(|(b, _)| k.len() > b.len()) {
            best = Some((k, v));
        }
    }
    match best {
        Some((k, v)) => format!("{v}{}", &de[k.len()..]),
        None => {
            if melden {
                fehlt(de);
            }
            de.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn platzhalter_und_rueckfall() {
        super::setzen(false);
        assert_eq!(super::tr("Auf Stufe {} aktualisieren", &[&"13a"]), "Auf Stufe 13a aktualisieren");
        super::setzen(true);
        assert_eq!(super::t("gibt es sicher nicht 123"), "gibt es sicher nicht 123");
        super::setzen(false);
    }

    #[test]
    fn muster_erkennen() {
        assert_eq!(super::muster("Abgebrochen: {}", "Abgebrochen: x y"), Some(vec!["x y".to_string()]));
        assert_eq!(super::muster("{} (Protokoll: {})", "Fehler a (Protokoll: /p.log)"), Some(vec!["Fehler a".into(), "/p.log".into()]));
        assert_eq!(super::muster("Stufe {}", "Stufe "), None);
        assert_eq!(super::muster("Stufe {}", "Etwas anderes"), None);
    }

    /// Jede Übersetzung hat dieselben Platzhalter wie das Original, kein Schlüssel doppelt
    #[test]
    fn tabelle_stimmig() {
        let mut gesehen = std::collections::HashSet::new();
        for (de, en) in crate::uebersetzung::TEXTE {
            assert!(gesehen.insert(*de), "doppelt: {de}");
            assert_eq!(de.matches("{}").count(), en.matches("{}").count(), "Platzhalter: {de}");
            assert!(!en.trim().is_empty(), "leer: {de}");
        }
    }
}

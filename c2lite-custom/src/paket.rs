//! Im Programm eingebaute Firmware: der Stand von release/ im Repo zur Bauzeit. jl_isd.bin für die Erstinstallation
//! (Seite „Installieren“), db_update_data.bin für Updates eines laufenden C2 Lite Custom (Seite „Firmware“).
//! Der Stand steht in der ersten Zeile von release/LIESMICH.txt („C2 Lite Custom, Stufe 11c (…)“).

/// Abbild für die Erstinstallation (Dual-Bank, jl_isd.bin)
#[cfg(target_os = "linux")]
pub const JL_ISD: &[u8] = include_bytes!("../../release/jl_isd.bin");
/// Update für einen laufenden C2 Lite Custom (db_update_data.bin)
pub const UPDATE: &[u8] = include_bytes!("../../release/db_update_data.bin");
const LIESMICH: &str = include_str!("../../release/LIESMICH.txt");

/// z. B. „C2 Lite Custom, Stufe 11c“
pub fn stand() -> &'static str {
    let z = LIESMICH.lines().next().unwrap_or("C2 Lite Custom");
    z.split(" (").next().unwrap_or(z)
}

/// z. B. „11c“ (so meldet die Firmware ihren Stand in „info“)
pub fn stufe() -> &'static str {
    stand().rsplit("Stufe ").next().unwrap_or("").trim()
}

/// Stufen vergleichen: Zahl, dann Buchstabe („9h“ < „11c“ < „13“ < „13a“). Unlesbares gilt als älter.
pub fn stufe_vergleich(a: &str, b: &str) -> std::cmp::Ordering {
    fn teile(s: &str) -> (u32, String) {
        let ziffern: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        (ziffern.parse().unwrap_or(0), s[ziffern.len()..].to_string())
    }
    teile(a).cmp(&teile(b))
}

#[cfg(test)]
mod tests {
    use super::stufe_vergleich;
    use std::cmp::Ordering::*;

    #[test]
    fn stufen() {
        assert_eq!(stufe_vergleich("9h", "11c"), Less);
        assert_eq!(stufe_vergleich("11c", "13"), Less);
        assert_eq!(stufe_vergleich("13", "13a"), Less);
        assert_eq!(stufe_vergleich("13", "13"), Equal);
        assert_eq!(stufe_vergleich("13", "11c"), Greater);
    }
}

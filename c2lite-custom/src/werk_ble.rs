//! Bluetooth LE zur WERKS-Firmware der C2 Lite vom PC aus (Linux), für die Erstinstallation. Die Werks-Firmware meldet
//! beim Koppeln die unzulässige Schlüssellänge 1 (Linux/BlueZ lehnt ab), rechnet aber mit dem vollen Schlüssel. Daher
//! übernimmt dieses Modul den Adapter exklusiv (HCI-User-Kanal, CAP_NET_ADMIN, BlueZ vorher aus) und koppelt selbst:
//! LE Legacy „Just Works“ (TK = 0), ungekürzter Kurzzeitschlüssel. Danach ATT: Dienst FF00 (App-Befehle FF01/FF02) und
//! FF10 (offizieller Update-Dienst wie die Mobapad-App, OtaUpgrade.java). Am Gerät erprobt am 30.10. (Prototyp
//! eines Prototyps).

/// Technisches Protokoll auf stderr (stdout gehört dem Installer für seine Fortschrittszeilen)
macro_rules! protokoll {
    ($($t:tt)*) => { eprintln!($($t)*) };
}

use aes::Aes128;
use aes::cipher::{BlockCipherEncrypt, KeyInit};
use std::io::{Error, ErrorKind};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- SMP-Krypto (alle Werte in Funkreihenfolge, LE)

/// e(k, d) der Spezifikation (MSB zuerst) auf LE-Arrays wie BlueZ smp_e
fn e(k: &[u8; 16], d: &[u8; 16]) -> [u8; 16] {
    let mut kb = *k;
    kb.reverse();
    let mut db = *d;
    db.reverse();
    let c = Aes128::new(&kb.into());
    let mut blk = db.into();
    c.encrypt_block(&mut blk);
    let mut r: [u8; 16] = blk.into();
    r.reverse();
    r
}

fn xor(a: &[u8; 16], b: &[u8; 16]) -> [u8; 16] {
    std::array::from_fn(|i| a[i] ^ b[i])
}

/// c1 (Bestätigungswert), ia/ra in Funkreihenfolge, iat/rat Adresstyp (0 öffentlich, 1 zufällig)
#[allow(clippy::too_many_arguments)]
fn c1(k: &[u8; 16], r: &[u8; 16], preq: &[u8; 7], pres: &[u8; 7], iat: u8, ia: &[u8; 6], rat: u8, ra: &[u8; 6]) -> [u8; 16] {
    let mut p1 = [0u8; 16];
    p1[0] = iat;
    p1[1] = rat;
    p1[2..9].copy_from_slice(preq);
    p1[9..16].copy_from_slice(pres);
    let mut p2 = [0u8; 16];
    p2[0..6].copy_from_slice(ra);
    p2[6..12].copy_from_slice(ia);
    let t = e(k, &xor(r, &p1));
    e(k, &xor(&t, &p2))
}

/// s1 (Kurzzeitschlüssel STK): je die unteren 64 Bit von r1 (Antwortender) und r2 (Einleitender)
fn s1(k: &[u8; 16], r1: &[u8; 16], r2: &[u8; 16]) -> [u8; 16] {
    let mut r = [0u8; 16];
    r[0..8].copy_from_slice(&r2[0..8]);
    r[8..16].copy_from_slice(&r1[0..8]);
    e(k, &r)
}

// ---------------------------------------------------------------- HCI-User-Kanal

const AF_BLUETOOTH: i32 = 31;
const BTPROTO_HCI: i32 = 1;
const HCI_CHANNEL_USER: u16 = 1;

#[repr(C)]
struct SockaddrHci {
    family: u16,
    dev: u16,
    channel: u16,
}

struct Hci {
    fd: i32,
    puffer: Vec<u8>,
    acl_max: usize,
    /// freie ACL-Puffer im Bluetooth-Chip (Flusskontrolle über „Number of Completed Packets“)
    guthaben: usize,
    /// beim Warten auf Guthaben gelesene Pakete, die noch abgeholt werden
    wartend: std::collections::VecDeque<(u8, Vec<u8>)>,
}

impl Drop for Hci {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

impl Hci {
    fn oeffnen(dev: u16) -> std::io::Result<Hci> {
        let fd = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_RAW | libc::SOCK_CLOEXEC, BTPROTO_HCI) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        let a = SockaddrHci { family: AF_BLUETOOTH as u16, dev, channel: HCI_CHANNEL_USER };
        let r = unsafe { libc::bind(fd, &a as *const _ as *const libc::sockaddr, std::mem::size_of::<SockaddrHci>() as u32) };
        if r < 0 {
            let e = Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        Ok(Hci { fd, puffer: Vec::new(), acl_max: 27, guthaben: 4, wartend: Default::default() })
    }

    fn schreiben(&self, d: &[u8]) -> std::io::Result<()> {
        let n = unsafe { libc::write(self.fd, d.as_ptr() as *const _, d.len()) };
        if n as usize != d.len() {
            return Err(Error::last_os_error());
        }
        Ok(())
    }

    /// Ein H4-Paket lesen (Typ, Nutzdaten), höchstens bis `bis`
    fn lesen(&mut self, bis: Instant) -> std::io::Result<Option<(u8, Vec<u8>)>> {
        if let Some(p) = self.wartend.pop_front() {
            return Ok(Some(p));
        }
        self.roh_lesen(bis)
    }

    /// Nächstes Paket vom Chip; „Number of Completed Packets“ wird hier als Guthaben verbucht und nicht gemeldet
    fn roh_lesen(&mut self, bis: Instant) -> std::io::Result<Option<(u8, Vec<u8>)>> {
        loop {
            match self.naechstes(bis)? {
                Some(p) if self.guthaben_buchen(&p) => continue,
                r => return Ok(r),
            }
        }
    }

    /// „Number of Completed Packets“ als Guthaben verbuchen; true, wenn es so ein Ereignis war
    fn guthaben_buchen(&mut self, p: &(u8, Vec<u8>)) -> bool {
        if p.0 != 0x04 || p.1[0] != 0x13 {
            return false;
        }
        for i in 0..p.1[2] as usize {
            self.guthaben += u16::from_le_bytes([p.1[5 + 4 * i], p.1[6 + 4 * i]]) as usize;
        }
        true
    }

    /// Nächstes rohe Paket vom Chip (alle Arten)
    fn naechstes(&mut self, bis: Instant) -> std::io::Result<Option<(u8, Vec<u8>)>> {
        loop {
            if let Some(p) = self.paket() {
                return Ok(Some(p));
            }
            let rest = bis.saturating_duration_since(Instant::now());
            if rest.is_zero() {
                return Ok(None);
            }
            let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
            let r = unsafe { libc::poll(&mut pfd, 1, rest.as_millis().min(1000) as i32) };
            if r < 0 {
                return Err(Error::last_os_error());
            }
            if r == 0 {
                continue;
            }
            let mut b = [0u8; 1100];
            let n = unsafe { libc::read(self.fd, b.as_mut_ptr() as *mut _, b.len()) };
            if n < 0 {
                return Err(Error::last_os_error());
            }
            self.puffer.extend_from_slice(&b[..n as usize]);
        }
    }

    fn paket(&mut self) -> Option<(u8, Vec<u8>)> {
        let b = &self.puffer;
        let (kopf, laenge) = match *b.first()? {
            0x04 if b.len() >= 3 => (3, b[2] as usize),
            0x02 if b.len() >= 5 => (5, u16::from_le_bytes([b[3], b[4]]) as usize),
            0x04 | 0x02 => return None,
            _ => {
                self.puffer.clear();
                return None;
            }
        };
        if b.len() < kopf + laenge {
            return None;
        }
        let typ = b[0];
        let p: Vec<u8> = b[1..kopf + laenge].to_vec();
        self.puffer.drain(..kopf + laenge);
        Some((typ, p))
    }

    /// Befehl senden und auf Command Complete/Status warten; Rückgabe: Parameter des Complete-Ereignisses
    fn befehl(&mut self, ogf: u16, ocf: u16, par: &[u8]) -> std::io::Result<Vec<u8>> {
        let op = (ogf << 10) | ocf;
        let mut d = vec![0x01, op as u8, (op >> 8) as u8, par.len() as u8];
        d.extend_from_slice(par);
        self.schreiben(&d)?;
        let bis = Instant::now() + Duration::from_secs(3);
        while let Some((typ, p)) = self.lesen(bis)? {
            if typ != 0x04 {
                continue;
            }
            match p[0] {
                0x0e if u16::from_le_bytes([p[3], p[4]]) == op => return Ok(p[5..].to_vec()),
                0x0f if u16::from_le_bytes([p[4], p[5]]) == op => {
                    if p[2] != 0 {
                        return Err(Error::other(format!("Befehl {op:04x}: Status {:02x}", p[2])));
                    }
                    return Ok(vec![0]);
                }
                _ => {}
            }
        }
        Err(Error::new(ErrorKind::TimedOut, format!("keine Antwort auf Befehl {op:04x}")))
    }

    /// L2CAP-Rahmen auf dem LE-Kanal senden (zerlegt in ACL-Pakete)
    fn l2cap(&mut self, handle: u16, cid: u16, nutz: &[u8]) -> std::io::Result<()> {
        let mut rahmen = Vec::with_capacity(nutz.len() + 4);
        rahmen.extend_from_slice(&(nutz.len() as u16).to_le_bytes());
        rahmen.extend_from_slice(&cid.to_le_bytes());
        rahmen.extend_from_slice(nutz);
        for (i, stueck) in rahmen.chunks(self.acl_max).enumerate() {
            let pb = if i == 0 { 0x0000 } else { 0x1000 };
            let h = handle | pb;
            let mut d = vec![0x02, h as u8, (h >> 8) as u8];
            d.extend_from_slice(&(stueck.len() as u16).to_le_bytes());
            d.extend_from_slice(stueck);
            let anfang = Instant::now();
            let bis = anfang + Duration::from_secs(120); /* der Controller löscht z. B. erst die Flash-Bank */
            let mut gemeldet = 0;
            while self.guthaben == 0 {
                let s = anfang.elapsed().as_secs();
                if s >= 2 && s != gemeldet {
                    gemeldet = s;
                    protokoll!("  warte auf den Controller ... {s} s");
                }
                /* sofort weiter, sobald ein Puffer frei wird (vorher wurde danach noch bis zur Frist gewartet) */
                match self.naechstes((Instant::now() + Duration::from_millis(500)).min(bis))? {
                    Some(p) if self.guthaben_buchen(&p) => {}
                    Some(p) => self.wartend.push_back(p),
                    None if Instant::now() < bis => {}
                    None => {
                        for (t, p) in &self.wartend {
                            protokoll!("  wartend: Typ {t:02x} {}", hex(&p[..p.len().min(24)]));
                        }
                        return Err(Error::new(ErrorKind::TimedOut, "Bluetooth-Chip nimmt keine Daten mehr an"));
                    }
                }
            }
            self.guthaben -= 1;
            self.schreiben(&d)?;
        }
        Ok(())
    }
}

/// Wartet auf einen L2CAP-Rahmen (cid) bzw. meldet wichtige Ereignisse; setzt ACL-Fragmente zusammen
struct Verbindung<'a> {
    hci: &'a mut Hci,
    handle: u16,
    teil: Vec<u8>,
    /// Meldungen (ATT-Notification-Werte), die während einer anderen Anfrage kamen
    meldungen: std::collections::VecDeque<Vec<u8>>,
}

enum Ankunft {
    L2cap(u16, Vec<u8>),
    Ereignis(Vec<u8>),
}

impl Verbindung<'_> {
    fn warten(&mut self, bis: Instant) -> std::io::Result<Option<Ankunft>> {
        while let Some((typ, p)) = self.hci.lesen(bis)? {
            if typ == 0x04 {
                if p[0] == 0x13 {
                    continue; // Number of Completed Packets
                }
                if p[0] == 0x3e && p[2] == 0x03 && p.len() >= 12 {
                    let iv = u16::from_le_bytes([p[6], p[7]]) as f32 * 1.25;
                    protokoll!("  (Verbindungsparameter jetzt: Intervall {iv} ms, Latenz {}, Timeout {} ms, Status {:02x})",
                        u16::from_le_bytes([p[8], p[9]]), u16::from_le_bytes([p[10], p[11]]) as u32 * 10, p[3]);
                    continue;
                }
                if p[0] == 0x3e && p[2] == 0x06 && p.len() >= 13 {
                    /* Der Controller will langsamere Parameter (LL-Prozedur): ablehnen, schnelles Intervall behalten */
                    protokoll!("  (Controller wünscht Intervall {}–{} ms, Latenz {}: abgelehnt)",
                        u16::from_le_bytes([p[5], p[6]]) as f32 * 1.25, u16::from_le_bytes([p[7], p[8]]) as f32 * 1.25,
                        u16::from_le_bytes([p[9], p[10]]));
                    self.hci.schreiben(&[0x01, 0x21, 0x20, 3, p[3], p[4], 0x3b])?;
                    continue;
                }
                return Ok(Some(Ankunft::Ereignis(p)));
            }
            let h = u16::from_le_bytes([p[0], p[1]]);
            if h & 0x0fff != self.handle {
                continue;
            }
            let daten = &p[4..];
            if (h >> 12) & 0x3 == 0x1 {
                self.teil.extend_from_slice(daten);
            } else {
                self.teil = daten.to_vec();
            }
            if self.teil.len() >= 4 {
                let l = u16::from_le_bytes([self.teil[0], self.teil[1]]) as usize;
                if self.teil.len() >= 4 + l {
                    let cid = u16::from_le_bytes([self.teil[2], self.teil[3]]);
                    let nutz = self.teil[4..4 + l].to_vec();
                    self.teil.clear();
                    if cid == 5 && nutz.len() >= 12 && nutz[0] == 0x12 {
                        /* L2CAP-Anfrage nach langsameren Verbindungsparametern: ablehnen (Ergebnis 1) */
                        protokoll!("  (Controller bittet um Intervall {}–{} ms, Latenz {}: abgelehnt)",
                            u16::from_le_bytes([nutz[4], nutz[5]]) as f32 * 1.25, u16::from_le_bytes([nutz[6], nutz[7]]) as f32 * 1.25,
                            u16::from_le_bytes([nutz[8], nutz[9]]));
                        self.hci.l2cap(self.handle, 5, &[0x13, nutz[1], 0x02, 0x00, 0x01, 0x00])?;
                        continue;
                    }
                    return Ok(Some(Ankunft::L2cap(cid, nutz)));
                }
            }
        }
        Ok(None)
    }

    fn smp(&mut self, bis: Instant) -> std::io::Result<Vec<u8>> {
        loop {
            match self.warten(bis)? {
                Some(Ankunft::L2cap(6, p)) if p[0] == 0x0b => protokoll!("  (Sicherheitsanfrage des Controllers: {})", hex(&p)),
                Some(Ankunft::L2cap(6, p)) => return Ok(p),
                Some(Ankunft::L2cap(4, p)) => {
                    protokoll!("  (ATT während des Koppelns: {})", hex(&p));
                    self.att_nebenbei(&p)?
                }
                Some(Ankunft::L2cap(c, p)) => protokoll!("  (L2CAP {c:04x}: {})", hex(&p)),
                Some(Ankunft::Ereignis(p)) if p[0] == 0x05 => {
                    return Err(Error::other(format!("Verbindung getrennt, Grund {:02x}", p.get(5).copied().unwrap_or(0))))
                }
                Some(Ankunft::Ereignis(p)) => protokoll!("  (Ereignis {})", hex(&p)),
                None => return Err(Error::new(ErrorKind::TimedOut, "keine SMP-Antwort")),
            }
        }
    }

    /// Anfragen der Gegenseite während wir warten (z. B. MTU) kurz beantworten
    fn att_nebenbei(&mut self, p: &[u8]) -> std::io::Result<()> {
        match p.first() {
            Some(0x02) => self.hci.l2cap(self.handle, 4, &[0x03, 247, 0]), // MTU-Anfrage: 247
            Some(&op) if op & 0x01 == 0 && op != 0x1b && op != 0x52 && op < 0x30 => {
                self.hci.l2cap(self.handle, 4, &[0x01, op, 0, 0, 0x06]) // Anfrage nicht unterstützt
            }
            _ => Ok(()),
        }
    }

    fn att(&mut self, anfrage: &[u8], bis: Instant) -> std::io::Result<Vec<u8>> {
        self.hci.l2cap(self.handle, 4, anfrage)?;
        loop {
            match self.warten(bis)? {
                Some(Ankunft::L2cap(4, p)) if p[0] == 0x1b => self.meldungen.push_back(p[3..].to_vec()),
                Some(Ankunft::L2cap(4, p)) if p[0] == anfrage[0] + 1 || p[0] == 0x01 => return Ok(p),
                Some(Ankunft::L2cap(4, p)) => self.att_nebenbei(&p)?,
                Some(Ankunft::Ereignis(p)) if p[0] == 0x05 => return Err(Error::other("Verbindung getrennt")),
                Some(_) => {}
                None => return Err(Error::new(ErrorKind::TimedOut, "keine ATT-Antwort")),
            }
        }
    }
}

/// Merkmale (16-Bit-UUID, Wert-Handle, Eigenschaften) eines Dienstes
fn merkmale(v: &mut Verbindung, a: u16, z: u16) -> std::io::Result<Vec<(u16, u16, u8)>> {
    let mut alle = Vec::new();
    let mut start = a;
    let bis = Instant::now() + Duration::from_secs(10);
    while start <= z {
        let r = v.att(&[0x08, start as u8, (start >> 8) as u8, z as u8, (z >> 8) as u8, 0x03, 0x28], bis)?;
        if r[0] != 0x09 {
            break;
        }
        for e in r[2..].chunks(r[1] as usize) {
            let decl = u16::from_le_bytes([e[0], e[1]]);
            if e.len() >= 7 {
                alle.push((u16::from_le_bytes([e[5], e[6]]), u16::from_le_bytes([e[3], e[4]]), e[2]));
            }
            start = decl + 1;
        }
    }
    Ok(alle)
}

/// CRC-16/XMODEM wie die Mobapad-App (Utils.crc)
fn crc16(d: &[u8]) -> u16 {
    let mut c: u16 = 0;
    for &b in d {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 };
        }
    }
    c
}

/// Rahmen des Update-Protokolls (KeyCommand.getRequestData): [Länge][ID low][ID high][Inhalt]
fn ota_rahmen(id: u16, inhalt: &[u8]) -> Vec<u8> {
    let mut r = vec![(inhalt.len() + 3) as u8, id as u8, (id >> 8) as u8];
    r.extend_from_slice(inhalt);
    r
}

fn ota_antwort(v: &mut Verbindung, id: u8, bis: Instant) -> std::io::Result<Vec<u8>> {
    while let Some(d) = v.meldungen.pop_front() {
        if d.len() >= 3 && d[1] == id {
            return Ok(d);
        }
        protokoll!("  (Meldung {})", hex(&d));
    }
    loop {
        match v.warten(bis)? {
            Some(Ankunft::L2cap(4, p)) if p[0] == 0x1b => {
                let d = &p[3..];
                if d.len() >= 3 && d[1] == id {
                    return Ok(d.to_vec());
                }
                protokoll!("  (Meldung {})", hex(d));
            }
            Some(Ankunft::L2cap(4, p)) => v.att_nebenbei(&p)?,
            Some(Ankunft::Ereignis(ev)) if ev[0] == 0x05 => return Err(Error::other(format!("Verbindung getrennt (Grund {:02x})", ev.get(4).copied().unwrap_or(0)))),
            Some(_) => {}
            None => return Err(Error::new(ErrorKind::TimedOut, format!("keine Antwort auf Update-Befehl {id:02x}"))),
        }
    }
}

/// Offizielles Update der Werks-Firmware (Dienst FF10, nach OtaUpgrade.java der Mobapad-App): Bereit (0x50) mit CRC und
/// Länge, Quittungsabstand (0x51), Daten in 100-Byte-Blöcken als 20-Byte-Schreibvorgänge auf FF12, Fertig (0x53)
fn ota(v: &mut Verbindung, a: u16, z: u16, fw: &[u8], fortschritt: &mut dyn FnMut(f32)) -> std::io::Result<()> {
    let m = merkmale(v, a, z)?;
    for (u, h, e) in &m {
        protokoll!("  Merkmal {u:04x} Handle {h:04x} Eigenschaften {e:02x}");
    }
    let wert = |u: u16| m.iter().find(|(x, _, _)| *x == u).map(|(_, h, e)| (*h, *e));
    let ((ff11, e11), (ff12, e12)) = (wert(0xff11).ok_or_else(|| Error::other("FF11 fehlt"))?, wert(0xff12).ok_or_else(|| Error::other("FF12 fehlt"))?);
    let bis = Instant::now() + Duration::from_secs(10);
    for h in [ff11] {
        let c = h + 1;
        let r = v.att(&[0x12, c as u8, (c >> 8) as u8, 0x01, 0x00], bis)?;
        protokoll!("Meldungen von Handle {h:04x} abonniert: {}", hex(&r));
    }
    /* wie Android: ohne Antwort nur, wenn das Merkmal es anbietet (Eigenschaft 0x04), sonst Schreiben mit Antwort */
    let schreiben = |v: &mut Verbindung, h: u16, d: &[u8]| -> std::io::Result<()> {
        let e = if h == ff11 { e11 } else { e12 };
        let mit_antwort = e & 0x04 == 0;
        let mut p = vec![if mit_antwort { 0x12 } else { 0x52 }, h as u8, (h >> 8) as u8];
        p.extend_from_slice(d);
        if mit_antwort {
            let r = v.att(&p, Instant::now() + Duration::from_secs(15))?;
            if r[0] != 0x13 {
                return Err(Error::other(format!("Schreiben auf {h:04x} abgelehnt: {}", hex(&r))));
            }
            Ok(())
        } else {
            v.hci.l2cap(v.handle, 4, &p)
        }
    };
    /* größere ATT-Pakete: MTU aushandeln; passt ein 100-Byte-Block der App in ein Paket, ihn am Stück schreiben */
    let r = v.att(&[0x02, 247, 0], Instant::now() + Duration::from_secs(5))?;
    let mtu = if r[0] == 0x03 && r.len() >= 3 { u16::from_le_bytes([r[1], r[2]]).min(247) as usize } else { 23 };
    let stueck = if std::env::var("C2_STUECK").is_ok() { 20 } else { (mtu - 3).min(100) };
    protokoll!("ATT-MTU {mtu}, schreibe {stueck} Byte pro Paket");
    let crc = crc16(fw);
    let n = fw.len();
    protokoll!("Update: {n} Byte, CRC16 {crc:04x}");
    schreiben(v, ff11, &ota_rahmen(0x50, &[(crc >> 8) as u8, crc as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8]))?;
    let r = ota_antwort(v, 0x50, Instant::now() + Duration::from_secs(15))?;
    protokoll!("Bereit: {}", hex(&r));
    if r[2] != 0 {
        return Err(Error::other("Controller lehnt das Update ab"));
    }
    schreiben(v, ff11, &ota_rahmen(0x51, &[]))?;
    let r = ota_antwort(v, 0x51, Instant::now() + Duration::from_secs(10))?;
    let abstand = if r.len() >= 5 { ((r[3] as usize) << 8) | r[4] as usize } else { 0 };
    protokoll!("Quittungsabstand {abstand} Byte");
    if abstand == 0 {
        return Err(Error::other("ungültiger Quittungsabstand"));
    }
    let start = Instant::now();
    let mut summe = 0usize;
    for (i, block) in fw.chunks(100).enumerate() {
        for teil in block.chunks(stueck) {
            schreiben(v, ff12, teil)?;
        }
        summe += 100;
        if summe % abstand == 0 {
            let r = ota_antwort(v, 0x52, Instant::now() + Duration::from_secs(15))?;
            if r.len() > 3 && r[3] != 0 {
                return Err(Error::other(format!("Datenquittung meldet Fehler bei {}: {}", i * 100, hex(&r))));
            }
        } else {
            std::thread::sleep(Duration::from_millis(30)); /* wie die App zwischen Blöcken ohne Quittung */
        }
        fortschritt((i * 100 + block.len()) as f32 / n as f32);
    }
    let _ = start;
    schreiben(v, ff11, &ota_rahmen(0x53, &[]))?;
    let r = ota_antwort(v, 0x53, Instant::now() + Duration::from_secs(30))?;
    protokoll!("Fertig: {}", hex(&r));
    if r[2] != 0 {
        return Err(Error::other("Controller meldet Fehler beim Abschluss"));
    }
    protokoll!("UPDATE ÜBERTRAGEN, der Controller startet neu. Adapter wieder an: bluetoothctl power on");
    Ok(())
}


fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn zufall16() -> [u8; 16] {
    let mut r = [0u8; 16];
    let f = std::fs::File::open("/dev/urandom").expect("urandom");
    use std::io::Read;
    (&f).read_exact(&mut r).expect("urandom");
    r
}


fn koppeln(hci: &mut Hci, ziel: &[u8; 6], ia: &[u8; 6], variante: &str) -> std::io::Result<u16> {
    let ziel_kopie = *ziel;
    // Verbinden (öffentliche Adresse; der Controller muss im Kopplungsmodus werben)
    let mut p = Vec::new();
    p.extend_from_slice(&0x60u16.to_le_bytes()); // Scan-Intervall
    p.extend_from_slice(&0x30u16.to_le_bytes()); // Scan-Fenster
    p.push(0); // keine Filterliste
    p.push(0); // Gegenstelle öffentlich
    p.extend_from_slice(&ziel_kopie);
    p.push(0); // eigene Adresse öffentlich
    for v in [0x0cu16, 0x18, 0, 0x1f4, 0, 0] {
        p.extend_from_slice(&v.to_le_bytes());
    }
    hci.befehl(0x08, 0x000d, &p)?;
    protokoll!("Verbinde ... (Controller in den Kopplungsmodus bringen, ich warte 2 Minuten)");
    let bis = Instant::now() + Duration::from_secs(120);
    let mut handle = None;
    while let Some((typ, ev)) = hci.lesen(bis)? {
        if typ == 0x04 && ev[0] == 0x3e && ev[2] == 0x01 {
            if ev[3] != 0 {
                return Err(Error::other(format!("Verbindung fehlgeschlagen, Status {:02x}", ev[3])));
            }
            handle = Some(u16::from_le_bytes([ev[4], ev[5]]) & 0x0fff);
            if ev.len() >= 20 {
                protokoll!("  (Verbindung: Intervall {} ms, Latenz {}, Timeout {} ms)", u16::from_le_bytes([ev[14], ev[15]]) as f32 * 1.25,
                    u16::from_le_bytes([ev[16], ev[17]]), u16::from_le_bytes([ev[18], ev[19]]) as u32 * 10);
            }
            break;
        }
    }
    let Some(handle) = handle else {
        let _ = hci.befehl(0x08, 0x000e, &[]);
        return Err(Error::other("Controller nicht erreicht (2 min)"));
    };
    protokoll!("verbunden, Handle {handle:04x}");
    let mut v = Verbindung { hci, handle, teil: Vec::new(), meldungen: Default::default() };
    let r = koppeln_verbunden(&mut v, ziel, ia, variante);
    if r.is_err() {
        let _ = v.hci.befehl(0x01, 0x0006, &[handle as u8, (handle >> 8) as u8, 0x13]);
        std::thread::sleep(Duration::from_millis(500));
    }
    r.map(|_| handle)
}

fn koppeln_verbunden(v: &mut Verbindung, ziel: &[u8; 6], ia: &[u8; 6], variante: &str) -> std::io::Result<()> {
    let handle = v.handle;
    let (ziel, ia) = (*ziel, *ia);

    // Koppeln: Legacy Just Works, keine Schlüsselverteilung (wir brauchen nur die Verschlüsselung)
    let preq = [0x01, 0x03, 0x00, 0x00, 16, 0x00, 0x00];
    v.hci.l2cap(handle, 6, &preq)?;
    let pres_v = v.smp(Instant::now() + Duration::from_secs(10))?;
    if pres_v[0] != 0x02 {
        return Err(Error::other(format!("statt Pairing Response: {}", hex(&pres_v))));
    }
    let pres: [u8; 7] = pres_v[..7].try_into().unwrap();
    let laenge = pres[4].clamp(1, 16) as usize;
    protokoll!("Pairing Response {} (Schlüssellänge {laenge})", hex(&pres));
    let tk = [0u8; 16];
    let mrand = zufall16();
    let mconf = c1(&tk, &mrand, &preq, &pres, 0, &ia, 0, &ziel);
    let mut d = vec![0x03];
    d.extend_from_slice(&mconf);
    v.hci.l2cap(handle, 6, &d)?;
    let sconf = v.smp(Instant::now() + Duration::from_secs(10))?;
    if sconf[0] != 0x03 {
        return Err(Error::other(format!("statt Pairing Confirm: {}", hex(&sconf))));
    }
    let mut d = vec![0x04];
    d.extend_from_slice(&mrand);
    v.hci.l2cap(handle, 6, &d)?;
    let srand_v = v.smp(Instant::now() + Duration::from_secs(10))?;
    if srand_v[0] != 0x04 {
        return Err(Error::other(format!("statt Pairing Random: {}", hex(&srand_v))));
    }
    let srand: [u8; 16] = srand_v[1..17].try_into().unwrap();
    if c1(&tk, &srand, &preq, &pres, 0, &ia, 0, &ziel)[..] != sconf[1..17] {
        return Err(Error::other("Bestätigungswert der Gegenstelle passt nicht"));
    }
    let mut stk = s1(&tk, &srand, &mrand);
    match variante {
        "kurz" => stk.iter_mut().skip(laenge).for_each(|b| *b = 0), /* Spezifikation: obere Bytes null */
        "hoch" => stk.iter_mut().take(16 - laenge).for_each(|b| *b = 0),
        _ => {}                                                      /* voll: ungekürzt */
    }
    let mut p = Vec::new();
    p.extend_from_slice(&handle.to_le_bytes());
    p.extend_from_slice(&[0u8; 8]); // Rand
    p.extend_from_slice(&[0u8; 2]); // EDIV
    p.extend_from_slice(&stk);
    v.hci.befehl(0x08, 0x0019, &p)?; // LE Enable Encryption
    let bis = Instant::now() + Duration::from_secs(10);
    loop {
        match v.warten(bis)? {
            Some(Ankunft::Ereignis(ev)) if ev[0] == 0x08 => {
                if ev[2] != 0 || ev[5] == 0 {
                    return Err(Error::other(format!("Verschlüsselung fehlgeschlagen: {}", hex(&ev))));
                }
                break;
            }
            Some(Ankunft::Ereignis(ev)) if ev[0] == 0x05 => return Err(Error::other("Verbindung getrennt")),
            Some(Ankunft::L2cap(4, p)) => v.att_nebenbei(&p)?,
            Some(_) => {}
            None => return Err(Error::other("keine Verschlüsselungsmeldung")),
        }
    }
    Ok(())
}



// ---------------------------------------------------------------- Schnittstelle für den Installer

/// Exklusiv übernommener Bluetooth-Adapter; beim Freigeben schaltet BlueZ ihn wieder ein
pub struct Werk {
    hci: Hci,
    ia: [u8; 6],
}

impl Drop for Werk {
    fn drop(&mut self) {
        let _ = std::process::Command::new("bluetoothctl").args(["power", "on"]).output();
    }
}

/// Adresse in Anzeigeform (AA:BB:…) aus Funkreihenfolge
pub fn adresse_text(a: &[u8; 6]) -> String {
    a.iter().rev().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

impl Werk {
    /// BlueZ gibt den Adapter frei, dann HCI-User-Kanal öffnen und den Chip einrichten (braucht CAP_NET_ADMIN/root)
    pub fn oeffnen() -> std::io::Result<Werk> {
        let _ = std::process::Command::new("bluetoothctl").args(["power", "off"]).output();
        let mut versuch = 0;
        let mut hci = loop {
            match Hci::oeffnen(0) {
                Err(e) if versuch < 15 && e.raw_os_error() != Some(libc::EPERM) => {
                    versuch += 1;
                    let _ = std::process::Command::new("bluetoothctl").args(["power", "off"]).output();
                    std::thread::sleep(Duration::from_secs(1));
                }
                Err(e) => return Err(Error::other(format!("Bluetooth-Adapter nicht exklusiv zu öffnen: {e}"))),
                Ok(h) => break h,
            }
        };
        hci.befehl(0x03, 0x0003, &[])?; // Reset
        hci.befehl(0x03, 0x0001, &0x3dbf_f807_fffb_ffffu64.to_le_bytes())?; // Event Mask
        hci.befehl(0x08, 0x0001, &0x3fu64.to_le_bytes())?; // LE Event Mask inkl. Parameterwunsch der Gegenseite
        let puf = hci.befehl(0x08, 0x0002, &[])?; // LE Read Buffer Size
        if puf.len() >= 4 && u16::from_le_bytes([puf[1], puf[2]]) >= 23 {
            hci.acl_max = u16::from_le_bytes([puf[1], puf[2]]) as usize;
            hci.guthaben = (puf[3] as usize).max(1);
        }
        let bd = hci.befehl(0x04, 0x0009, &[])?; // Read BD_ADDR
        let ia: [u8; 6] = bd[1..7].try_into().unwrap();
        protokoll!("Adapter {} übernommen, ACL {} Byte, {} Puffer", adresse_text(&ia), hci.acl_max, hci.guthaben);
        Ok(Werk { hci, ia })
    }

    /// Passive LE-Suche nach einer Werks-C2-Lite im Kopplungsmodus (Name „Mobapad C2-lite“); öffentliche Adresse
    pub fn suchen(&mut self, dauer: Duration) -> std::io::Result<Option<[u8; 6]>> {
        let mut p = vec![0u8]; // passiv
        p.extend_from_slice(&0x60u16.to_le_bytes());
        p.extend_from_slice(&0x30u16.to_le_bytes());
        p.extend_from_slice(&[0, 0]); // eigene Adresse öffentlich, keine Filterliste
        self.hci.befehl(0x08, 0x000b, &p)?;
        self.hci.befehl(0x08, 0x000c, &[1, 0])?;
        let bis = Instant::now() + dauer;
        let mut gefunden = None;
        'suche: while let Some((typ, ev)) = self.hci.lesen(bis)? {
            if typ != 0x04 || ev[0] != 0x3e || ev.get(2) != Some(&0x02) {
                continue;
            }
            let mut o = 4; // Anzahl bei ev[3]
            for _ in 0..ev[3] {
                if o + 9 > ev.len() {
                    break;
                }
                let adr_typ = ev[o + 1];
                let adr: [u8; 6] = ev[o + 2..o + 8].try_into().unwrap();
                let l = ev[o + 8] as usize;
                let daten = ev.get(o + 9..o + 9 + l).unwrap_or(&[]);
                o += 9 + l + 1;
                let mut i = 0;
                while i + 1 < daten.len() {
                    let n = daten[i] as usize;
                    if n == 0 || i + 1 + n > daten.len() {
                        break;
                    }
                    if matches!(daten[i + 1], 0x08 | 0x09) {
                        let name = String::from_utf8_lossy(&daten[i + 2..i + 1 + n]);
                        if name.contains("C2-lite") && adr_typ == 0 {
                            protokoll!("Gefunden: {} „{name}“", adresse_text(&adr));
                            gefunden = Some(adr);
                            break 'suche;
                        }
                    }
                    i += 1 + n;
                }
            }
        }
        let _ = self.hci.befehl(0x08, 0x000c, &[0, 0]);
        Ok(gefunden)
    }

    /// Verbinden, koppeln und Dienste suchen (bis zu 5 Versuche bei Funkaussetzern)
    pub fn verbinden(&mut self, ziel: &[u8; 6], hinweis: &mut dyn FnMut(&str)) -> std::io::Result<Sitzung<'_>> {
        let mut handle = None;
        for versuch in 1..=5 {
            match koppeln(&mut self.hci, ziel, &self.ia, "voll") {
                Ok(h) => {
                    handle = Some(h);
                    break;
                }
                Err(e) if (e.to_string().contains("Grund 3e") || e.to_string().contains("keine SMP-Antwort")) && versuch < 5 => {
                    protokoll!("Aufbau gestört ({e}), neuer Versuch");
                    if e.to_string().contains("keine SMP-Antwort") {
                        /* am Gerät: so verhält sich die Werks-Firmware mit gestecktem Kabel oder nach Ende des Kopplungsfensters */
                        hinweis("Der Controller hat das Koppeln nicht angenommen. USB-Kabel ab? Bitte den Kopplungsmodus noch einmal aktivieren.");
                    }
                }
                Err(e) => return Err(e),
            }
        }
        let handle = handle.ok_or_else(|| Error::other("Verbindung nicht aufzubauen"))?;
        let mut v = Verbindung { hci: &mut self.hci, handle, teil: Vec::new(), meldungen: Default::default() };
        let bis = Instant::now() + Duration::from_secs(15);
        let (mut start, mut ff00, mut ff10) = (1u16, None, None);
        loop {
            let r = v.att(&[0x10, start as u8, (start >> 8) as u8, 0xff, 0xff, 0x00, 0x28], bis)?;
            if r[0] != 0x11 {
                break;
            }
            for e in r[2..].chunks(r[1] as usize) {
                let (a, z) = (u16::from_le_bytes([e[0], e[1]]), u16::from_le_bytes([e[2], e[3]]));
                match &e[4..] {
                    [0x00, 0xff] => ff00 = Some((a, z)),
                    [0x10, 0xff] => ff10 = Some((a, z)),
                    _ => {}
                }
                start = z.wrapping_add(1);
            }
            if start == 0 {
                break;
            }
        }
        Ok(Sitzung { v, ff00, ff10, ff01: None })
    }
}

/// Verbundene, verschlüsselte Sitzung mit der Werks-Firmware
pub struct Sitzung<'a> {
    v: Verbindung<'a>,
    ff00: Option<(u16, u16)>,
    ff10: Option<(u16, u16)>,
    ff01: Option<u16>,
}

impl Sitzung<'_> {
    /// App-Befehl auf FF01 senden und die Antwort von FF02 holen (harmlose Abfragen für den Probelauf)
    pub fn abfrage(&mut self, rahmen: &[u8]) -> std::io::Result<Option<Vec<u8>>> {
        let bis = Instant::now() + Duration::from_secs(10);
        if self.ff01.is_none() {
            let (a, z) = self.ff00.ok_or_else(|| Error::other("Befehlsdienst FF00 fehlt"))?;
            let m = merkmale(&mut self.v, a, z)?;
            let wert = |u: u16| m.iter().find(|(x, _, _)| *x == u).map(|(_, h, _)| *h);
            let (ff01, ff02) = (wert(0xff01).ok_or_else(|| Error::other("FF01 fehlt"))?, wert(0xff02).ok_or_else(|| Error::other("FF02 fehlt"))?);
            let c = ff02 + 1;
            self.v.att(&[0x12, c as u8, (c >> 8) as u8, 0x01, 0x00], bis)?;
            self.ff01 = Some(ff01);
        }
        let h = self.ff01.unwrap();
        let mut d = vec![0x52, h as u8, (h >> 8) as u8];
        d.extend_from_slice(rahmen);
        self.v.hci.l2cap(self.v.handle, 4, &d)?;
        let bis = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(m) = self.v.meldungen.pop_front() {
                return Ok(Some(m));
            }
            match self.v.warten(bis)? {
                Some(Ankunft::L2cap(4, p)) if p[0] == 0x1b => return Ok(Some(p[3..].to_vec())),
                Some(Ankunft::L2cap(4, p)) => self.v.att_nebenbei(&p)?,
                Some(Ankunft::Ereignis(ev)) if ev[0] == 0x05 => return Err(Error::other("Verbindung getrennt")),
                Some(_) => {}
                None => return Ok(None),
            }
        }
    }

    /// Offizielles Update (Dienst FF10) einspielen; der Controller startet danach neu
    pub fn update(&mut self, daten: &[u8], fortschritt: &mut dyn FnMut(f32)) -> std::io::Result<()> {
        let (a, z) = self.ff10.ok_or_else(|| Error::other("Update-Dienst FF10 fehlt"))?;
        ota(&mut self.v, a, z, daten, fortschritt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c1_testvektor() {
        // Core Spec Vol 3 Part H Anhang / BlueZ smp.c test_c1
        let k = [0u8; 16];
        let r = [0xe0, 0x2e, 0x70, 0xc6, 0x4e, 0x27, 0x88, 0x63, 0x0e, 0x6f, 0xad, 0x56, 0x21, 0xd5, 0x83, 0x57];
        let preq = [0x01, 0x01, 0x00, 0x00, 0x10, 0x07, 0x07];
        let pres = [0x02, 0x03, 0x00, 0x00, 0x08, 0x00, 0x05];
        let ra = [0xb6, 0xb5, 0xb4, 0xb3, 0xb2, 0xb1];
        let ia = [0xa6, 0xa5, 0xa4, 0xa3, 0xa2, 0xa1];
        let soll = [0x86, 0x3b, 0xf1, 0xbe, 0xc5, 0x4d, 0xa7, 0xd2, 0xea, 0x88, 0x89, 0x87, 0xef, 0x3f, 0x1e, 0x1e];
        assert_eq!(c1(&k, &r, &preq, &pres, 1, &ia, 0, &ra), soll);
    }

    #[test]
    fn s1_testvektor() {
        let k = [0u8; 16];
        let mut r1 = [0u8; 16];
        r1[..8].copy_from_slice(&[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
        let mut r2 = [0u8; 16];
        r2[..8].copy_from_slice(&[0x00, 0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99]);
        let soll = [0x62, 0xa0, 0x6d, 0x79, 0xae, 0x16, 0x42, 0x5b, 0x9b, 0xf4, 0xb0, 0xe8, 0xf0, 0xe1, 0x1f, 0x9a];
        assert_eq!(s1(&k, &r1, &r2), soll);
    }
}


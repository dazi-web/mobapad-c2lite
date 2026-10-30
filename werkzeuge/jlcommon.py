"""Jieli-Hilfsfunktionen für die Bau-Werkzeuge: CRC16, ENC-Verschlüsselung, Verzeichniseinträge (nur Dateien)."""
import binascii

KEY = 0xA80F

def crc16(data, crc=0):
    return binascii.crc_hqx(bytes(data), crc)

def enc_block(buf, key):
    out = bytearray(buf)
    for i in range(len(out)):
        out[i] ^= key & 0xFF
        key = ((key << 1) ^ (0x1021 if key & 0x8000 else 0)) & 0xFFFF
    return bytes(out)

def crypt(d, base=0, key=KEY):
    """ENC in 32-Byte-Bloecken, Blockschluessel = key ^ ((base+o)>>2). base = Adresse von d[0]."""
    assert base % 32 == 0
    return b"".join(enc_block(d[o:o+32], key ^ (((base + o) >> 2) & 0xFFFF)) for o in range(0, len(d), 32))

def entry(h):
    return dict(hcrc=int.from_bytes(h[0:2], "little"), dcrc=int.from_bytes(h[2:4], "little"),
                off=int.from_bytes(h[4:8], "little"), size=int.from_bytes(h[8:12], "little"),
                attr=h[12], res=h[13], idx=int.from_bytes(h[14:16], "little"),
                name=h[16:32].split(b"\0")[0].decode("latin1"), hok=crc16(h[2:32]) == int.from_bytes(h[0:2], "little"))

def fmt(e):
    return (f"{e['name']:16s} off={e['off']:#010x} size={e['size']:#08x} ({e['size']}) attr={e['attr']:#04x} "
            f"idx={e['idx']} dcrc={e['dcrc']:#06x} hdrcrc={'OK' if e['hok'] else 'FALSCH'}")

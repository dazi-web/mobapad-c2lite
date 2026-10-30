/* Prüft c2_aes.h und die Kopplungsrechnung des Pro Controller 2 auf dem Rechner:
 *   cc -I sdk-arbeit/jieli-sdk/sdk/apps/hid/modules/c2lite -o /tmp/t werkzeuge/test_pro2_krypto.c && /tmp/t
 * Testwerte: FIPS-197 Anhang C.1 und der Mitschnitt in ndeadly/switch2_controller_research (bluetooth_interface.md,
 * commands.md: A1 3503e929…, Antwort B2 134c97f5…). */
#include <stdio.h>
#include <string.h>
#include "c2_aes.h"

static void hex(const char *t, unsigned char *b)
{
    for (int i = 0; i < 16; i++) {
        sscanf(t + 2 * i, "%2hhx", b + i);
    }
}

int main(void)
{
    unsigned char k[16], p[16], soll[16], ist[16];
    hex("000102030405060708090a0b0c0d0e0f", k);
    hex("00112233445566778899aabbccddeeff", p);
    hex("69c4e0d86a7b0430d8cdb78070b4c55a", soll);
    c2_aes128(k, p, ist);
    int fips = !memcmp(ist, soll, 16);

    /* Leitungsdaten wie in c2_pro2.c: LTK = A1 ^ B1 (Leitung), B2 = AES(rev LTK, rev A2), so gesendet */
    unsigned char a1[16], b1[16], a2[16], ltk_rev[16], a2_rev[16];
    hex("3503e92982877124bea80c664615834b", a1);
    hex("5cf6ee792cdf05e1ba2b6325c41a5f10", b1);
    hex("6fc6df8ad8fedf15bb8c15e91f320544", a2);
    hex("134c97f511b9b6dd4d86fd40f536e9ed", soll);
    for (int i = 0; i < 16; i++) {
        ltk_rev[i] = a1[15 - i] ^ b1[15 - i];
        a2_rev[i] = a2[15 - i];
    }
    c2_aes128(ltk_rev, a2_rev, ist);
    int pro2 = !memcmp(ist, soll, 16);
    printf("FIPS-197: %s, Pro-Controller-2-Mitschnitt: %s\n", fips ? "ok" : "FALSCH", pro2 ? "ok" : "FALSCH");
    return !(fips && pro2);
}

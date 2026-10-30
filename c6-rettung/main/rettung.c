/*
 * C6 als Rettungssender für den C2 Lite: sendet den USB-Schlüssel der Jieli-Chips (0x16EF), damit der Controller
 * im ROM-Download-Modus „UBOOT1.00“ startet (USB 4c4a:2342, dann jl-uboot-tool am Laptop). Logik und Zeiten von
 * einem früheren, erfolgreichen Rettungssender übernommen (28.10.).
 *
 * Verkabelung (PENGLIN-Platine zwischen Controller und heilem Kabel zum Laptop, 1 kΩ in Reihe als Schutz optional):
 *   C6 IO6 -> D+ (Takt), C6 IO5 -> D− (Daten), C6 GND -> GND. VBUS NIE an den C6.
 *
 * Bedienung über die serielle Konsole (UART-Buchse, 115200) oder den BOOT-Knopf (= s, erst nach dem Start drücken):
 *   s Start  x Stopp  p Pegel  w Takt/Daten tauschen  t Tempo weiterschalten  d Dauer weiterschalten  h Hilfe
 * Ablauf: Laptop-Ende steckt; s, dann einmal Nadel-Reset (kaputte Firmware startet von selbst ständig neu). Danach
 * nichts mehr anfassen: Ein weiterer Reset wirft den Chip aus dem Download-Modus.
 * Quittiert der Chip (zieht den Takt über 300 µs auf low), hört der C6 sofort auf, lässt los und gibt ERKANNT aus.
 */
#include <stdio.h>

#include "driver/gpio.h"
#include "driver/uart.h"
#include "esp_attr.h"
#include "esp_rom_sys.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "hal/gpio_ll.h"
#include "soc/gpio_struct.h"

#define PIN_DM 5   /* D− */
#define PIN_DP 6   /* D+ */
#define PIN_BOOT 9 /* BOOT-Knopf, gedrückt = low */
#define PINS ((1ULL << PIN_DM) | (1ULL << PIN_DP))

/* Laut BR23-ROM (sdk cpu/br23/tools/rom.lst, boot_skip.c): Takt auf D+, Daten auf D−, Datenbit wird kurz NACH der
 * steigenden Takt-Flanke per Software gelesen, höchstes Bit zuerst; das ROM horcht nur ~10 ms nach jedem Reset.
 * Treffer quittiert es, indem es beide Leitungen ~2 ms auf low treibt, danach legt es den D+-Pull-up an und eicht
 * seinen Takt an den SOFs des PCs. Deshalb lückenlos senden und bei der Quittung sofort loslassen. */
#define SCHLUESSEL 0x16EF
#define PRUEF_US 2000 /* so oft kurz aus dem kritischen Abschnitt (Stopp, Ende) */

static const struct {
    int low, high;
} TEMPO[] = {{7, 3}, {10, 10}, {20, 20}, {50, 50}}; /* 7/3 = „TEMPO ALT“, damit hat es geklappt */
static const int DAUER_S[] = {30, 60, 15};

static struct {
    TaskHandle_t task;
    volatile bool stopp, laeuft;
    bool takt_an_dp; /* true = Takt an IO6 (laut ROM D+; damit klappt es) */
    int tempo, dauer;
    int takt, daten;
    int64_t ende_us;
    volatile uint32_t gesendet;
    volatile bool erkannt;
} r;

static portMUX_TYPE sperre = portMUX_INITIALIZER_UNLOCKED;

/* Ein Schlüssel, ca. 180 µs. Im IRAM, damit kein Flash-Cache-Fehlgriff den Takt dehnt. Daten wechseln nur bei
 * Takt low. true = Quittung: Der Takt liest low, obwohl wir ihn (schwach) high treiben, weil der Chip ihn nach
 * Masse zieht, und zwar über 300 µs (die Quittung dauert ~2 ms). Pakete des PCs an ein angemeldetes Gerät ziehen
 * D+ nur für Mikrosekunden, das zählt nicht. */
static bool IRAM_ATTR schluessel_senden(int takt, int daten, int low, int high)
{
    for (int i = 15; i >= 0; i--) {
        gpio_ll_set_level(&GPIO, takt, 0);
        gpio_ll_set_level(&GPIO, daten, (SCHLUESSEL >> i) & 1);
        esp_rom_delay_us(low);
        gpio_ll_set_level(&GPIO, takt, 1);
        esp_rom_delay_us(high);
        if (!gpio_ll_get_level(&GPIO, takt)) {
            esp_rom_delay_us(100);
            if (!gpio_ll_get_level(&GPIO, takt)) {
                esp_rom_delay_us(200);
                if (!gpio_ll_get_level(&GPIO, takt)) {
                    return true;
                }
            }
        }
    }
    /* Pause wie im Sketch: Takt bleibt 2 µs länger high, dann beide low */
    esp_rom_delay_us(2);
    gpio_ll_set_level(&GPIO, takt, 0);
    gpio_ll_set_level(&GPIO, daten, 0);
    esp_rom_delay_us(low);
    return false;
}

/* Sofort ganz loslassen: Eingang ohne Pull, damit nichts mehr in den SOF-Abgleich des ROMs funkt */
static void IRAM_ATTR loslassen(void)
{
    gpio_ll_output_disable(&GPIO, PIN_DM);
    gpio_ll_output_disable(&GPIO, PIN_DP);
    gpio_ll_pulldown_dis(&GPIO, PIN_DM);
    gpio_ll_pulldown_dis(&GPIO, PIN_DP);
}

/* Frei: nur Eingang mit schwachem Pull-down (~45k, parallel zu den 15k des Laptops unkritisch). D+ liest nur dann 1,
 * wenn ein Gerät seinen 1,5k-Pull-up anlegt. */
static void leitungen_frei(void)
{
    const gpio_config_t c = {.pin_bit_mask = PINS, .mode = GPIO_MODE_INPUT, .pull_up_en = GPIO_PULLUP_DISABLE,
                             .pull_down_en = GPIO_PULLDOWN_ENABLE};
    gpio_config(&c);
}

static void sende_task(void *arg)
{
    (void)arg;
    /* Nur die Quittung zählt: Am Pegel allein (D+ hoch) ist der Download-Modus nicht von einer Firmware mit USB zu
     * unterscheiden, die sich nach dem Reset wieder anmeldet. */
    bool quittung = false;
    int low = TEMPO[r.tempo].low, high = TEMPO[r.tempo].high;
    /* Ohne vTaskDelay: Eine Tick-Pause (10 ms) wäre so lang wie das ganze Hörfenster des ROMs. Die Konsole läuft
     * trotzdem, weil sie gleiche Priorität hat und meist in uart_read_bytes wartet. */
    while (!r.stopp && !r.erkannt && esp_timer_get_time() < r.ende_us) {
        int64_t pruef = esp_timer_get_time() + PRUEF_US;
        while (!quittung && esp_timer_get_time() < pruef) {
            portENTER_CRITICAL(&sperre);
            quittung = schluessel_senden(r.takt, r.daten, low, high);
            if (quittung) {
                loslassen();
            }
            portEXIT_CRITICAL(&sperre);
            r.gesendet++;
        }
        if (quittung) {
            r.erkannt = true;
            break;
        }
    }
    loslassen();
    if (r.erkannt) {
        vTaskDelay(pdMS_TO_TICKS(20)); /* Quittung (~2 ms) vorbei, dann sollte D+ vom Chip hoch sein */
        printf("ERKANNT (%s): Controller meldet sich (D- %d, D+ %d), %lu Schlüssel gesendet, Leitungen frei\n",
               quittung ? "Quittung" : "Pegel", gpio_get_level(PIN_DM), gpio_get_level(PIN_DP),
               (unsigned long)r.gesendet);
    } else {
        printf("ENDE: %lu Schlüssel gesendet, keine Quittung, Leitungen frei\n", (unsigned long)r.gesendet);
    }
    r.laeuft = false;
    r.task = NULL;
    vTaskDelete(NULL);
}

static void einstellungen(void)
{
    printf("Takt an %s, Daten an %s, Tempo %d/%d us, Dauer %d s\n", r.takt_an_dp ? "D+ (IO6)" : "D- (IO5)",
           r.takt_an_dp ? "D- (IO5)" : "D+ (IO6)", TEMPO[r.tempo].low, TEMPO[r.tempo].high, DAUER_S[r.dauer]);
}

static void start(void)
{
    if (r.laeuft) {
        printf("SENDET SCHON\n");
        return;
    }
    r.takt = r.takt_an_dp ? PIN_DP : PIN_DM;
    r.daten = r.takt_an_dp ? PIN_DM : PIN_DP;
    /* Ein- und Ausgang zugleich: Beim Senden wird der Takt zurückgelesen (Quittung) */
    const gpio_config_t c = {.pin_bit_mask = PINS, .mode = GPIO_MODE_INPUT_OUTPUT, .pull_up_en = GPIO_PULLUP_DISABLE,
                             .pull_down_en = GPIO_PULLDOWN_ENABLE};
    gpio_config(&c);
    /* Schwächste Stufe (~5 mA): Quittiert der Chip, zieht er die Leitungen gegen unseren Pegel nach Masse */
    gpio_set_drive_capability(PIN_DM, GPIO_DRIVE_CAP_0);
    gpio_set_drive_capability(PIN_DP, GPIO_DRIVE_CAP_0);
    gpio_set_level(PIN_DM, 0);
    gpio_set_level(PIN_DP, 0);
    r.gesendet = 0;
    r.erkannt = false;
    r.stopp = false;
    r.ende_us = esp_timer_get_time() + (int64_t)DAUER_S[r.dauer] * 1000000;
    r.laeuft = true;
    /* Über dem Hauptprogramm (Konsole): gesendet wird durchgehend, nur alle 15 ms ein Tick Pause */
    if (xTaskCreate(sende_task, "rettung", 3072, NULL, 1, &r.task) != pdPASS) {
        r.laeuft = false;
        leitungen_frei();
        printf("FEHLER: Task nicht anlegbar\n");
        return;
    }
    printf("START: sende Schlüssel. JETZT Controller einschalten (Schiebeschalter) bzw. Nadel-Reset, EINMAL.\n");
    einstellungen();
}

static void stopp(void)
{
    r.stopp = true;
    while (r.laeuft) {
        vTaskDelay(pdMS_TO_TICKS(5));
    }
}

static void pegel(void)
{
    if (r.laeuft) {
        printf("SENDET, Pegel erst danach\n");
        return;
    }
    printf("PEGEL: D- %d, D+ %d (beide 0 = nichts angemeldet, nur D+ 1 = Gerät meldet sich, beide 1 = ROM horcht)\n",
           gpio_get_level(PIN_DM), gpio_get_level(PIN_DP));
}

static void hilfe(void)
{
    printf("C6-Rettungssender bereit. s Start, x Stopp, p Pegel, w Takt/Daten tauschen, t Tempo, d Dauer, h Hilfe. "
           "BOOT-Knopf = s.\n");
    einstellungen();
}

void app_main(void)
{
    r.takt_an_dp = true;
    leitungen_frei();
    const gpio_config_t knopf = {.pin_bit_mask = 1ULL << PIN_BOOT, .mode = GPIO_MODE_INPUT,
                                 .pull_up_en = GPIO_PULLUP_ENABLE, .pull_down_en = GPIO_PULLDOWN_DISABLE};
    gpio_config(&knopf);
    uart_driver_install(UART_NUM_0, 256, 0, 0, NULL, 0);
    hilfe();
    pegel();

    bool knopf_alt = true;
    int64_t naechster_stand = 0;
    for (;;) {
        uint8_t z;
        if (uart_read_bytes(UART_NUM_0, &z, 1, pdMS_TO_TICKS(50)) == 1) {
            switch (z) {
            case 's': start(); break;
            case 'x': stopp(); break;
            case 'p': pegel(); break;
            case 'h': hilfe(); break;
            case 'w':
            case 't':
            case 'd':
                if (r.laeuft) {
                    printf("SENDET, erst x\n");
                    break;
                }
                if (z == 'w') {
                    r.takt_an_dp = !r.takt_an_dp;
                } else if (z == 't') {
                    r.tempo = (r.tempo + 1) % (int)(sizeof(TEMPO) / sizeof(TEMPO[0]));
                } else {
                    r.dauer = (r.dauer + 1) % (int)(sizeof(DAUER_S) / sizeof(DAUER_S[0]));
                }
                einstellungen();
                break;
            default: break;
            }
        }
        bool knopf_jetzt = gpio_get_level(PIN_BOOT);
        if (knopf_alt && !knopf_jetzt) {
            start();
        }
        knopf_alt = knopf_jetzt;
        if (r.laeuft && esp_timer_get_time() >= naechster_stand) {
            int64_t rest = (r.ende_us - esp_timer_get_time()) / 1000000;
            printf("SENDET: %lu Schlüssel, noch %lld s\n", (unsigned long)r.gesendet, rest > 0 ? rest : 0);
            naechster_stand = esp_timer_get_time() + 2000000;
        }
    }
}

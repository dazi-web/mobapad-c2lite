/* Brücke 2: ersetzt den Anfang der Werks-App 0.04 (Einsprung 0x01E000C0). Wartet, bis USB-Spannung an PB5 anliegt
 * (Pull-down, entprellt), und springt dann in den ROM-Download-Modus: nvram_set_boot_state(2) im Masken-ROM
 * (0x1127F0, sdk.ld) und System-Reset über JL_CLOCK->PWR_CON Bit 4 – wie go_mask_usb_updata() im SDK.
 * Bewusst NICHT die werkseigene Funktion 0x01E0156C: Die ruft eine RAM-Hilfsfunktion (0x114), die beim Start noch
 * nicht kopiert ist. Ohne Kabel wartet der Controller einfach; der Download-Modus beginnt erst mit USB, früh im Start
 * (kein Watchdog der Werks-Firmware). Port B: 0x1E5040 (br23.h, JL_PORT_FLASH_TypeDef: OUT IN DIR DIE PU PD). */
#define PB ((volatile unsigned int *)0x1E5040)
#define PB_IN 1
#define PB_DIR 2
#define PB_DIE 3
#define PB_PU 4
#define PB_PD 5
#define BIT5 (1u << 5)

__attribute__((section(".text.bruecke"), used, noreturn)) void bruecke(void)
{
    PB[PB_DIR] |= BIT5; /* Eingang */
    PB[PB_DIE] |= BIT5; /* digitaler Eingang an */
    PB[PB_PU] &= ~BIT5;
    PB[PB_PD] |= BIT5;  /* Pull-down: ohne Kabel sicher 0 */
    unsigned int n = 0;
    while (n < 20000) { /* einige ms durchgehend high */
        n = (PB[PB_IN] & BIT5) ? n + 1 : 0;
    }
    ((void (*)(unsigned int))0x1127F0)(2);    /* nvram_set_boot_state(UPGRADE_USB_SOFT_KEY) */
    *(volatile unsigned int *)0x1E0000 |= 0x10; /* JL_CLOCK->PWR_CON: System-Reset */
    while (1) {
    }
}

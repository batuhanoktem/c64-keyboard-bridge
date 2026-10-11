/* VICE runs the real assembly scanner at idle, then the real client with a
 * deterministic local-exit matrix provider. No Ultimate/UCI emulation claim. */
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#define BYTE(a) (*(volatile uint8_t *)(a))
extern uint8_t __fastcall__ scan_raw(uint8_t *matrix);
extern uint8_t __fastcall__ check_scan_flags(uint8_t *matrix);
extern int bridge_main(void);
static uint8_t matrix[8];

uint8_t __fastcall__ test_scan_raw(uint8_t *out) {
    memset(out, 0, 8); out[7] = 0xa0; return 0;
}
static void fail(uint8_t code) {BYTE(0xd7ff)=code;for (;;) {}}
int main(void) {
    uint8_t i, ddr_a=BYTE(0xdc02), ddr_b=BYTE(0xdc03), start;
    if (!check_scan_flags(matrix)) fail(1);
    if (BYTE(0xdc02)!=ddr_a || BYTE(0xdc03)!=ddr_b) fail(2);
    if (scan_raw(matrix)) fail(3);
    for (i=0;i<8;++i) if (matrix[i]) fail(4);
    BYTE(0x289)=10;BYTE(0x291)=0;start=BYTE(0xa2);
    bridge_main();
    if ((uint8_t)(BYTE(0xa2)-start)<60) fail(5);
    if (BYTE(0x289)!=10 || BYTE(0x291)!=0 || BYTE(0xc6)!=0) fail(6);
    if (BYTE(0xdc02)!=ddr_a || BYTE(0xdc03)!=ddr_b) fail(7);
    puts("VICE SCANNER AND MOCKED LOCAL EXIT PASS");
    BYTE(0xd7ff)=0;return 0;
}

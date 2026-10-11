/* Native white-box tests run actual complete/schedule/scan/main client code.
 * Only display, memory-mapped I/O and the assembly matrix provider are mocked. */
#include <assert.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>

static uint8_t memory[65536], test_matrix[8], jiffy_auto;
static uint8_t *test_byte(uint16_t address) {
    if (address == 0xa2 && jiffy_auto) ++memory[address];
    return memory + address;
}
#define BYTE(address) (*test_byte((uint16_t)(address)))
#define __fastcall__
#define main production_main
#include "../src/main.c"
#undef main

void gotoxy(uint8_t x, uint8_t y) { (void)x; (void)y; }
void cclear(uint8_t count) { (void)count; }
void cputs(const char *text) { (void)text; }
int cprintf(const char *format, ...) { (void)format; return 0; }
void cputc(char character) { (void)character; }
void clrscr(void) {}
uint8_t scan_raw(uint8_t *matrix) { memcpy(matrix, test_matrix, 8); return 0; }
static uint8_t test_read(uint16_t address) { return memory[address]; }
static void test_write(uint16_t address, uint8_t value) { memory[address] = value; }

static void reset_state(void) {
    memset(memory, 0, sizeof(memory)); memset(test_matrix, 0, sizeof(test_matrix));
    jiffy_auto = 0;
    uci_init(&uci, test_read, test_write); kb_init(&keyboard); kb_reader_init(&reader);
    now = 1000; last_read = last_hb = now; last_scan = 0; retry_delay = 120;
    socket_handle = 7; socket_valid = 1; want_close = 0;
    state = STATE_READY; enabled = 1; quit = quit_stage = chord_held = 0;
    tx_len = tx_pos = 0; tx_sequence = 3; need_sync = halted_after_close = 0;
#ifdef NETWORK_PROBE
    probe_stage = ack_bytes = idle_observed = 0;
    memset(probe_report, 0, sizeof(probe_report));
#endif
}
static void set_status(const char *text) {
    strcpy(uci.status, text); uci.status_len = (uint16_t)strlen(text);
}
static void write_completed(uint8_t count) {
    uci.phase = 0; uci.response_len = 2;
    uci.response[0] = count; uci.response[1] = 0; set_status("00,OK"); complete(1);
}

static void test_eof_and_idle(void) {
    reset_state(); operation = OP_READ; want_close = 1;
    set_status("01,CONNECTION CLOSED BY HOST"); uci.response_len = 2;
    complete(1);
    assert(!socket_valid && !want_close);
    schedule(); assert(!uci.phase); /* Pending watchdog CLOSE must not survive EOF. */

    reset_state(); operation = OP_READ; last_read = 900;
    set_status("02,NO DATA: 11"); uci.response_len = 2;
    uci.response[0] = uci.response[1] = 255;
    complete(1);
    assert(socket_valid && !want_close && state == STATE_READY && last_read == now);
}

static void test_partial_write_and_exit(void) {
    uint8_t old_frame[16], i;
    reset_state(); operation = OP_WRITE; tx_len = 12; tx_pos = 2; write_requested = 10;
    for (i = 0; i < tx_len; ++i) tx[i] = i;
    write_completed(3); assert(tx_pos == 5 && tx_len == 12);
    schedule(); assert(operation == OP_WRITE && write_requested == 7);
    assert(uci.command_len == 10 && !memcmp(uci.command + 3, tx + 5, 7));
    write_completed(7); assert(!tx_len && !tx_pos);

    reset_state(); tx_len = kb_frame(tx, KB_SYNC, 3, keyboard.accepted, 8);
    memcpy(old_frame, tx, 16); tx_pos = 5; tx_sequence = 4; quit = 1;
    schedule();
    assert(quit_stage == 4 && operation == OP_WRITE && write_requested == 11);
    assert(tx_len == 16 && tx_pos == 5 && tx_sequence == 4 && !memcmp(tx, old_frame, 16));
    assert(!memcmp(uci.command + 3, old_frame + 5, 11));
    write_completed(4); assert(tx_pos == 9 && quit_stage == 4);
    schedule(); assert(write_requested == 7 && !memcmp(uci.command + 3, old_frame + 9, 7));
    write_completed(7); assert(!tx_len && !tx_pos && !quit_stage);
    schedule();
    assert(quit_stage == 1 && tx_len == 9 && tx[3] == KB_DISCONNECT && tx[6] == 4);
    write_completed(9); assert(quit_stage == 2);
    schedule(); assert(operation == OP_CLOSE && uci.command_len == 3 && uci.command[2] == 7);
    uci.phase = 0; uci.response_len = 0; set_status("00,OK"); complete(1);
    assert(quit_stage == 3 && !socket_valid && !want_close);

    reset_state(); quit = 1; quit_stage = 4; operation = OP_WRITE;
    uci.response_len = 2; uci.response[0] = uci.response[1] = 255;
    set_status("12,SEND ERROR"); complete(1);
    assert(quit_stage == 2);
    schedule(); assert(operation == OP_CLOSE); /* Failed flush still gets bounded cleanup. */
}

static void test_snapshot_or_observation(void) {
    reset_state();
#ifndef NETWORK_PROBE
    need_sync = 1; keyboard.accepted[1] = 4;
    keyboard.count = 1; keyboard.queue[0].key = 10; keyboard.queue[0].type = KB_DOWN;
    fill_tx(); assert(tx_len == 16 && tx[3] == KB_SYNC && tx[9] == 4 && !keyboard.count);
#else
    probe_stage = 5; memset(probe_report, 0x7b, 97);
    fill_tx(); assert(tx_len == 105 && !memcmp(tx, idle_ok, 8));
    assert(!memcmp(tx + 8, probe_report, 97));
    reset_state(); operation = OP_WRITE; probe_stage = 1;
    uci.started = now - 7; tx_len = 32; write_requested = 32;
    write_completed(5);
    assert(probe_report[3] == 4 && probe_report[58] == OP_WRITE);
    assert(probe_report[67] == 5 && probe_report[68] == 0);
    assert(probe_report[69] == 7 && probe_report[70] == 0);
#endif
}

static void test_local_exit_and_cleanup_deadline(void) {
    reset_state(); now = 0; socket_valid = 0; jiffy_auto = 1;
    test_matrix[7] = 0xa0; memory[0x289] = 10; memory[0x291] = 0x21;
    assert(production_main() == 0);
    assert(now >= 61 && now <= 64);
    assert(memory[0x289] == 10 && memory[0x291] == 0x21 && memory[0xc6] == 0);
#ifndef NETWORK_PROBE
    assert(!(keyboard.accepted[7] & 0x80)); /* Exit RUN/STOP never reaches host matrix. */
#endif

    reset_state(); now = 0; jiffy_auto = 1; test_matrix[7] = 0xa0;
    memory[0x289] = 10; memory[0x291] = 0x21;
    memcpy(bridge_config + 10, "127.0.0.1", 10); bridge_config[26] = 0x66; bridge_config[27] = 0x19;
    memory[UCI_COMMAND] = 0xc9; memory[UCI_CONTROL] = 0x10;
    assert(production_main() == 0);
    assert(now >= 181 && now <= 184 && memory[UCI_CONTROL] == 4);
    assert(memory[0x289] == 10 && memory[0x291] == 0x21 && memory[0xc6] == 0);
}

int main(void) {
    test_eof_and_idle(); test_partial_write_and_exit(); test_snapshot_or_observation();
    test_local_exit_and_cleanup_deadline();
    puts("C64 production state tests passed: EOF, idle, partial writes, framed exit, bounded cleanup");
    return 0;
}

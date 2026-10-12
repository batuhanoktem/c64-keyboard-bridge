#include "bridge.h"
#include "uci.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static uint8_t command_bytes[256], command_count, push_count, accept_count, abort_count;
static uint8_t hardware_state, data[256], data_len, data_pos, chunk_more;
static char status_bytes[256];
static uint16_t status_len, status_pos, step_transfers;

static uint8_t read_register(uint16_t address) {
    if (address == UCI_COMMAND) return 0xc9;
    if (address == UCI_CONTROL) {
        return hardware_state | (data_pos < data_len ? 0x80 : 0) |
               (status_pos < status_len ? 0x40 : 0);
    }
    ++step_transfers;
    if (address == UCI_RESPONSE) return data[data_pos++];
    assert(address == UCI_STATUS);
    return (uint8_t)status_bytes[status_pos++];
}
static void write_register(uint16_t address, uint8_t value) {
    if (address == UCI_COMMAND) {
        command_bytes[command_count++] = value; ++step_transfers; return;
    }
    assert(address == UCI_CONTROL);
    if (value == 1) { ++push_count; hardware_state = 0x10; }
    if (value == 2) { ++accept_count; hardware_state = chunk_more ? 0x10 : 0; }
    if (value == 4) { ++abort_count; hardware_state = 0x10; }
    if (value == 8) hardware_state &= (uint8_t)~8;
}
static void reset_hardware(void) {
    command_count = push_count = accept_count = abort_count = 0;
    data_len = data_pos = chunk_more = 0;
    status_len = status_pos = hardware_state = step_transfers = 0;
}
static void response(const uint8_t *bytes, uint8_t len, const char *status, uint8_t more) {
    if (len) memcpy(data, bytes, len);
    data_len = len; data_pos = 0;
    strcpy(status_bytes, status); status_len = (uint16_t)strlen(status); status_pos = 0;
    chunk_more = more; hardware_state = more ? 0x30 : 0x20;
}
static uint8_t bounded_step(Uci *u, uint16_t tick) {
    uint8_t result;
    step_transfers = 0; result = uci_step(u, tick);
    assert(step_transfers <= 32);
    return result;
}

static void test_commands_and_fifo(void) {
    Uci u;
    uint8_t cmd[131], body[130], n, i, result = 0;
    reset_hardware(); uci_init(&u, read_register, write_register);
    assert(uci_available(&u));
    n = uci_open_command(cmd, (const uint8_t *)"192.168.0.42", 6502);
    assert(n == 17 && cmd[0] == 3 && cmd[1] == 7 && cmd[2] == 0x66 && cmd[3] == 0x19);
    assert(!memcmp(cmd + 4, "192.168.0.42\0", 13));
    assert(uci_read_command(cmd, 17, 128) == 5);
    assert(!memcmp(cmd, "\3\20\21\200\0", 5));
    assert(!uci_read_command(cmd, 17, 129));
    assert(uci_close_command(cmd, 17) == 3 && cmd[0] == 3 && cmd[1] == 9 && cmd[2] == 17);
    body[0] = 0; body[1] = 128; body[2] = 255;
    assert(uci_write_command(cmd, 17, body, 3) == 6);
    assert(cmd[0] == 3 && cmd[1] == 17 && cmd[2] == 17 && !memcmp(cmd + 3, body, 3));
    memset(cmd, 0xa5, sizeof(cmd)); cmd[0] = 3; cmd[1] = 0x11; cmd[2] = 7;
    cmd[3] = 0; /* Binary NUL must survive, not terminate the command. */
    assert(uci_begin(&u, cmd, 131, 0, 300));
    assert(!uci_begin(&u, cmd, 2, 0, 300));
    for (i = 0; i < 5; ++i) assert(!bounded_step(&u, i));
    assert(command_count == 131 && push_count == 1 && !memcmp(cmd, command_bytes, 131));
    /* Busy can clear before a response exists: do not complete on bit zero alone. */
    hardware_state = 0; assert(!bounded_step(&u, 6));
    for (i = 0; i < 130; ++i) body[i] = i;
    response(body, 130, "00,OK", 0);
    for (i = 7; i < 30 && !result; ++i) result = bounded_step(&u, i);
    assert(result == 1 && accept_count == 1 && u.response_len == 130);
    assert(!memcmp(u.response, body, 130) && !strcmp(u.status, "00,OK"));
}

static void test_multiblock_and_timeout(void) {
    Uci u;
    uint8_t cmd[] = {3,1}, first[] = {1,2}, second[] = {3,4};
    reset_hardware(); uci_init(&u, read_register, write_register);
    assert(uci_begin(&u, cmd, 2, 65530, 20));
    bounded_step(&u, 65530);
    response(first, 2, "", 1); bounded_step(&u, 65531);
    assert(accept_count == 1 && u.phase == 3);
    response(second, 2, "00,OK", 0); bounded_step(&u, 65532);
    assert(bounded_step(&u, 65533) == 1);
    assert(u.response_len == 4 && !memcmp(u.response, "\1\2\3\4", 4));
    assert(uci_begin(&u, cmd, 2, 65530, 20)); bounded_step(&u, 65530);
    assert(!bounded_step(&u, 14) && abort_count == 1 && u.phase == 5);
    assert(!uci_begin(&u, cmd, 2, 15, 20));
    assert(!bounded_step(&u, 1000)); /* Abort pending stays responsive, never pushes another command. */
    hardware_state = 4; assert(!bounded_step(&u, 1001));
    hardware_state = 0; assert(bounded_step(&u, 1002) == 2 && u.phase == 0);
    assert(uci_begin(&u, cmd, 2, 1003, 20));
}

static void test_status_and_counts(void) {
    Uci u;
    int16_t count;
    memset(&u, 0, sizeof(u));
    u.response[0] = u.response[1] = 255; u.response_len = 2;
    strcpy(u.status, "02,NO DATA: 11"); u.status_len = (uint16_t)strlen(u.status);
    assert(uci_read_result(&u, 128, &count) == 2 && count == -1);
    strcpy(u.status, "02,NO DATA: 104"); u.status_len = (uint16_t)strlen(u.status);
    assert(!uci_read_result(&u, 128, &count));
    strcpy(u.status, "00,OK"); u.status_len = 5;
    assert(!uci_read_result(&u, 128, &count));
    u.response[0] = 3; u.response[1] = 0; u.response_len = 5;
    assert(uci_read_result(&u, 3, &count) == 1 && count == 3);
    assert(!uci_read_result(&u, 2, &count));
    u.response_len = 4; assert(!uci_read_result(&u, 3, &count));
    u.response_len = 2; assert(uci_write_result(&u, 128, &count) && count == 3);
    assert(!uci_write_result(&u, 2, &count));
    u.response[0] = 0; strcpy(u.status, "01,CONNECTION CLOSED BY HOST");
    u.status_len = (uint16_t)strlen(u.status);
    assert(uci_read_result(&u, 128, &count) == 3);
    strcpy(u.status, "12,SEND ERROR"); u.status_len = (uint16_t)strlen(u.status);
    assert(!uci_write_result(&u, 128, &count));
    strcpy(u.status, "00,OK"); u.status_len = 5;
    assert(!uci_read_result(&u, 128, &count)); /* Undocumented zero+OK must not be called idle. */
}

static void test_scanner_transitions(void) {
    Keyboard k;
    KeyTransition e;
    uint8_t m[8] = {0}, key;
    for (key = 0; key < 64; ++key) {
        kb_init(&k); memset(m, 0, 8); m[key >> 3] = (uint8_t)(1U << (key & 7));
        assert(!kb_sample(&k, m, 0)); assert(kb_sample(&k, m, 0) == 1);
        assert(kb_pop(&k, &e) && e.type == KB_DOWN && e.key == key);
        assert(!kb_sample(&k, m, 0) && !kb_pop(&k, &e));
        memset(m, 0, 8); kb_sample(&k, m, 0); assert(kb_sample(&k, m, 0) == 1);
        assert(kb_pop(&k, &e) && e.type == KB_UP && e.key == key && e.modifiers == 0);
    }
    kb_init(&k); m[1] = 128 | 4; /* A + Left Shift */
    kb_sample(&k, m, 0); kb_sample(&k, m, 0);
    assert(kb_pop(&k, &e) && e.key == 15 && e.modifiers == 1);
    assert(kb_pop(&k, &e) && e.key == 10 && e.modifiers == 1);
    m[6] = 16; kb_sample(&k, m, 0); kb_sample(&k, m, 0);
    assert(kb_pop(&k, &e) && e.key == 52 && e.modifiers == 1);
    m[1] &= 4; kb_sample(&k, m, 0); kb_sample(&k, m, 0);
    assert(kb_pop(&k, &e) && e.key == 15 && e.type == KB_UP && e.modifiers == 1);
    memset(m, 0, 8); kb_sample(&k, m, 0); kb_sample(&k, m, 0);
    assert(kb_pop(&k, &e) && e.key == 10 && e.modifiers == 1);
    assert(kb_pop(&k, &e) && e.key == 52 && e.modifiers == 0);
}

static void test_ambiguity_overflow_and_bounce(void) {
    Keyboard k;
    uint8_t m[8] = {0}, i;
    kb_init(&k); m[1] = 4; kb_sample(&k, m, 0);
    memset(m, 0, 8); kb_sample(&k, m, 0); kb_sample(&k, m, 0);
    assert(k.count == 0); /* One unstable scan never creates a key press. */
    m[1] = m[2] = 3; assert(kb_ambiguous(m));
    assert(kb_sample(&k, m, 0) == 2 && k.blocked);
    m[2] = 0; assert(!kb_sample(&k, m, 0)); /* Cannot resume with held ambiguous keys. */
    memset(m, 0, 8); kb_sample(&k, m, 0); assert(!k.blocked);
    assert(kb_sample(&k, m, 1) == 2 && k.blocked);
    kb_init(&k);
    for (i = 0; i < 64; ++i) {
        m[1] = 4; kb_sample(&k, m, 0); kb_sample(&k, m, 0);
        m[1] = 0; kb_sample(&k, m, 0); kb_sample(&k, m, 0);
    }
    assert(k.count == 128);
    m[1] = 4; kb_sample(&k, m, 0);
    assert(kb_sample(&k, m, 0) == 3 && k.overflow);
}

static void test_independent_key_debounce(void) {
    Keyboard k;
    KeyTransition e;
    uint8_t m[8], key, neighbor, column, mask, neighbor_column, neighbor_mask;
    /* A neighboring edge on the second sample must not delay either direction.
     * Exercise all keys, including same-column neighbors and each modifier. */
    for (key = 0; key < 64; ++key) {
        kb_init(&k); memset(m, 0, 8);
        neighbor = (key + 1) & 63;
        column = key >> 3; mask = (uint8_t)(1U << (key & 7));
        neighbor_column = neighbor >> 3; neighbor_mask = (uint8_t)(1U << (neighbor & 7));
        m[column] = mask;
        assert(!kb_sample(&k, m, 0));
        m[neighbor_column] |= neighbor_mask;
        assert(kb_sample(&k, m, 0) == 1);
        assert(kb_pop(&k, &e) && e.type == KB_DOWN && e.key == key);
        assert(!kb_pop(&k, &e));
        m[neighbor_column] &= (uint8_t)~neighbor_mask;
        assert(!kb_sample(&k, m, 0)); /* One-sample neighboring press was bounce. */
        m[column] &= (uint8_t)~mask; m[neighbor_column] |= neighbor_mask;
        assert(!kb_sample(&k, m, 0));
        m[neighbor_column] &= (uint8_t)~neighbor_mask;
        assert(kb_sample(&k, m, 0) == 1);
        assert(kb_pop(&k, &e) && e.type == KB_UP && e.key == key && e.modifiers == 0);
        assert(!kb_pop(&k, &e));
    }

    /* Rolling A then B overlaps for one sample; each key is stable twice even
     * though no whole pressed matrix repeats. Preserve both complete taps. */
    kb_init(&k); memset(m, 0, 8); m[1] = 4;
    assert(!kb_sample(&k, m, 0));
    m[3] = 16; assert(kb_sample(&k, m, 0) == 1);
    assert(kb_pop(&k, &e) && e.key == 10 && e.type == KB_DOWN);
    m[1] = 0; assert(kb_sample(&k, m, 0) == 1);
    assert(kb_pop(&k, &e) && e.key == 28 && e.type == KB_DOWN);
    m[3] = 0; assert(kb_sample(&k, m, 0) == 1);
    assert(kb_pop(&k, &e) && e.key == 10 && e.type == KB_UP);
    assert(kb_sample(&k, m, 0) == 1);
    assert(kb_pop(&k, &e) && e.key == 28 && e.type == KB_UP);
    assert(!kb_pop(&k, &e));

    /* Independently ready modifier and ordinary transitions retain wire order
     * and masks while an unrelated key starts or stops bouncing. */
    kb_init(&k); memset(m, 0, 8); m[1] = 128 | 4;
    assert(!kb_sample(&k, m, 0));
    m[3] = 16; assert(kb_sample(&k, m, 0) == 1);
    assert(kb_pop(&k, &e) && e.key == 15 && e.type == KB_DOWN && e.modifiers == 1);
    assert(kb_pop(&k, &e) && e.key == 10 && e.type == KB_DOWN && e.modifiers == 1);
    m[1] = m[3] = 0; assert(!kb_sample(&k, m, 0));
    m[3] = 16; assert(kb_sample(&k, m, 0) == 1);
    assert(kb_pop(&k, &e) && e.key == 10 && e.type == KB_UP && e.modifiers == 1);
    assert(kb_pop(&k, &e) && e.key == 15 && e.type == KB_UP && e.modifiers == 0);
    assert(!kb_pop(&k, &e));
}

static void test_response_overflow(void) {
    Uci u;
    uint8_t cmd[2] = {3,1}, body[140], i, result = 0;
    memset(body, 0xa5, sizeof(body));
    reset_hardware(); uci_init(&u, read_register, write_register);
    assert(uci_begin(&u, cmd, 2, 0, 100)); bounded_step(&u, 0);
    response(body, sizeof(body), "00,OK", 0);
    for (i = 1; i < 30 && !result; ++i) result = bounded_step(&u, i);
    assert(result == 3 && u.response_len == 130 && data_pos == sizeof(body));
    assert(accept_count == 1); /* Even malformed replies are drained and accepted safely. */
}

static void test_protocol(void) {
    FrameReader r;
    uint8_t frame[72], m[8] = {0,1,2,3,4,5,6,7}, n, i;
    n = kb_frame(frame, KB_SYNC, 65535, m, 8); assert(n == 16);
    kb_reader_init(&r); r.expected_seq = 65535;
    for (i = 0; i < n; ++i) assert(kb_reader_byte(&r, frame[i]) == (i == n - 1));
    kb_reader_next(&r); assert(r.expected_seq == 0);
    n = kb_frame(frame, KB_HEARTBEAT, 0, m, 8);
    for (i = 0; i < n; ++i) assert(kb_reader_byte(&r, frame[i]) == (i == n - 1));
    kb_reader_init(&r); assert(kb_reader_byte(&r, 0) == 2);
    kb_reader_init(&r); frame[4] = 65;
    for (i = 0; i < 4; ++i) assert(!kb_reader_byte(&r, frame[i]));
    assert(kb_reader_byte(&r, frame[4]) == 2);
    kb_reader_init(&r); frame[4] = 8; frame[6] = 1;
    for (i = 0; i < 7; ++i) assert(!kb_reader_byte(&r, frame[i]));
    assert(kb_reader_byte(&r, frame[7]) == 2);
}

int main(void) {
    test_commands_and_fifo(); test_multiblock_and_timeout(); test_status_and_counts();
    test_scanner_transitions(); test_ambiguity_overflow_and_bounce();
    test_independent_key_debounce(); test_protocol();
    test_response_overflow();
    puts("C64 portable driver/scanner/protocol tests passed"); return 0;
}

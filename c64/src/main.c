#include <stdint.h>
#include <string.h>
#include <conio.h>
#include "bridge.h"
#include "uci.h"

/* This is the only KBCFGv1 marker in the PRG. Root host patches these exact 64 bytes. */
uint8_t bridge_config[64] = {
    0x4b,0x42,0x43,0x46,0x47,0x76,0x31,0,
    1,64,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    0,0,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    12,67,54,52,45,85,108,116,105,109,97,116,101,0,0,0,0,0,0,0
};

extern uint8_t __fastcall__ scan_raw(uint8_t *matrix);
#ifndef BYTE
#define BYTE(address) (*(volatile uint8_t *)(address))
#endif
#define OP_IDENTIFY 1
#define OP_OPEN 2
#define OP_WRITE 3
#define OP_READ 4
#define OP_CLOSE 5
#define STATE_RETRY 0
#define STATE_ACK 1
#define STATE_READY 2
#define STATE_HALTED 3

static Uci uci;
static Keyboard keyboard;
static FrameReader reader;
static uint8_t raw[8], tx[128], payload[64], command[131];
static uint8_t tx_len, tx_pos, write_requested, socket_handle, socket_valid;
static uint8_t operation, state, enabled, need_sync, want_close;
static uint8_t quit, quit_stage, last_jiffy, saved_typeahead, saved_shiftmode;
static uint16_t now, last_scan, retry_at, retry_delay = 120, host_seen, last_read, last_hb;
static uint16_t tx_sequence, quit_started, chord_started;
static uint8_t chord_held, halted_after_close;
static uint8_t abort_notified;
#ifdef NETWORK_PROBE
static uint8_t probe_stage, ack_bytes, idle_observed;
/* Fixed 97-byte observation record: PB/version/presence/build/id + 5 UCI rows. */
static uint8_t probe_report[97];
static const uint8_t probe_bytes[32] = {
    72,69,76,76,79,32,70,82,79,77,32,67,54,52,32,85,76,84,73,77,65,84,69,10,
    0,1,127,128,255,75,66,0
};
static const uint8_t read_ok[8] = {82,69,65,68,95,79,75,10};
static const uint8_t idle_ok[8] = {73,68,76,69,95,79,75,10};
static const uint8_t probe_ack[4] = {65,67,75,0};
static void probe_observe(uint8_t row, int16_t count) {
    uint8_t i, *entry = probe_report + 32 + row * 13;
    uint16_t elapsed = now - uci.started;
    probe_report[3] |= (uint8_t)(1U << row);
    entry[0] = operation;
    for (i = 0; i < 8; ++i) entry[i + 1] = i < uci.status_len ? (uint8_t)uci.status[i] : 0;
    entry[9] = (uint8_t)count; entry[10] = (uint8_t)((uint16_t)count >> 8);
    entry[11] = (uint8_t)elapsed; entry[12] = (uint8_t)(elapsed >> 8);
}
#endif

static uint8_t mmio_read(uint16_t address) { return BYTE(address); }
static void mmio_write(uint16_t address, uint8_t value) { BYTE(address) = value; }
static void clock_update(void) {
    uint8_t j = BYTE(0xa2);
    now += (uint8_t)(j - last_jiffy);
    last_jiffy = j;
}
static void message(const char *s) {
    gotoxy(0,5); cclear(40); gotoxy(0,5); cputs(s);
}
static void show_status(void) {
    uint16_t i;
    uint8_t c;
    gotoxy(0,7); cclear(40); gotoxy(0,7);
    for (i = 0; i < uci.status_len && i < 39; ++i) {
        c = (uint8_t)uci.status[i];
        if (c >= 97 && c <= 122) c -= 32;
        cputc(c >= 32 && c < 127 ? c : 32);
    }
}
static void show_identity(void) {
    uint16_t i;
    uint8_t c;
    gotoxy(0,9); cclear(40); gotoxy(0,9);
    for (i = 0; i < uci.response_len && i < 39; ++i) {
        c = uci.response[i];
        if (c >= 97 && c <= 122) c -= 32;
        cputc(c >= 32 && c < 127 ? c : 32);
    }
}
#ifdef NETWORK_PROBE
static void show_probe_transfer(uint8_t row) {
    uint16_t elapsed = now - uci.started;
    gotoxy(0, row); cclear(40); gotoxy(0, row);
    cputs(row == 11 ? "WRITE " : "READ  ");
    if (uci.response_len >= 2) cprintf("COUNT %d", uci_count(&uci));
    else cputs("COUNT N/A");
    cprintf("  TICKS %u", elapsed);
}
#endif

static uint8_t config_valid(void) {
    uint8_t i, dot = 0, digits = 0, octet = 0;
    uint16_t value = 0;
    if (bridge_config[8] != 1 || bridge_config[9] != 64 ||
        bridge_config[44] == 0 || bridge_config[44] > 19 ||
        !(bridge_config[26] | bridge_config[27])) return 0;
    for (i = 10; i < 26; ++i) {
        if (bridge_config[i] == 0) {
            if (!digits || dot != 3) return 0;
            return 1;
        }
        if (bridge_config[i] == 46) {
            if (!digits || dot == 3) return 0;
            ++dot; value = digits = octet = 0;
        } else {
            if (bridge_config[i] < 48 || bridge_config[i] > 57) return 0;
            value = value * 10 + bridge_config[i] - 48;
            if (value > 255 || ++digits > 3) return 0;
            octet = (uint8_t)value;
        }
    }
    (void)octet;
    return 0;
}

static void start_command(uint8_t op, uint8_t len, uint16_t timeout) {
    if (uci_begin(&uci, command, len, now, timeout)) operation = op;
}
static void network_failed(const char *why) {
    message(why); show_status();
    tx_len = tx_pos = 0; enabled = 0;
    kb_clear_queue(&keyboard);
    if (socket_valid) want_close = 1;
    if (quit && socket_valid) quit_stage = 2; /* Failed flush still attempts CLOSE. */
    state = STATE_RETRY; retry_at = now + retry_delay;
    if (retry_delay < 1800) {
        retry_delay *= 2;
        if (retry_delay > 1800) retry_delay = 1800;
    }
#ifdef NETWORK_PROBE
    /* One probe run must remain reviewable; do not silently repeat observations. */
    halted_after_close = 1;
#endif
}

static void append_frame(uint8_t type, const uint8_t *body, uint8_t len) {
    uint8_t n;
    n = kb_frame(tx + tx_len, type, tx_sequence, body, len);
    tx_len += n; ++tx_sequence;
}
static void hello(void) {
    uint8_t idlen = bridge_config[44];
    memcpy(payload, bridge_config + 28, 16);
    payload[16] =
#ifdef NETWORK_PROBE
        2;
#else
        1;
#endif
    payload[17] = 0; payload[18] = idlen;
    memcpy(payload + 19, bridge_config + 45, idlen);
    tx_len = tx_pos = 0;
    append_frame(KB_HELLO, payload, idlen + 19);
}

static uint8_t received_frame(void) {
    uint8_t type = reader.bytes[3], len = reader.bytes[4], flag;
    if (state == STATE_ACK) {
        if (type != KB_ACK || len != 1 || reader.bytes[8] > 1) return 0;
        enabled = reader.bytes[8]; state = STATE_READY;
        retry_delay = 120; host_seen = now; last_hb = now;
        kb_clear_queue(&keyboard); need_sync = 1;
#ifdef NETWORK_PROBE
        probe_stage = 1; message("PROBE: SENDING TEXT + BINARY");
#else
        message(enabled ? "CONNECTED: INPUT ENABLED" : "CONNECTED: INPUT DISABLED");
#endif
        return 1;
    }
    if (state != STATE_READY) return 0;
    if (type == KB_HEARTBEAT && len == 1 && reader.bytes[8] <= 1) {
        flag = reader.bytes[8];
        if (flag != enabled) { enabled = flag; need_sync = 1; kb_clear_queue(&keyboard); }
        host_seen = now;
#ifndef NETWORK_PROBE
        message(enabled ? "CONNECTED: INPUT ENABLED" : "CONNECTED: INPUT DISABLED");
#endif
        return 1;
    }
    if (type == KB_DISCONNECT && len == 1) { network_failed("HOST DISCONNECTED"); return 1; }
    return 0;
}

static uint8_t receive_bytes(const uint8_t *data, uint8_t length) {
    uint8_t i, result;
    for (i = 0; i < length; ++i) {
#ifdef NETWORK_PROBE
        if (state == STATE_READY && probe_stage >= 2) {
            if (probe_stage != 2 || ack_bytes >= 4 || data[i] != probe_ack[ack_bytes++]) return 0;
            if (ack_bytes == 4) { probe_stage = 3; message("PROBE: ACK WITH NUL RECEIVED"); }
            continue;
        }
#endif
        result = kb_reader_byte(&reader, data[i]);
        if (result == 2) return 0;
        if (result == 1) {
            if (!received_frame()) return 0;
            kb_reader_next(&reader);
        }
    }
    return 1;
}

static void complete(uint8_t result) {
    int16_t count = 0;
    uint8_t classification;
    if (result != 1) {
        if (operation == OP_IDENTIFY) {
            state = STATE_HALTED; message("NETWORK IDENTIFICATION FAILED"); show_status(); return;
        }
        if (operation == OP_CLOSE) {
            socket_valid = want_close = 0; state = STATE_HALTED; halted_after_close = 1;
            message("CLOSE FAILED - SOCKET MAY REMAIN"); show_status();
            if (quit) quit_stage = 3;
            return;
        }
        if (operation == OP_OPEN) halted_after_close = 1; /* Unknown handle after timeout: avoid leaks. */
        network_failed(result == 2 ? "UCI TIMEOUT" : "UCI TRANSPORT FAILURE");
        if (halted_after_close && !socket_valid) state = STATE_HALTED;
        return;
    }
    show_status();
    if (operation == OP_IDENTIFY) {
        if (!uci_status_is(&uci, 0) || !uci.response_len) {
            message("NETWORK TARGET NOT AVAILABLE"); state = STATE_HALTED; return;
        }
        show_identity(); message("NETWORK TARGET IDENTIFIED"); state = STATE_RETRY; retry_at = now;
#ifdef NETWORK_PROBE
        probe_observe(0, (int16_t)uci.response_len);
        probe_report[9] = uci.response_len > 22 ? 22 : (uint8_t)uci.response_len;
        memcpy(probe_report + 10, uci.response, probe_report[9]);
#endif
    } else if (operation == OP_OPEN) {
        if (!uci_status_is(&uci, 0) || uci.response_len != 1) {
            network_failed("TCP OPEN FAILED"); return;
        }
        socket_handle = uci.response[0]; socket_valid = 1;
#ifdef NETWORK_PROBE
        probe_observe(1, (int16_t)socket_handle);
#endif
        tx_sequence = 0; kb_reader_init(&reader);
        state = STATE_ACK; host_seen = now; last_read = now;
        hello(); message("TCP OPEN: WAITING FOR HOST ACK");
    } else if (operation == OP_WRITE) {
#ifdef NETWORK_PROBE
        show_probe_transfer(11);
#endif
        if (!uci_write_result(&uci, write_requested, &count)) {
            network_failed("TCP WRITE FAILED"); return;
        }
        tx_pos += (uint8_t)count;
#ifdef NETWORK_PROBE
        if (probe_stage == 1) probe_observe(2, count);
#endif
        if (tx_pos == tx_len) { tx_len = tx_pos = 0;
#ifdef NETWORK_PROBE
            if (probe_stage == 1) { probe_stage = 2; ack_bytes = 0; }
            else if (probe_stage == 3) probe_stage = 4;
            else if (probe_stage == 5) probe_stage = 6;
#endif
            if (quit_stage == 1) quit_stage = 2;
            else if (quit_stage == 4) quit_stage = 0; /* Old frames drained; append disconnect next. */
        }
    } else if (operation == OP_READ) {
#ifdef NETWORK_PROBE
        show_probe_transfer(12);
#endif
        /* Historical READ can wait 40ms: leave a fresh gap for queued writes. */
        last_read = now;
        classification = uci_read_result(&uci, UCI_BATCH, &count);
#ifdef NETWORK_PROBE
        if (classification == 1 && probe_stage == 2) probe_observe(3, count);
#endif
        if (classification == 3) {
            /* Confirmed EOF closes the firmware FD; never close it twice. */
            socket_valid = want_close = 0;
#ifdef NETWORK_PROBE
            if (probe_stage >= 4) {
                message(idle_observed ? "PASS: TCP/BINARY/ACK/IDLE/CLOSE" : "TCP PASS; IDLE NOT OBSERVED");
                state = STATE_HALTED; return;
            }
#endif
            network_failed("HOST CLOSED TCP"); return;
        }
        if (!classification) { network_failed("TCP READ FAILED"); return; }
        if (classification == 1 && !receive_bytes(uci.response + 2, (uint8_t)count)) {
            network_failed("INVALID HOST RESPONSE"); return;
        }
#ifdef NETWORK_PROBE
        if (classification == 2 && probe_stage == 4) {
            probe_observe(4, count);
            idle_observed = 1; probe_stage = 5;
            message("PROBE: IDLE READ CONFIRMED");
        }
#endif
    } else if (operation == OP_CLOSE) {
        /* A failed close may have leaked a firmware socket; stop automatic retries. */
        socket_valid = 0; want_close = 0;
        if (!uci_status_is(&uci, 0)) halted_after_close = 1;
        if (quit) quit_stage = 3;
        else if (halted_after_close) state = STATE_HALTED;
    }
}

static void fill_tx(void) {
#ifndef NETWORK_PROBE
    KeyTransition event;
#endif
    if (tx_len || state != STATE_READY || want_close) return;
#ifdef NETWORK_PROBE
    if (probe_stage == 1) { memcpy(tx, probe_bytes, 32); tx_len = 32; }
    else if (probe_stage == 3) { memcpy(tx, read_ok, 8); tx_len = 8; }
    else if (probe_stage == 5) { memcpy(tx, idle_ok, 8); memcpy(tx + 8, probe_report, 97); tx_len = 105; }
#else
    if (need_sync) {
        append_frame(KB_SYNC, keyboard.accepted, 8); need_sync = 0;
        kb_clear_queue(&keyboard); /* Snapshot supersedes transitions collected before it. */
    }
    while (tx_len <= 118 && kb_pop(&keyboard, &event)) {
        payload[0] = event.key; payload[1] = event.modifiers;
        append_frame(event.type, payload, 2);
    }
    if (!keyboard.count && tx_len <= 112 && (uint16_t)(now - last_hb) >= 60) {
        append_frame(KB_HEARTBEAT, keyboard.accepted, 8); last_hb = now;
    }
#endif
}

static void scan(void) {
    uint8_t flags, result;
    if (now == last_scan) return;
    last_scan = now;
    flags = scan_raw(raw);
    if (!(flags & 1)) {
        if ((raw[7] & 0xa0) == 0xa0) {
            if (!chord_held) { chord_started = now; chord_held = 1; }
            if ((uint16_t)(now - chord_started) >= 60 && !quit) {
                quit = 1; quit_started = now; message("EXIT: RELEASING CONNECTION");
            }
#ifndef NETWORK_PROBE
            raw[7] &= 0x7f; /* RUN/STOP belongs to the reserved local exit chord. */
#endif
        } else chord_held = 0;
#ifndef NETWORK_PROBE
    }
    if (!(flags & 1) || (flags & 2)) {
        result = kb_sample(&keyboard, raw, flags & 2);
        if (result == 2) {
            need_sync = 1;
            message(flags & 2 ? "CONTROLLER ACTIVE: INPUT PAUSED" : "AMBIGUOUS CHORD: RELEASE ALL KEYS");
        } else if (result == 3) network_failed("EVENT QUEUE FULL: RECONNECTING");
        if (state != STATE_READY) kb_clear_queue(&keyboard);
#else
        result = 0; (void)result;
#endif
    }
}

static void schedule(void) {
    uint8_t len;
    uint16_t port;
    if (uci.phase) return;
    if (quit) {
        if (!socket_valid) { quit_stage = 3; return; }
        if (!quit_stage) {
            /* Prebuilt frames already consume sequence numbers: finish their tail. */
            if (tx_pos < tx_len) quit_stage = 4;
            else {
                tx_len = tx_pos = 0; payload[0] = 0;
                append_frame(KB_DISCONNECT, payload, 1); quit_stage = 1;
            }
        }
        if (quit_stage == 2) {
            len = uci_close_command(command, socket_handle);
            start_command(OP_CLOSE, len, 60); return;
        }
    } else if (want_close) {
        len = uci_close_command(command, socket_handle);
        start_command(OP_CLOSE, len, 300); return;
    }
    if (!quit && state == STATE_RETRY && !socket_valid && (int16_t)(now - retry_at) >= 0) {
        port = bridge_config[26] | ((uint16_t)bridge_config[27] << 8);
        len = uci_open_command(command, bridge_config + 10, port);
        if (!len) { state = STATE_HALTED; message("INVALID DESTINATION ADDRESS"); return; }
        start_command(OP_OPEN, len, 1800); message("CONNECTING TCP..."); return;
    }
    if (!socket_valid) return;
    if (!quit) fill_tx();
    if (!quit && (uint16_t)(now - last_read) >= 3) {
        len = uci_read_command(command, socket_handle, UCI_BATCH);
        start_command(OP_READ, len, 300); last_read = now; return;
    }
    if (tx_len > tx_pos) {
        write_requested = tx_len - tx_pos;
        len = uci_write_command(command, socket_handle, tx + tx_pos, write_requested);
        start_command(OP_WRITE, len, quit ? 60 : 300);
    }
}

int main(void) {
    uint8_t result;
    clrscr();
#ifdef NETWORK_PROBE
    cputs("C64 KEYBOARD BRIDGE - NETWORK TEST\r\n");
#else
    cputs("C64 KEYBOARD BRIDGE\r\n");
#endif
    cputs("HOLD COMMODORE + RUN/STOP 1S TO EXIT\r\n");
    cputs("HOST PATCHES CONFIGURATION BEFORE RUN\r\n");
    saved_typeahead = BYTE(0x289); saved_shiftmode = BYTE(0x291);
    BYTE(0x289) = 0; BYTE(0x291) = saved_shiftmode | 0x80; BYTE(0xc6) = 0;
    last_jiffy = BYTE(0xa2); kb_init(&keyboard); uci_init(&uci, mmio_read, mmio_write);
#ifdef NETWORK_PROBE
    memset(probe_report, 0, sizeof(probe_report));
    probe_report[0] = 80; probe_report[1] = 66; probe_report[2] = 1;
    probe_report[4] = 48; probe_report[5] = 46; probe_report[6] = 49;
    probe_report[7] = 46; probe_report[8] = 48; /* Build release 0.1.0. */
#endif
    if (!config_valid()) { message("NO CONFIG: DEPLOY FROM HOST"); state = STATE_HALTED; }
    else if (!uci_available(&uci)) { message("UCI NOT AVAILABLE (EXPECTED $C9)"); state = STATE_HALTED; }
    else {
        command[0] = 3; command[1] = 1;
        start_command(OP_IDENTIFY, 2, 300); state = STATE_HALTED;
    }
    while (quit_stage != 3) {
        clock_update(); scan();
        if (quit && (uint16_t)(now - quit_started) >= 120) {
            if (uci.phase || socket_valid) uci_abort(&uci, now);
            break;
        }
        if (uci.phase) {
            result = uci_step(&uci, now);
            if (result) complete(result);
            if (uci.phase == 5 && !abort_notified) {
                message("UCI ABORT PENDING - EXIT STILL WORKS"); abort_notified = 1;
            } else if (!uci.phase) abort_notified = 0;
        }
        if (!quit && state == STATE_ACK && (uint16_t)(now - host_seen) >= 300)
            network_failed("HOST HANDSHAKE TIMEOUT");
#ifndef NETWORK_PROBE
        if (!quit && state == STATE_READY && (uint16_t)(now - host_seen) >= 180)
            network_failed("HOST HEARTBEAT TIMEOUT");
#else
        if (!quit && state == STATE_READY && (uint16_t)(now - host_seen) >= 900)
            network_failed("PROBE TIMEOUT - SEE HOST REPORT");
#endif
        schedule();
    }
    BYTE(0x289) = saved_typeahead; BYTE(0x291) = saved_shiftmode; BYTE(0xc6) = 0;
    cputs("\r\nBRIDGE EXITED. BASIC KEYBOARD RESTORED.\r\n");
    return 0;
}

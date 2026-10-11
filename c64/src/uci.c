#include "uci.h"
#include <string.h>

void uci_init(Uci *u, UciRead read, UciWrite write) {
    memset(u, 0, sizeof(*u)); u->read = read; u->write = write;
}
uint8_t uci_available(Uci *u) { return u->read(UCI_COMMAND) == 0xc9; }

uint8_t uci_begin(Uci *u, const uint8_t *command, uint16_t len,
                  uint16_t now, uint16_t timeout) {
    if (u->phase || !len || len > UCI_COMMAND_MAX) return 0;
    memcpy(u->command, command, len);
    u->command_len = len; u->command_pos = u->response_len = u->status_len = 0;
    u->status[0] = 0; u->result = u->overflow = 0;
    u->started = now; u->timeout = timeout; u->phase = 1;
    return 1;
}

void uci_abort(Uci *u, uint16_t now) {
    u->write(UCI_CONTROL, 4);
    u->phase = 5; u->started = now; u->result = 2;
}

uint8_t uci_step(Uci *u, uint16_t now) {
    uint8_t s, budget = 32, byte, state;
    if (!u->phase) return u->result;
    s = u->read(UCI_CONTROL); state = s & 0x30;
    if (u->phase == 5) {
        /* ABORT is asynchronous; never permit a new command until fully idle. */
        if (!(s & 0x37)) u->phase = 0;
        return u->phase ? 0 : u->result;
    }
    if ((uint16_t)(now - u->started) >= u->timeout) {
        uci_abort(u, now); return 0;
    }
    if (s & 8) {
        u->write(UCI_CONTROL, 8);
        uci_abort(u, now); u->result = 4; return 0;
    }
    if (u->phase == 1) {
        if (state || (s & 7)) return 0;
        u->phase = 2;
    }
    if (u->phase == 2) {
        while (budget && u->command_pos < u->command_len) {
            u->write(UCI_COMMAND, u->command[u->command_pos++]); --budget;
        }
        if (u->command_pos == u->command_len) {
            u->write(UCI_CONTROL, 1); u->phase = 3;
        }
        return 0;
    }
    if (u->phase == 4) {
        if (!state && !(s & 7)) {
            u->phase = 0; u->result = u->overflow ? 3 : 1;
            return u->result;
        }
        return 0;
    }
    if (state != 0x20 && state != 0x30) return 0;
    while (budget) {
        s = u->read(UCI_CONTROL);
        if (s & 0x80) {
            byte = u->read(UCI_RESPONSE);
            if (u->response_len < UCI_RESPONSE_MAX) u->response[u->response_len++] = byte;
            else u->overflow = 1;
        } else if (s & 0x40) {
            byte = u->read(UCI_STATUS);
            if (u->status_len < UCI_STATUS_MAX - 1) {
                u->status[u->status_len++] = (char)byte;
                u->status[u->status_len] = 0;
            } else u->overflow = 1;
        } else {
            u->write(UCI_CONTROL, 2);
            u->phase = state == 0x20 ? 4 : 3;
            return 0;
        }
        --budget;
    }
    return 0;
}

uint8_t uci_status_is(const Uci *u, uint8_t code) {
    return u->status_len >= 3 && u->status[0] == (char)('0' + code / 10) &&
           u->status[1] == (char)('0' + code % 10) && u->status[2] == ',';
}
int16_t uci_count(const Uci *u) {
    return (int16_t)((uint16_t)u->response[0] | ((uint16_t)u->response[1] << 8));
}

/* Read result: 1=data, 2=no data, 3=closed, 0=invalid/failure. */
uint8_t uci_read_result(const Uci *u, uint8_t requested, int16_t *count) {
    static const uint8_t no_data[] = {48,50,44,78,79,32,68,65,84,65,58,32};
    uint16_t i, error = 0;
    if (u->response_len < 2) return 0;
    *count = uci_count(u);
    if (*count == -1 && uci_status_is(u, 2) && u->response_len == 2 &&
        u->status_len > sizeof(no_data) && !memcmp(u->status, no_data, sizeof(no_data))) {
        for (i = sizeof(no_data); i < u->status_len && u->status[i]; ++i) {
            if (u->status[i] < '0' || u->status[i] > '9' || error > 1000) return 0;
            error = error * 10 + (u->status[i] - '0');
        }
        return error == 11 ? 2 : 0;
    }
    if (*count == 0 && uci_status_is(u, 1) && u->response_len == 2) return 3;
    if (*count > 0 && *count <= requested && uci_status_is(u, 0) &&
        u->response_len == (uint16_t)(*count + 2)) return 1;
    return 0;
}
uint8_t uci_write_result(const Uci *u, uint8_t requested, int16_t *count) {
    if (u->response_len != 2 || !uci_status_is(u, 0)) return 0;
    *count = uci_count(u);
    return *count > 0 && *count <= requested;
}
uint8_t uci_open_command(uint8_t *out, const uint8_t *ip, uint16_t port) {
    uint8_t i;
    out[0] = 3; out[1] = 7; out[2] = (uint8_t)port; out[3] = (uint8_t)(port >> 8);
    for (i = 0; i < 16; ++i) {
        out[4 + i] = ip[i];
        if (!ip[i]) return i + 5;
    }
    return 0;
}
uint8_t uci_read_command(uint8_t *out, uint8_t handle, uint16_t length) {
    if (!length || length > UCI_BATCH) return 0;
    out[0] = 3; out[1] = 0x10; out[2] = handle;
    out[3] = (uint8_t)length; out[4] = (uint8_t)(length >> 8);
    return 5;
}
uint8_t uci_write_command(uint8_t *out, uint8_t handle, const uint8_t *bytes, uint8_t length) {
    if (!length || length > UCI_BATCH) return 0;
    out[0] = 3; out[1] = 0x11; out[2] = handle;
    memcpy(out + 3, bytes, length);
    return length + 3;
}
uint8_t uci_close_command(uint8_t *out, uint8_t handle) {
    out[0] = 3; out[1] = 9; out[2] = handle; return 3;
}

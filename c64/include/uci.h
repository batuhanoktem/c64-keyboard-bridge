#ifndef KB_UCI_H
#define KB_UCI_H
#include <stdint.h>

#define UCI_BATCH 128
#define UCI_COMMAND_MAX 131
#define UCI_RESPONSE_MAX 130
#define UCI_STATUS_MAX 256
#define UCI_CONTROL 0xdf1c
#define UCI_COMMAND 0xdf1d
#define UCI_RESPONSE 0xdf1e
#define UCI_STATUS 0xdf1f

typedef uint8_t (*UciRead)(uint16_t address);
typedef void (*UciWrite)(uint16_t address, uint8_t value);
typedef struct {
    uint8_t command[UCI_COMMAND_MAX], response[UCI_RESPONSE_MAX];
    char status[UCI_STATUS_MAX];
    uint16_t command_len, command_pos, response_len, status_len;
    uint16_t started, timeout;
    uint8_t phase, result, overflow;
    UciRead read;
    UciWrite write;
} Uci;

/* result: 0=pending, 1=done, 2=timeout, 3=overflow, 4=transport error. */
void uci_init(Uci *u, UciRead read, UciWrite write);
uint8_t uci_begin(Uci *u, const uint8_t *command, uint16_t length,
                  uint16_t now, uint16_t timeout);
uint8_t uci_step(Uci *u, uint16_t now);
void uci_abort(Uci *u, uint16_t now);
uint8_t uci_available(Uci *u);
uint8_t uci_status_is(const Uci *u, uint8_t code);
int16_t uci_count(const Uci *u);
uint8_t uci_read_result(const Uci *u, uint8_t requested, int16_t *count);
uint8_t uci_write_result(const Uci *u, uint8_t requested, int16_t *count);
uint8_t uci_open_command(uint8_t *out, const uint8_t *ip16, uint16_t port);
uint8_t uci_read_command(uint8_t *out, uint8_t handle, uint16_t length);
uint8_t uci_write_command(uint8_t *out, uint8_t handle, const uint8_t *bytes, uint8_t length);
uint8_t uci_close_command(uint8_t *out, uint8_t handle);

#endif

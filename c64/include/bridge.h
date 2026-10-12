#ifndef KB_BRIDGE_H
#define KB_BRIDGE_H

#include <stdint.h>
#include <stddef.h>

#define KB_PAYLOAD_MAX 64
#define KB_FRAME_MAX 72
#define KB_QUEUE_SIZE 128
#define KB_HELLO 1
#define KB_ACK 2
#define KB_DOWN 3
#define KB_UP 4
#define KB_SYNC 5
#define KB_HEARTBEAT 6
#define KB_ERROR 7
#define KB_DISCONNECT 8

typedef struct {
    uint8_t type, key, modifiers;
} KeyTransition;

typedef struct {
    uint8_t accepted[8], candidate[8];
    uint8_t blocked;
    KeyTransition queue[KB_QUEUE_SIZE];
    uint8_t head, tail, count, overflow;
} Keyboard;

typedef struct {
    uint8_t bytes[KB_FRAME_MAX];
    uint8_t used;
    uint16_t expected_seq;
} FrameReader;

uint8_t kb_modifiers(const uint8_t *matrix);
uint8_t kb_ambiguous(const uint8_t *matrix);
void kb_init(Keyboard *keyboard);
/* 0=no change, 1=accepted change, 2=interference, 3=overflow. */
uint8_t kb_sample(Keyboard *keyboard, const uint8_t *raw, uint8_t interference);
uint8_t kb_pop(Keyboard *keyboard, KeyTransition *event);
void kb_clear_queue(Keyboard *keyboard);
uint8_t kb_frame(uint8_t *out, uint8_t type, uint16_t seq,
                 const uint8_t *payload, uint8_t len);
void kb_reader_init(FrameReader *reader);
/* Feed one byte: 0=incomplete, 1=complete, 2=invalid. Completed data stays in bytes. */
uint8_t kb_reader_byte(FrameReader *reader, uint8_t byte);
void kb_reader_next(FrameReader *reader);

#endif

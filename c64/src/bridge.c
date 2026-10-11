#include "bridge.h"
#include <string.h>

uint8_t kb_modifiers(const uint8_t *m) {
    uint8_t r = 0;
    if ((m[1] & 128) || (m[6] & 16)) r |= 1;
    if (m[7] & 4) r |= 2;
    if (m[7] & 32) r |= 4;
    return r;
}

static uint8_t modifier(uint8_t key) {
    return key == 15 || key == 52 || key == 58 || key == 61;
}

uint8_t kb_ambiguous(const uint8_t *m) {
    uint8_t a, b, common;
    for (a = 0; a < 8; ++a) {
        for (b = a + 1; b < 8; ++b) {
            common = m[a] & m[b];
            if (common && (common & (common - 1))) return 1;
        }
    }
    return 0;
}

void kb_init(Keyboard *k) { memset(k, 0, sizeof(*k)); }
void kb_clear_queue(Keyboard *k) { k->head = k->tail = k->count = k->overflow = 0; }

static uint8_t append(Keyboard *k, uint8_t key, uint8_t down) {
    uint8_t column = key >> 3, mask = (uint8_t)(1U << (key & 7));
    KeyTransition *e;
    if (k->count == KB_QUEUE_SIZE) { k->overflow = 1; return 0; }
    if (down) k->accepted[column] |= mask;
    else k->accepted[column] &= (uint8_t)~mask;
    e = &k->queue[k->head];
    e->type = down ? KB_DOWN : KB_UP;
    e->key = key;
    e->modifiers = kb_modifiers(k->accepted);
    k->head = (k->head + 1) & (KB_QUEUE_SIZE - 1);
    ++k->count;
    return 1;
}

uint8_t kb_sample(Keyboard *k, const uint8_t *raw, uint8_t interference) {
    uint8_t i, key, old, next, pass, changed = 0, any = 0;
    for (i = 0; i < 8; ++i) any |= raw[i];
    if (interference || kb_ambiguous(raw)) {
        if (k->blocked) return 0;
        k->blocked = 1;
        kb_clear_queue(k);
        memset(k->accepted, 0, 8);
        k->candidate_age = 0;
        return 2;
    }
    if (k->blocked) {
        if (any) return 0;
        k->blocked = 0;
    }
    if (memcmp(k->candidate, raw, 8)) {
        memcpy(k->candidate, raw, 8);
        k->candidate_age = 1;
        return 0;
    }
    if (k->candidate_age < 2) ++k->candidate_age;
    if (k->candidate_age < 2 || !memcmp(k->accepted, raw, 8)) return 0;
    /* Release ordinary keys before modifiers; press modifiers before ordinary keys. */
    for (pass = 0; pass < 4; ++pass) {
        for (key = 0; key < 64; ++key) {
            i = key >> 3;
            old = (k->accepted[i] >> (key & 7)) & 1;
            next = (raw[i] >> (key & 7)) & 1;
            if (old == next) continue;
            if ((pass == 0 && old && !modifier(key)) ||
                (pass == 1 && old && modifier(key)) ||
                (pass == 2 && next && modifier(key)) ||
                (pass == 3 && next && !modifier(key))) {
                if (!append(k, key, next)) return 3;
                changed = 1;
            }
        }
    }
    return changed;
}

uint8_t kb_pop(Keyboard *k, KeyTransition *e) {
    if (!k->count) return 0;
    *e = k->queue[k->tail];
    k->tail = (k->tail + 1) & (KB_QUEUE_SIZE - 1);
    --k->count;
    return 1;
}

uint8_t kb_frame(uint8_t *out, uint8_t type, uint16_t seq,
                 const uint8_t *payload, uint8_t len) {
    if (len > KB_PAYLOAD_MAX || type < 1 || type > 8) return 0;
    out[0] = 0x4b; out[1] = 0x42; out[2] = 1; out[3] = type;
    out[4] = len; out[5] = 0;
    out[6] = (uint8_t)seq; out[7] = (uint8_t)(seq >> 8);
    if (len) memcpy(out + 8, payload, len);
    return len + 8;
}

void kb_reader_init(FrameReader *r) { memset(r, 0, sizeof(*r)); }
uint8_t kb_reader_byte(FrameReader *r, uint8_t b) {
    uint8_t len;
    if (r->used >= KB_FRAME_MAX) return 2;
    r->bytes[r->used++] = b;
    if ((r->used == 1 && b != 0x4b) || (r->used == 2 && b != 0x42) ||
        (r->used == 3 && b != 1) || (r->used == 4 && (b < 1 || b > 8)) ||
        (r->used == 5 && b > KB_PAYLOAD_MAX) || (r->used == 6 && b != 0)) return 2;
    if (r->used < 8) return 0;
    if (((uint16_t)r->bytes[6] | ((uint16_t)r->bytes[7] << 8)) != r->expected_seq) return 2;
    len = r->bytes[4];
    return r->used == len + 8 ? 1 : 0;
}
void kb_reader_next(FrameReader *r) { r->used = 0; ++r->expected_seq; }

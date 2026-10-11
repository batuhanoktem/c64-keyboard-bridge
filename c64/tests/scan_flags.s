.export _check_scan_flags
.import _scan_raw
.importzp ptr1
.segment "CODE"
; Verify the production scanner preserves I with interrupts both disabled and
; enabled. Restore caller's original flags; no CIA timer/IRQ changes.
_check_scan_flags:
    php
    sei
    jsr _scan_raw
    php
    pla
    and #$04
    cmp #$04
    bne failed
    lda ptr1
    ldx ptr1+1
    cli
    jsr _scan_raw
    php
    pla
    and #$04
    bne failed
    plp
    lda #1
    ldx #0
    rts
failed:
    plp
    lda #0
    ldx #0
    rts

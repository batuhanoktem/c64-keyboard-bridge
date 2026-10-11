.export _scan_raw
.importzp ptr1, tmp1, tmp2

.segment "RODATA"
masks: .byte $fe,$fd,$fb,$f7,$ef,$df,$bf,$7f

.segment "CODE"
; unsigned char __fastcall__ scan_raw(unsigned char *matrix)
; Result flags: 1=unstable sample, 2=controller interference. Never spins.
_scan_raw:
    sta ptr1
    stx ptr1+1
    php
    sei
    lda $dc02
    pha
    lda $dc03
    pha
    lda #0
    sta tmp1
    sta $dc02
    sta $dc03
    lda #$ff
    sta $dc00
    lda $dc00
    and #$1f
    cmp #$1f
    bne interference
    lda $dc01
    and #$1f
    cmp #$1f
    beq setup
interference:
    lda #2
    sta tmp1
setup:
    lda #$ff
    sta $dc02
    ldx #0
    ldy #0
column:
    lda masks,x
    sta $dc00
    nop
    nop
    lda $dc01
    sta tmp2
    cmp $dc01
    beq stable
    lda tmp1
    ora #1
    sta tmp1
stable:
    lda tmp2
    eor #$ff
    sta (ptr1),y
    iny
    inx
    cpx #8
    bne column
    ; PRA reads pins, not its latch. Restore the standard KERNAL idle selection.
    lda #$7f
    sta $dc00
    pla
    sta $dc03
    pla
    sta $dc02
    plp
    lda tmp1
    ldx #0
    rts

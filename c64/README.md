# C64 client

This directory builds two real Commodore 64 PRGs using cc65 C and ca65 assembly. They run inside the physical C64 Ultimate's C64 core. Neither PRG modifies firmware or permanent device settings.

## Build

On macOS, install the toolchain with `brew install cc65`. Linux can use its cc65 package or build the pinned upstream `V2.19` tag. Windows can use cc65 with GNU Make in MSYS2. See [cc65 installation](https://cc65.github.io/).

From the project root:

```sh
make -C c64 all test
```

Outputs are `c64/build/KEYBRIDGE.PRG` and `c64/build/NETWORKTEST.PRG`, both with the standard `$0801` load address and a BASIC `SYS` stub. The build keeps assembly intermediates and object files for the two programs separate, so parallel builds are supported. `make -C c64 BUILD=/absolute/output/directory all` selects another output directory; `CC65`, `CA65`, and `CL65` select the compiler, assembler, and linker paths. The desktop build embeds both PRGs using isolated compiler output directories.

The unpatched programs deliberately have no destination IP or port and display `NO CONFIG`. Deploy them through the desktop application's configuration patcher. Do not manually hardcode your password into this program; it contains a persistent pairing token, not the Ultimate's REST password.

## Configuration block

The linker output contains exactly one 64-byte patch block. All multi-byte values are little-endian. The host must check the marker, version and length before patching.

| Offset | Size | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII `KBCFGv1` followed by NUL |
| 8 | 1 | Block version, `1` |
| 9 | 1 | Total block length, `64` |
| 10 | 16 | ASCII IPv4 address followed by NUL, zero-padded |
| 26 | 2 | Destination TCP port |
| 28 | 16 | Opaque bridge authentication token |
| 44 | 1 | Device identifier byte length, `1..19` |
| 45 | 19 | ASCII device identifier, zero-padded |

The client validates the IPv4 address, port and identifier bounds. This first version accepts IPv4 literals, avoiding DNS ambiguity during hardware testing.

## Network feasibility probe

Run `NETWORKTEST.PRG` only through the host's dedicated probe listener. It cannot send keyboard events or request keyboard injection. Its sequence is:

1. Check `$DF1D == $C9`, then execute Network Target Identify (`03 01`) and display the returned identity.
2. Open TCP (`03 07`, port little-endian, ASCII destination and NUL).
3. Send the normal framed protocol HELLO, with capability bit `2`, the token and device identifier. Wait for framed HELLO_ACK.
4. Write exactly 32 raw bytes: ASCII `HELLO FROM C64 ULTIMATE\n` followed by `00 01 7F 80 FF 4B 42 00`.
5. Read exactly raw `41 43 4B 00` (`ACK` plus NUL). Then transmit ASCII `READ_OK\n`.
6. Wait for a signed `-1` read count and exact `02,NO DATA: 11` status, then transmit ASCII `IDLE_OK\n` followed by a fixed 97-byte `PB` version 1 observation record. It contains the build version, returned identity and five rows for Identify, Open, binary Write, ACK Read and idle Read. Each row records the operation, first eight status bytes, signed count or handle, and elapsed jiffies; partial transfers retain the last relevant transaction. See the [observation record format](../docs/protocol.md#probe-exchange-and-observation-record).
7. Continue reading until count `0` and status `01,CONNECTION CLOSED BY HOST`. Display `PASS: TCP/BINARY/ACK/IDLE/CLOSE` only after these observations. This confirms peer closure; explicit local CLOSE is exercised separately on normal exit or a non-EOF failure.

The host validates all 32 bytes before acknowledging, waits for the two reports, and subsequently closes its socket. It can record bidirectional and idle-read verification from those reports. The screen also retains the last READ and WRITE signed counts and transaction durations in jiffies, including transactions before the host handshake succeeds. Confirm peer closure on the C64 screen separately; it is not reported after that socket has closed. If idle status was not observed, the screen explicitly says so. Unexpected statuses remain errors; `00,OK` with zero data is not silently classified as a successful idle test.

Hold **Commodore + RUN/STOP for one second** to leave either PRG. RUN/STOP is suppressed from host events while this reserved chord is held. Normal exit drains prebuilt frames before appending a framed disconnect, preserving partial-write tails and sequence numbers, then attempts socket close within a two-second total cleanup deadline. A failed flush still attempts CLOSE. If firmware abort/close is still pending, the host's connection watchdog remains responsible for releasing injected keys.

## Keyboard scanning

The scanner drives CIA1 PA columns through `$FE,$FD,$FB,$F7,$EF,$DF,$BF,$7F`, reads active-low PB rows, and exports normalized eight-byte pressed bitmaps. A physical ID is `PA_column * 8 + PB_row`. Left Shift is 15, Right Shift 52, CTRL 58 and Commodore 61. Modifier masks are aggregate Shift=`1`, CTRL=`2`, Commodore=`4`.

The foreground loop requests a scan whenever the KERNAL jiffy changes; actual cadence on the physical device has not been measured. Assembly masks IRQs only during the short scan, using `PHP/SEI/PLP`, and restores the CIA direction registers. Each column is read twice; an unstable scan is skipped rather than spinning. Key changes require two consecutive matching complete observations. Ordinary releases precede modifier releases; modifier presses precede ordinary presses. Each queued transition carries the modifier mask after that transition.

UCI command building and response draining copy at most 32 bytes per scheduler step. Socket batches are at most 128 bytes. No network wait loop blocks sampling. The event queue has 128 slots; overflow closes the session instead of losing releases. Reconnect delays grow from two to thirty seconds, and the new session starts with an authoritative state snapshot. Old transitions are never replayed following that snapshot. Host heartbeats expire after three seconds.

Both joystick ports share keyboard matrix lines. The scanner tests their idle baseline and suspends forwarding when controller interference is detected. An ambiguous rectangle also clears forwarded state and suspends input until all keys are released. This conservative behavior can reject valid four-key rectangles. The actual rollover characteristics of the new Ultimate keyboard still require hardware testing.

SHIFT LOCK is observed through the compatible matrix as held Left Shift. It does not have an independent physical ID here. The Ultimate's modern keyboard may have additional firmware-specific capabilities; this application does not assume them. RESTORE is an NMI source and cannot supply normal matrix KeyDown/KeyUp. RUN/STOP+RESTORE invokes the KERNAL warm start and bypasses application cleanup; the desktop watchdog must recover from that abrupt stop. Use the documented local exit chord for graceful cleanup.

The program temporarily suppresses BASIC typeahead and the KERNAL Shift+Commodore character-set toggle, restoring those settings on normal exit. It does not take over CIA timers, IRQ/NMI vectors, CIA2, or banking. CIA port reads expose pins, not their output latches; the scanner restores standard KERNAL idle selection `$7F` instead of pretending a read can capture the previous latch. Returning to BASIC is supported; resuming the software overwritten by loading a PRG is not.

## UCI behavior and firmware compatibility

The driver uses `$DF1C` control/status, `$DF1D` command/identification, `$DF1E` response, and `$DF1F` status. It waits for the documented `Data Last`/`Data More` states, drains both queues, sends `DATA_ACC`, and observes return to idle. Status text is retained in full, with a bounded 255-byte payload. Oversized responses are drained but rejected. Signed read/write counts are checked against requested byte counts, and partial writes preserve the unsent tail, including embedded NUL bytes.

Idle reads are recognized as `-1` plus `02,NO DATA: 11`, based on the historical firmware source's lwIP EAGAIN value. Other errno values, negative write counts, zero-byte successful writes and inconsistent response lengths are errors. Only confirmed zero-count `01` host closure invalidates the socket handle: firmware closes it before returning that response, so issuing CLOSE again could target a reused descriptor. Normal exits and other failures close the owned handle explicitly. OPEN handles are accepted only on `00,OK` with exactly one returned byte.

ABORT is asynchronous. The driver continues sampling and never pushes another command until the transport is fully idle. A timed-out OPEN can leave an unknown socket in older firmware, so automatic retries halt after that uncertain operation. A failed CLOSE also halts retries. No reset or firmware update is used to recover these cases.

Sources used for an independently written driver and scanner:

- [Official UCI architecture](https://1541u-documentation.readthedocs.io/en/master/uci/core_uci_architecture.html).
- [Official Network Target protocol](https://1541u-documentation.readthedocs.io/en/master/uci/network_target.html).
- [Historical v3.14 network implementation](https://github.com/GideonZ/1541ultimate/blob/v3.14/software/io/network/network_target.cc), including signed socket counts and errno status text.
- [Ultimate keyboard implementation](https://github.com/GideonZ/1541ultimate/blob/master/software/io/c64/keyboard_c64.cc), used to confirm matrix ordering.
- [Original MOS 6526 datasheet](https://www.retrodocs.fr/wp-content/uploads/pdf/MOS-6526.pdf), describing port reads and write-only interrupt masks.
- [cc65 C64 runtime](https://cc65.github.io/doc/c64.html) and [Zlib license](https://github.com/cc65/cc65/blob/master/LICENSE).

Ultimate firmware sources are GPLv3 references; no source was copied into this implementation. Open ROMs LGPLv3 and other reference algorithms were studied without adaptation. The installed C64 Ultimate firmware 1.1.0 is not assumed equivalent to either historical or current source. The physical probe described below establishes the tested Network Target compatibility separately from those references.

## Verification status

The PRGs compile with cc65 2.19. Portable C tests cover command bytes, bounded FIFO work, response-state handling, multiple response blocks, full status collection, signed counts, binary NULs, partial writes, timeout/abort sequencing, oversized responses, all 64 physical IDs, modifier ordering, debounce, ghost suppression, queue overflow, framing, sequence wrap and malformed headers. Native tests include the production client code with mocked display, memory and matrix access, checking EOF ownership, idle reads, partial-write tails, frame order on local exit, snapshot/telemetry bounds, the one-second reserved chord and two-second cleanup deadline. These tests can also run with AddressSanitizer and UndefinedBehaviorSanitizer using the host C compiler.

A limited VICE 3.10 harness passes the production scanner at idle, preservation of the interrupt flag and CIA direction registers, and the actual compiled client loop with a mocked local exit chord held for at least 60 jiffies. The client restores `$0289` and `$0291` and clears `$C6` in that harness. Run `python3 c64/tests/run_vice.py` with VICE and C64 ROMs installed. This does not test UCI, real held keys, or returning the full production PRG to BASIC on the physical device.

On 2026-10-11, the deployed network probe passed on C64 Ultimate firmware 1.1.0, FPGA 122, core 1.49: Identify/Open, HELLO, 32 binary bytes including NUL, ACK, signed idle read, observation record and peer closure. The returned identity was `ULTIMATE-II NETWORK INTERFACE V1.0`. The C64 screen confirmed `PASS: TCP/BINARY/ACK/IDLE/CLOSE` and the connection-closed status; the user confirmed its local exit returned cleanly to BASIC. The keyboard client subsequently connected and authenticated with native input disabled. See [recorded hardware observations](../docs/hardware-testing.md#2-uci-feasibility-gate).

**Not yet verified:** physical CIA matrix behavior and native forwarding, explicit CLOSE during a connected keyboard-client exit, that client's return to BASIC, actual jiffy cadence, wall-clock network latency and reliable reconnect behavior. The earlier handshake-timeout attempt has no established cause. VICE alone does not prove Ultimate UCI Network Target behavior.

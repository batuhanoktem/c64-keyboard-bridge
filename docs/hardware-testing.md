# Acceptance testing and evidence

On 2026-10-11, authenticated REST access and a deployed `NETWORKTEST.PRG` passed the physical UCI feasibility gate on C64 Ultimate firmware 1.1.0, FPGA 122, core 1.49. The host received HELLO, all 32 binary test bytes, `READ_OK`, `IDLE_OK` and the 97-byte observation record. The C64 screen showed `PASS: TCP/BINARY/ACK/IDLE/CLOSE` and `01,CONNECTION CLOSED BY HOST`, confirming peer closure. The user then confirmed the probe's local exit chord returned cleanly to BASIC. A limited VICE 3.10 headless baseline/exit check has also passed, as described below.

The keyboard client subsequently passed the initial physical transition sequence described below with native input disabled, then reconnected and synchronized after a clean host stop/start without resetting the C64. With the user's explicit authorization, native input was enabled and the user observed TextEdit and VS Code checks pass on ARM macOS with US layout. Held Shift+A pause/resume behavior and keyboard-client local exit to BASIC were also user-confirmed. The host observed 391 raw events, no errors and no blocked combinations before stopping; native input is now disabled. A separate read-only measurement after exit observed a nominal 60 Hz KERNAL counter rate in BASIC. Broad keyboard/layout coverage, other applications/platforms, network-interruption/reconnect reliability and client scan cadence under sustained traffic remain unverified. An earlier probe attempt ended with a handshake timeout; its cause is not established. Keep that failed attempt separate from the subsequent successful exchange and observe the remaining checks before claiming full keyboard operation.

## Build and portable checks

```sh
make -C c64 all test
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

These exercise protocol fragmentation/coalescing/bounds, sequence wrap, matrix IDs, mapping ownership, Shift conflicts, repeat, cleanup, configuration, pairing, REST request formats, and portable C64 driver/scanner behavior. Cross-target `cargo check` checks Rust target compatibility without linking or running a native desktop. Preserve these distinctions in reports.

The complete scripted check is `scripts/check.sh` on macOS/Linux: pinned cc65 build, PRG provenance validation, portable C tests with sanitizers, Rust checks, and a temporary loopback recording CLI smoke test. The [script documentation](../scripts/README.md) covers Windows host-only checks and packaging. The CI definition includes native build jobs; an unexecuted workflow is not evidence of a successful job or real keyboard injection.

The real templates load at `$0801` with a BASIC SYS stub and exactly one configuration block. The limited VICE check is:

```sh
python3 c64/tests/run_vice.py
```

It requires `x64sc` on PATH, cc65 tools, and separately installed C64 ROMs; no ROM images are bundled. Set `VICE` to another emulator executable path if needed. The test compiles a separate check PRG under `c64/build/vice/` and saves `vice.log` there.

The observed VICE 3.10 result checks the production assembly scanner's idle bitmap, preservation of the processor interrupt-enable flag with both initial states, and saved CIA DDR restoration. It also compiles the production client main with a mocked local-exit matrix provider, holds that chord for at least 60 KERNAL jiffies, and checks saved `$0289`/`$0291` settings and cleared `$C6` on return. It does not emulate UCI, verify held-key scanning/ghosting, execute physical Ultimate networking, or prove hardware return to BASIC. Keep that limited result separate from the physical observations below and remaining acceptance tests.

## 1. Device and REST baseline

Set the private configuration and run `c64bridge doctor --json` and `c64bridge device info`. Record firmware, FPGA, core, host OS, backend, layout, and commit. Confirm the product is C64 Ultimate. These commands do not reset or type. Review diagnostic metadata before sharing it.

Stop any bridge host. Every subsequent deployment command resets the C64 and replaces its running program; save anything needed first. This program does not resume overwritten software on exit.

After the observed keyboard-client exit to BASIC on 2026-10-11, seven read-only REST reads of the KERNAL counter at `$A0..$A2` produced these samples. No reset, pause or input operation accompanied this measurement. Times are host HTTP request/response midpoints relative to the measurement start; HTTP round-trip times ranged from 17.959 to 22.544 ms.

| Host HTTP midpoint (seconds) | KERNAL counter |
| --- | --- |
| 0.010346 | 80018 |
| 1.031796 | 80079 |
| 2.055499 | 80140 |
| 3.078796 | 80202 |
| 4.099794 | 80263 |
| 5.121489 | 80324 |
| 6.144126 | 80386 |

The counter advanced 368 ticks over 6.133780 seconds, or 59.996 ticks per second: a nominal 60 Hz KERNAL jiffy rate observed on this physical device in BASIC. This does not measure client scan cadence under sustained traffic, keystroke-to-application latency, or video refresh rate.

## 2. UCI feasibility gate

```sh
c64bridge device test --allow-reset
```

Watch the C64 screen and confirm all of the following:

- UCI identification and a real Network Target identification response.
- TCP Open succeeds, with a valid handle and status.
- The host receives all 32 expected bytes, including embedded NUL, `7F`, `80`, and `FF`.
- The C64 receives `ACK` plus NUL and sends `READ_OK`.
- Idle read produces signed `-1` and the recognized `02,NO DATA: 11` status; the host records whether `IDLE_OK` was received.
- After host closure, the C64 observes zero count and the exact connection-closed status. Confirm the screen's complete PASS result; the host cannot verify this final screen observation over the already closed socket.
- The local exit chord returns to BASIC. Record any timeout, abort, or failed close instead of inferring success.

After `IDLE_OK`, the probe sends its fixed observation record described in the [protocol](protocol.md). The evidence includes Network Target identity, build version, five transaction status prefixes, signed counts/handle, and elapsed KERNAL jiffies. Counts and durations describe the last relevant transaction, not an aggregate partial transfer; jiffy timing is not a measured end-to-end typing latency.

The host stores `hardware-evidence.json` only after the bidirectional exchange succeeds. It separately marks idle-read verification, records `uci_observations`, and states that peer-close remains a screen check. The `probe_template_fnv1a64` identifies the unpatched embedded probe template; FNV-1a is not a cryptographic signature. The file is not evidence of physical typing or explicit CLOSE. Peer-close was observed separately on the C64 screen, and the user observed the probe return to BASIC after local exit. The later connected keyboard-client exit was also user-confirmed; its screen observations are recorded below. `doctor` recognizes previous evidence only when device and reported firmware/FPGA/core versions match.

The successful run returned Network Target identity `ULTIMATE-II NETWORK INTERFACE V1.0` and these transaction observations:

| Transaction | Signed count or handle | Elapsed KERNAL jiffies | Status |
| --- | --- | --- | --- |
| Identify | 34 bytes | 4 | `00,OK` |
| Open | Handle 6 | 6 | `00,OK` |
| Binary Write | 32 bytes | 6 | `00,OK` |
| ACK Read | 4 bytes | 3 | `00,OK` |
| Idle Read | -1 | 6 | Recognized `02,NO DATA: 11` |

The final screen retained WRITE count 105 in 10 jiffies and READ count 0 in 3 jiffies. These are transaction durations in the device's jiffy units. The separate BASIC measurement above observed a nominal 60 Hz counter rate; these durations do not measure end-to-end typing latency or client scan cadence under traffic. No private addresses, passwords or pairing tokens are included here.

If this gate fails on another run or device, retain the displayed status and investigate compatibility. A REST response, `$C9` ID, or current upstream source is not a substitute for a successful exchange. The recorded result verifies the tested network path on this device/version combination; it does not establish other firmware versions or the remaining physical keyboard checks.

## 3. Native output gate

The user observed these TextEdit checks pass on ARM macOS with US layout: `Hello123`, Return, Backspace, held A repeat stopping on release, Commodore+A selecting all and Commodore+Z undoing. Native input was explicitly authorized and the application was focused. This stage recorded 120 raw events without errors or blocked combinations.

The user then observed the requested VS Code checks behave correctly: typing; symbols `+ = * @ : ;`; Right/Left/Down/Up cursors; F1 opening the Command Palette; RUN/STOP acting as Escape; and Commodore+A selecting all. The complete session reached 391 raw events without errors or blocked combinations. Other applications, layouts and platforms still need their own acceptance tests.

Run `c64bridge permissions` and `doctor` without enabling a host. On macOS, use `permissions --request` only when you deliberately want the Accessibility prompt. Focus an empty scratch document and run:

```sh
c64bridge test-input --countdown 5
```

Observe `c64 bridge` and Enter in the focused application. Test TextEdit or VS Code on macOS, Notepad or VS Code on Windows, and both the intended desktop editor and terminal on Linux. Record actual results for each. A posted event or successful permission check does not prove delivery. Avoid holding the computer's own modifier keys while doing these tests.

## 4. Physical keyboard and mappings

The first observed physical sequence passed on the device/version combination above with native input disabled: A, Shift+A, Return, INST/DEL, cursor right, Shift+cursor right, F1 and Shift+F1. The host observed 22 balanced expected press/release transitions, including both Shift keys, without errors. This establishes matrix-to-host transitions for that sequence; it does not establish complete rollover, mappings or delivery to a native application.

Start with deliberate deployment and input:

```sh
c64bridge start --deploy --allow-reset --enable-input
```

Verify letters, digits, punctuation, Space, Return, both Shifts, CTRL, Commodore shortcuts, DEL/Shift+DEL, HOME, cursor directions, all eight function keys, and RUN/STOP. Test US and UK only with matching active input sources. Test available Unicode actions separately; document unsupported Linux text actions and authentic-profile symbol gaps.

Hold and release each key, then hold two ordinary keys and release them in each order. Confirm that repeat stops on release and changes to the last admitted repeatable key. Test CTRL plus Commodore on Windows/Linux to confirm shared native Control ownership. Test Shift+A followed by cursor and Shift+cursor followed by A: the new conflicting chord is blocked, earlier output remains held, and release restores normal typing. A modifier change that creates conflict must release output and wait for all ordinary keys to be up.

Check SHIFT LOCK as Left Shift. Test valid rollover combinations and ambiguous rectangles; record conservative rejections. Test with joystick ports idle and with each connected controller, checking that interference suspends forwarding safely. Measure keystroke-to-visible-output latency under idle and sustained traffic; do not advertise an unmeasured latency target as a result.

## 5. Cleanup, interruption, and reconnect

A clean host stop/start was observed without another C64 reset. The client reconnected, authenticated and supplied a fresh state synchronization. The user later held Shift+A during pause/resume and confirmed output stopped, remained quiet after resume until a fresh press, and then produced lowercase `a` after releasing the held keys.

The user confirmed the connected keyboard client's local exit chord returned to BASIC. A read-only screen inspection showed `EXIT: RELEASING CONNECTION`, `00,OK`, `BRIDGE EXITED. BASIC KEYBOARD RESTORED.` and `READY.`. The `00,OK` is the status displayed with that observed exit; this does not verify every firmware abort/error/close path. The host was then stopped with native input disabled. Network interruption while keys are held, repeated reconnect failures and the other unperformed cleanup cases below remain separate acceptance tests.

- Hold a key, run `pause`, and confirm output releases. Resume while a key is held; it must not type until released and pressed again.
- Hold modifiers and an ordinary key, disconnect the network, and confirm release within the traffic timeout when the OS permits it. Reconnect must establish a fresh baseline without replaying old events.
- Test host Ctrl+C and `stop`, C64 local exit, and an abrupt C64 reset/warm start. Observe application state and status after each.
- Revoke native authorization during a held chord where supported. Input must disable and report a failure; an OS-refused release cannot be called verified cleanup.
- Exercise malformed, sequence-gap, queue-overflow, and heartbeat-timeout paths in the recording harness or focused development tests before attempting equivalent hardware failures.
- Repeat on every supported host OS/backend you intend to claim, including native Wayland applications for uinput. Compilation on another target is insufficient.

Record date, commit, versions, backend, profile/layout, precise expected/observed behavior, and failure text. Keep private addresses, passwords, and tokens out of committed test reports. Leave every unperformed step explicitly unverified.

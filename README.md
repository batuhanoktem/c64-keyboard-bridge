# C64 Keyboard Bridge

Use the physical Commodore 64 Ultimate keyboard as a keyboard for a modern computer. A real C64 PRG scans CIA1, sends physical key transitions through Ultimate UCI TCP sockets, and a Rust command-line host maps them to native input. No hardware modification or firmware replacement is required by the design.

This is GPL-3.0-only software, with dependency attribution in [third-party notices](THIRD_PARTY_NOTICES.md). There is no GUI. On 2026-10-11, physical testing passed on C64 Ultimate firmware 1.1.0, FPGA 122, core 1.49 with ARM macOS and US layout: UCI binary networking, an initial key sequence, clean host restart/reconnect, TextEdit and VS Code typing/shortcuts, held-key pause/resume, and local exit to BASIC. A read-only measurement in BASIC observed a nominal 60 Hz KERNAL counter rate. The host finished with native input disabled. Broader layouts/platforms, rollover, network-interruption failures and client scan cadence under sustained traffic still require the [hardware acceptance tests](docs/hardware-testing.md). Compilation and cross-target checks do not establish those results.

A limited VICE 3.10 check passed for the production scanner at idle, preserved interrupt state/CIA directions, and the client exit path with a mocked exit chord. It does not emulate UCI or prove physical key input.

## Build

Install Rust 1.99.0, a native C/linker toolchain, and [cc65](https://cc65.github.io/) with `cl65`, `cc65`, and `ca65` on `PATH`. `rust-toolchain.toml` pins Rust 1.99.0. macOS requires 10.15 or later on Intel and 11.0 or later on Apple Silicon; `brew install cc65` provides the C64 tools. Other platform details are in [platform support](docs/platforms.md).

```sh
make -C c64 all test
cargo build --release --locked
cargo test --workspace --locked
cargo install --path crates/bridge-cli --locked
```

The desktop executable is `target/release/c64bridge` (`c64bridge.exe` on Windows). The standalone templates are `c64/build/KEYBRIDGE.PRG` and `c64/build/NETWORKTEST.PRG`; Cargo also builds and embeds both in its output directory. Unconfigured templates display `NO CONFIG`. The host patches a copy in memory before deployment. See [C64 client details](c64/README.md).

For the pinned cc65 build, sanitizers, CLI smoke checks, and release archives, use [the build and package scripts](scripts/README.md). `scripts/check.sh` runs the complete local check path on macOS/Linux; pinned PRGs are emitted under `target/c64-prgs/` and archives under `target/dist/`.

## Configure and test

```sh
cp .env.example .env
```

Edit the ignored `.env` file: set `C64_HOST` to your device's actual IPv4 address or hostname, and enter its network password privately. Do not put passwords in command arguments. Select the same US or UK input source on the computer as `KEYBOARD_LAYOUT`. Read [configuration](docs/configuration.md) for password quoting, interfaces, precedence, and defaults.

```sh
c64bridge doctor
c64bridge device info
c64bridge permissions
```

`doctor` and `permissions` inspect authorization without typing or opening a permission prompt. On macOS, explicitly request the Accessibility prompt with `c64bridge permissions --request`. A deliberate native test is `c64bridge test-input --countdown 5`: focus an empty scratch document before the countdown finishes and verify `c64 bridge` followed by Enter.

Before relying on UCI networking, stop the bridge and run:

```sh
c64bridge device test --allow-reset
```

This resets the C64 and replaces its running program with the network probe. Observe the C64 display, including peer closure and clean exit. REST success alone does not prove UCI sockets or physical keyboard operation.

## Run

`c64bridge start` runs in the foreground with native input disabled and no deployment by default. It reads device information when `AUTO_CONNECT=true`. To explicitly reset the C64, run the client, and enable native input in one command:

```sh
c64bridge start --deploy --allow-reset --enable-input
```

From another terminal using the same configuration and state directory:

```sh
c64bridge status
c64bridge pause
c64bridge resume
c64bridge stop
```

Pause, disconnect, and watchdog expiry release bridge-owned keys. Resume establishes a fresh baseline; ordinary keys already held must be released before they can type. Ctrl+C stops the host. Hold **Commodore + RUN/STOP for one second** to exit the C64 client gracefully. Abrupt C64 resets rely on the host watchdog.

`device deploy --allow-reset` loads the patched keyboard client without running it. `device run --allow-reset` loads and starts it. Both reset the C64 and interrupt its software. Do not retry an ambiguous upload failure until you inspect the device.

## Test without hardware

Start a loopback recording host in one terminal:

```sh
c64bridge start --fake-device --enable-input
```

In another terminal, run:

```sh
c64bridge --device-host 127.0.0.1 --advertise-ip 127.0.0.1 --backend recording simulate --scenario typing --fragment
c64bridge status
c64bridge stop
```

The fake host forces loopback, the recording backend, and no device contact or deployment. Other simulator scenarios are `disconnect-held`, `heartbeat-timeout`, and `malformed`. They exercise transport behavior without native input.

Read [mappings](docs/mappings.md), [protocol](docs/protocol.md), [architecture and firmware evidence](docs/architecture.md), [security](docs/security.md), [troubleshooting](docs/troubleshooting.md), and [hardware acceptance tests](docs/hardware-testing.md) for the practical limits and validation procedure.

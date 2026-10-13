# Build, check and package

Use Rust 1.99.0, Python 3.11 or newer, Git, GNU Make and a native C compiler. Run scripts from the project checkout; they resolve its root themselves. Build output and toolchain caches stay under ignored `target/` directories. They never read `.env` for build inputs.

GUI builds use eframe/Glow and the native window-system libraries. On Ubuntu 24.04 install the build dependencies before running the workspace checks:

```sh
sudo apt install pkg-config libxkbcommon-dev libwayland-dev libegl1-mesa-dev libgl1-mesa-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libdbus-1-dev
```

On macOS and Linux:

```sh
scripts/check.sh
rustup component add rust-docs --toolchain 1.99.0
scripts/package.sh
```

`check.sh` performs a pinned cc65 build, both real C64 PRG builds, portable C tests with and without AddressSanitizer/UndefinedBehaviorSanitizer, Rust formatting, clippy, workspace tests, both frontend builds, portable packaging contract tests, and the unchanged real CLI smoke test. The CLI smoke test creates a temporary state directory, forces the recording backend, and talks only to the loopback simulator. It never requests native input permission, opens a virtual keyboard device, deploys software or contacts physical hardware.

`bootstrap-cc65.sh` fetches cc65 V2.19 at full commit `555282497c3ecf8b313d87d5973093af19c35bd5`, checks tracked source changes, and builds only the compiler/assembler/linker tools and C64 runtime. It prints the source root on stdout, with build output on stderr. V2.19's tools report V2.18, so provenance is checked using the full revision and source manifests, not the displayed version. Build identification is explicitly `Git 55528249`.

`build-prgs.sh` always rebuilds both PRGs into `target/c64-prgs/`. A file lock with a 60-second wait limit prevents simultaneous invocations from overwriting intermediate assembly and object files. The build rejects source changes before publishing its manifest. `verify-prgs.py` rejects a changed load address, missing/ambiguous configuration marker, configured destination/token, or mismatch between the PRGs, source hash and pinned-build manifest.

The Windows native CI job preserves LF checkout bytes and downloads the PRGs and manifest from the Ubuntu C64 build job. Hashing orders POSIX relative path strings consistently across platforms, so Windows and Unix source manifests agree. For an equivalent local host-only check using a matching build artifact:

```sh
export C64_PRG_DIR=/path/to/verified/prgs
bash scripts/check.sh --host-only
bash scripts/package.sh --binary-only
```

This runs native Windows Rust checks and recording CLI smoke. The source-bound PRG manifest is required. GNU Make/C compiler checks for the C64 application remain covered by the Unix jobs. Windows release packaging requires Rust runtime notices (`rust-docs`) too.

## Isolated Linux X11 check

The Linux-only `x11-smoke.py` harness tests the real XTEST backend without using the logged-in desktop or a C64. It starts its own Xvfb server on a free display with a private MIT authentication cookie and `-nolisten tcp`, then creates and focuses a collector window on that display. Standard US and UK maps are applied only to this server. The harness removes Wayland session variables only from child processes that use the private display; it leaves the caller's WSLg or other desktop environment unchanged.

On Ubuntu 24.04, install the public distribution packages and run the harness as your ordinary user:

```sh
sudo apt install python3 xvfb xauth x11-xkb-utils libx11-6 x11-utils
python3 scripts/x11-smoke.py --binary target/release/c64-keyboard-bridge
```

CI uses Python 3.12 and runs this test on its Ubuntu Linux jobs. A debug executable or an extracted Linux release executable can also be supplied through `--binary`. The executable must run on the distribution's architecture and libc; the packaged Linux binaries are built on Ubuntu 24.04. For an older Ubuntu with an incompatible glibc, rebuild the matching source locally rather than changing its system libc. On other operating systems the harness reports a skip.

For each layout, the actual CLI's `test-input` command must deliver `c64 bridge` plus Return, with 11 presses and 11 releases, and leave no held keys in `XQueryKeymap`. A mismatched configured layout must refuse before producing events. After the owned server stops, a new CLI connection must fail within a process deadline.

The harness also starts the real CLI with input enabled only on that private display and admits its own authenticated loopback protocol client. The collector confirms Shift+A held before the supervisor temporarily stops its owned Xvfb process. Valid device traffic continues while the physical releases are sent. Within five seconds, the bridge must disable input and retain exactly one layout-validation failure and its failed cleanup; a device heartbeat timeout or unrelated failure cannot substitute for this result. The supervisor always resumes Xvfb during cleanup. After resuming the same server, an explicit `pause` retries bridge-owned releases. The same collector must observe ordinary-key release before modifier release and an empty keymap, while the original failed-cleanup diagnostic remains retained. Buffered requests and the retry may emit duplicate releases, so the harness does not require exactly one key-up or claim successful cleanup during the outage. Killing the server cannot satisfy the held-key cleanup assertion.

The stall collector requires XKB detectable auto-repeat on its own connection so repeat-generated key-ups cannot falsely satisfy release order. This changes event reporting for the collector only and leaves server and bridge repeat settings unchanged. See the [X.Org detectable auto-repeat specification](https://www.x.org/archive/X11R7.7/doc/libX11/XKB/xkblib.html#The_DetectableAutorepeat_Control).

All generated events target the harness-owned collector. A supervisor bounds Xlib operations in killable process groups; cleanup removes the harness's authentication and loopback pairing files and terminates its own processes. No C64 is contacted and no existing pairing state is read. Success establishes Linux input delivery and recoverable server-stall behavior on a virtual X server; it does not establish physical desktop, WSLg Weston, or Wayland uinput acceptance. See the [WSLg support distinction](../docs/platforms.md#ubuntu-in-wslg).

On 2026-10-12, the initial phrase/layout checks passed against a packaged ARM64 Linux executable in an isolated Ubuntu 24.04 container running as UID 1000, with network access disabled and no host display sockets or private configuration mounted. The established-server stall and retry extension, with collector-only detectable auto-repeat, subsequently passed under the same isolation against the packaged ARM64 executable from commit `8977ac0`. An intentional reversed modifier-before-ordinary release after native repeat was rejected by the corrected collector. A separate interruption check sent SIGTERM while owned Xvfb was stopped and confirmed bounded cleanup with no bridge, collector or Xvfb process surviving. The [acceptance record](../docs/hardware-testing.md#isolated-linux-x11-observation) lists the observed checks and their limits.

The separate recording test can be run inside Ubuntu without any display packages:

```sh
python3 scripts/cli-smoke.py --binary target/release/c64-keyboard-bridge
```

## Release packaging

`package.sh` writes portable archives, native installers and checksums to `target/dist/`, with both `c64-keyboard-bridge` and `c64-keyboard-bridge-gui`, `.env.example`, mapping templates, documentation, both unconfigured PRGs and their manifest, build provenance, crate/font notices and Rust runtime notices. It defaults to the native Rust host. `--target` accepts the five CI target triples, and executable suffixes, archive names and ZIP/tar.gz format follow that binary target. `--binary-only`, `--source-only`, and `--output DIR` are also supported. Distribute the matching source archive with GPL-covered binaries, even when creating archives in separate jobs.

Native packaging produces an `.app` plus DMG on each Mac architecture, a per-user NSIS installer on Windows x64 and an Ubuntu 24.04+ DEB on each Linux architecture. Windows packaging downloads the checksum-pinned official NSIS 3.13 portable compiler under `target/tools/` and checks `v3.13` before execution. Its Zlib runtime notices accompany the installer. Linux requires `dpkg-deb`; packages have no maintainer scripts, uinput permission changes or automatic startup. `installer-smoke.py target/dist` mounts DMGs read-only, silently installs/uninstalls NSIS on the ephemeral CI user, or installs/purges DEBs under a private dpkg root, checking that both frontends and resources match the portable package.

Ordinary CI does not use signing keys. The protected, approved release workflow is documented in [secure releases](../docs/secure-releases.md). Explicit local development signing uses the already provisioned Keychain identity without exporting it:

```sh
scripts/package.sh --binary-only --sign-macos --output target/dist-signed
python3 scripts/installer-smoke.py target/dist-signed
```

The approved identity is `Developer ID Application: Flame Lion Ltd (TUGY329T35)`. Signing verifies both nested executables and the app, then signs the DMG. To notarize, provision a local `notarytool` Keychain profile through Apple's tool and use `--notarize-profile "$C64BRIDGE_NOTARY_PROFILE"` with `--sign-macos`. Packaging never reads Apple passwords or private keys from the repository. An accepted notarization response and successful staple validation are required before DMG metadata says `notarized`; otherwise it is `signed-only` or `unsigned`. This local development packaging option notarizes the DMG only; it does not label the portable app notarized. Use the shared `scripts/release.py` promotion service for the public-release app/payload and DMG notarization gate.

`build-provenance.json` schema 2 records hashes of both packaged executables. Mac `.metadata.json` also records the signed app executable hashes and precise DMG signing/notarization status, since signing changes binary bytes. Checksums cover all portable archives, source, DMGs, installer EXEs, DEBs and Mac metadata files. Regenerate the dependency inventory with `python3 scripts/update-dependency-notices.py` after lockfile changes, then review `python3 scripts/supplement-notices.py` results for crates whose published payload omits licenses. It fetches licenses at each crate's published upstream commit and records origin URLs and hashes; packaging validates those stored hashes and commits without network access for this step. Git attributes preserve exact notice and binary artwork bytes across operating systems. Complete vendored sources and nested font/API notices are retained independently.

On macOS, an installed alternate Rust architecture target can be cross linked with the native SDK. For example, on Apple Silicon:

```sh
rustup target add x86_64-apple-darwin --toolchain 1.99.0
scripts/package.sh --binary-only --target x86_64-apple-darwin
```

Packaging sets `MACOSX_DEPLOYMENT_TARGET` to at least 10.15 for Intel and 11.0 for ARM, matching the native APIs used by the backend. A valid higher configured version is honored. The Mac CI matrix applies the same floors. Provenance records `build_host_target`, `binary_target` and the effective `minimum_os` separately. A cross-linked archive still needs execution tests on its destination architecture. The script rejects foreign operating-system cross linking; use the native CI runners for those artifacts.

The source archive uses an explicit project-source allowlist, excluding private `.env`, runtime state, generated objects and configured PRGs. It includes complete Cargo-vendored dependencies and `.cargo/config.toml`, plus the exact cc65 source tree and a source hash manifest. With build tools already installed, the included sources allow dependency builds without fetching crates or cc65. Rebuilding a source archive uses its included cc65 tree and verifies its original source files before compiling.

CI runs native jobs on macOS ARM64/x86_64, Linux ARM64/x86_64 and Windows x86_64. All GitHub actions are pinned to full commit SHAs. Artifacts are compiled and synthetically checked; native input delivery and physical C64 operation still require the acceptance tests. The ordinary CI workflow never publishes releases. The separate approved release workflow consumes exact tagged-commit artifacts, signs and notarizes both Mac architectures, and prepares a draft by default. Publication requires its explicit `publish` selection and successful final verification; see [secure releases](../docs/secure-releases.md).

## Isolated desktop GUI check

On Ubuntu 24.04, install `xvfb xauth libgl1-mesa-dri dbus-x11 python3-dbus python3-gi` and run:

```sh
python3 scripts/gui-smoke.py --binary target/debug/c64-keyboard-bridge-gui
```

This launches the actual window/event loop with Mesa software rendering on a private authenticated Xvfb display and an isolated D-Bus StatusNotifierWatcher fixture. Smoke mode uses fake services, private settings/state and a working directory without `.env`; it never connects to a C64, reads the real credential store or enables native input. The harness requires successful rendered frames, a PNG screenshot, actual tray registration and clean exit. Results and screenshots are retained under `target/gui-smoke/`. On other systems a deliberately invoked `gui-smoke.py` requires an available graphical session; the five-platform native CI always compiles and tests GUI logic, while the Linux jobs also exercise the real graphical fixtures. Physical native GUI input, real desktop tray/vault interaction and permission acceptance remain separate checks.

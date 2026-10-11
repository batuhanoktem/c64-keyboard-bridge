# Build, check and package

Use Rust 1.99.0, Python 3.11 or newer, Git, GNU Make and a native C compiler. Run scripts from the project checkout; they resolve its root themselves. Build output and toolchain caches stay under ignored `target/` directories. They never read `.env` for build inputs.

On macOS and Linux:

```sh
scripts/check.sh
rustup component add rust-docs --toolchain 1.99.0
scripts/package.sh
```

`check.sh` performs a pinned cc65 build, both real C64 PRG builds, portable C tests with and without AddressSanitizer/UndefinedBehaviorSanitizer, Rust formatting, clippy, workspace tests, and a real CLI smoke test. The smoke test creates a temporary state directory, forces the recording backend, and talks only to the loopback simulator. It never requests native input permission, opens a virtual keyboard device, deploys software or contacts physical hardware.

`bootstrap-cc65.sh` fetches cc65 V2.19 at full commit `555282497c3ecf8b313d87d5973093af19c35bd5`, checks tracked source changes, and builds only the compiler/assembler/linker tools and C64 runtime. It prints the source root on stdout, with build output on stderr. V2.19's tools report V2.18, so provenance is checked using the full revision and source manifests, not the displayed version. Build identification is explicitly `Git 55528249`.

`build-prgs.sh` always rebuilds both PRGs into `target/c64-prgs/`. A file lock with a 60-second wait limit prevents simultaneous invocations from overwriting intermediate assembly and object files. The build rejects source changes before publishing its manifest. `verify-prgs.py` rejects a changed load address, missing/ambiguous configuration marker, configured destination/token, or mismatch between the PRGs, source hash and pinned-build manifest.

The Windows native CI job preserves LF checkout bytes and downloads the PRGs and manifest from the Ubuntu C64 build job. Hashing orders POSIX relative path strings consistently across platforms, so Windows and Unix source manifests agree. For an equivalent local host-only check using a matching build artifact:

```sh
export C64_PRG_DIR=/path/to/verified/prgs
bash scripts/check.sh --host-only
bash scripts/package.sh --binary-only
```

This runs native Windows Rust checks and recording CLI smoke. The source-bound PRG manifest is required. GNU Make/C compiler checks for the C64 application remain covered by the Unix jobs. Windows release packaging requires Rust runtime notices (`rust-docs`) too.

`package.sh` writes archives and checksums to `target/dist/`, with an executable, `.env.example`, mapping templates, documentation, both unconfigured PRGs and their manifest, build provenance, crate notices and Rust runtime notices. It defaults to the native Rust host. `--target` accepts the five CI target triples, and executable suffixes, archive names and ZIP/tar.gz format follow that binary target. `--binary-only`, `--source-only`, and `--output DIR` are also supported. Distribute the matching source archive with GPL-covered binaries, even when creating archives in separate jobs.

On macOS, an installed alternate Rust architecture target can be cross linked with the native SDK. For example, on Apple Silicon:

```sh
rustup target add x86_64-apple-darwin --toolchain 1.99.0
scripts/package.sh --binary-only --target x86_64-apple-darwin
```

Packaging sets `MACOSX_DEPLOYMENT_TARGET` to at least 10.15 for Intel and 11.0 for ARM, matching the native APIs used by the backend. A valid higher configured version is honored. The Mac CI matrix applies the same floors. Provenance records `build_host_target`, `binary_target` and the effective `minimum_os` separately. A cross-linked archive still needs execution tests on its destination architecture. The script rejects foreign operating-system cross linking; use the native CI runners for those artifacts.

The source archive uses an explicit project-source allowlist, excluding private `.env`, runtime state, generated objects and configured PRGs. It includes complete Cargo-vendored dependencies and `.cargo/config.toml`, plus the exact cc65 source tree and a source hash manifest. With build tools already installed, the included sources allow dependency builds without fetching crates or cc65. Rebuilding a source archive uses its included cc65 tree and verifies its original source files before compiling.

CI runs native jobs on macOS ARM64/x86_64, Linux ARM64/x86_64 and Windows x86_64. All GitHub actions are pinned to full commit SHAs. Artifacts are compiled and synthetically checked; native input delivery and physical C64 operation still require the acceptance tests. GitHub publication occurs only on a version tag or a manual workflow run with `publish` enabled and an existing version tag pointing to the checked-out commit.

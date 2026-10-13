# Builds and release verification

The project builds the CLI, desktop companion and two genuine, unconfigured C64 PRGs. Both frontends use the same host services and embedded PRG templates. Build prerequisites and commands are in [the build guide](../scripts/README.md); contributor checks are in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Reproducible inputs

Rust and cc65 are pinned. Cargo.lock resolves Rust dependencies, and the cc65 source revision is verified before compilation. Native CI checks macOS ARM64/Intel, Windows x64 and Linux x64/ARM64. Tests, private graphical fixtures and installer checks have a different scope from physical device acceptance; see [platform support](platforms.md) and [hardware tests](hardware-testing.md).

Portable packages and installers include both frontends, unconfigured PRGs, public documentation, licensing notices, provenance and checksums. The matching corresponding-source archive includes project sources, locked vendored Rust dependencies and the pinned cc65 source. Private configuration, configured PRGs, credentials and maintainer runbooks are excluded.

## Public release gates

Ordinary pushes and pull requests build unsigned artifacts and never receive Apple signing credentials. The separately dispatched release workflow validates an existing version tag and a successful five-platform CI run for that exact commit. It verifies source bytes, PRG provenance, executable hashes and package checksums before promotion. The workflow does not create a tag.

Mac promotion requires approval through a protected GitHub environment. Signing runs on a fresh hosted Mac with restricted temporary credential storage. It consumes tested packages without rebuilding or executing their binaries, checks the approved certificate identity, and signs every executable copy and app bundle. The certificate identity and fingerprint are public checks; private keys and passwords are not part of source or packages.

Apple must accept the executable/app payload before app stapling. Shipped portable archives are extracted and verified again. DMGs are created, signed, independently notarized and stapled. Evidence distinguishes payload acceptance, app validation and DMG validation. A signed-only development package does not satisfy the public release gate.

Credential cleanup must succeed before promoted assets are uploaded. A separate job with release-write permission and no Apple credentials assembles the exact Mac, Windows, Linux and corresponding-source assets, regenerates SHA256SUMS, then downloads and verifies the complete set. Releases remain drafts unless publication is explicitly selected. Published releases are not overwritten.

The reviewed implementation is public: [native CI](https://github.com/batuhanoktem/c64-keyboard-bridge/blob/main/.github/workflows/ci.yml), [release workflow](https://github.com/batuhanoktem/c64-keyboard-bridge/blob/main/.github/workflows/release.yml), [promotion service](https://github.com/batuhanoktem/c64-keyboard-bridge/blob/main/scripts/release_service.py), [safety fixtures](https://github.com/batuhanoktem/c64-keyboard-bridge/blob/main/scripts/release-tests.py) and [packaging](https://github.com/batuhanoktem/c64-keyboard-bridge/blob/main/scripts/package.py). Account setup and credential provisioning are private maintainer operations. No credential values belong in an issue, pull request or diagnostic attachment.

## Download verification and limitations

Check downloaded assets against the release's SHA256SUMS before installation. On macOS/Linux, use sha256sum or shasum -a 256; on Windows, use Get-FileHash -Algorithm SHA256. A checksum confirms byte identity with the listed assets. Signing identifies the publisher; notarization records Apple's acceptance and does not replace source review or testing.

Windows installers remain unsigned until a Windows certificate is provided. Linux installation grants no uinput access and does not run the bridge as root. Package signature status and completed physical acceptance must be described accurately in release notes.

Report security concerns privately using [SECURITY.md](../SECURITY.md). Runtime and network assumptions are described in [security and operation](security.md).

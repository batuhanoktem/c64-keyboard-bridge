# Contributing

Bug reports, documentation improvements and focused pull requests are welcome. The project is GPL-3.0-only; contributions are distributed under the same license. Preserve attribution and dependency notices for adapted code.

## Before making a change

Read the [architecture](docs/architecture.md), [protocol](docs/protocol.md), [mapping rules](docs/mappings.md) and [platform limitations](docs/platforms.md) relevant to the change. Open an issue for substantial behavior changes so their acceptance criteria can be agreed first. Report security issues privately through [SECURITY.md](SECURITY.md).

Use Rust 1.99.0, pinned cc65 2.19, Python and a native C/linker toolchain. The [build guide](scripts/README.md) lists platform dependencies and verified toolchain bootstrapping. Run scripts/check.sh from the checkout; Windows host-only checks use the matching source-bound PRG artifact described there. Do not update dependencies or generated notices incidentally.

## Validation

Run formatting checks, Clippy, workspace tests, portable C checks and the existing CLI smoke test for implementation changes. Add tests for changed behavior and failures rather than duplicating implementation details. GUI and input checks must use isolated recording services or private graphical fixtures unless native acceptance was deliberately arranged. Five-platform CI is required before release packaging.

Do not deploy a PRG, reset a C64 or post native keyboard events during ordinary tests. Such acceptance work needs a prepared scratch application and deliberate opt-in. Keep unperformed physical tests explicit; a screenshot or successful build does not establish keyboard delivery. Never run the network application as root or silently grant device permissions.

## Pull requests

Keep commits feature-based and use Conventional Commits, for example feat(gui): describe the behavior or fix(host): describe the failure. Explain the concrete problem, resulting behavior and relevant validation in the PR. Update public documentation and CHANGELOG.md for user-visible changes. Keep version 0.1.0 until an agreed version change; do not create release tags from a feature PR.

Do not commit .env, passwords, pairing tokens, configured PRGs, signing keys or private maintainer instructions. Review logs and screenshots before attaching them: addresses, device identities and raw key traces can reveal private information. Diagnostic exports are preferred over unredacted status/debug output.

Build and signing verification remain public and auditable in [builds and release verification](docs/build-and-release.md). Maintainer account setup is separate from contributing to the source.

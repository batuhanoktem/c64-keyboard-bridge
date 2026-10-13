# Security policy

## Reporting a vulnerability

Use this repository's **Security → Report a vulnerability** form for private reports. Do not disclose exploitable details or credentials in a public issue. Include the affected commit/version, platform, a minimal reproduction and the expected security boundary. Use synthetic credentials and redact device addresses, pairing tokens, raw key traces and personal paths.

If you accidentally publish an active password, pairing token or signing credential, revoke or rotate it through the relevant service. Removing a Git commit cannot invalidate a copied credential.

## Support and boundaries

Version 0.1.0 is in development; there is no published stable release or response-time guarantee. Security fixes target the current main branch until a released-version support policy is announced.

The bridge posts input only after deliberate enabling. Pairing and device REST traffic require a trusted LAN and are not encrypted. Permission or backend failures must disable forwarding and retain cleanup evidence. See [security and operation](docs/security.md) for the complete runtime assumptions and [release verification](docs/build-and-release.md) for the build/signing gates.

No private credentials, configured PRGs or maintainer runbooks belong in corresponding-source or binary packages. Public release scripts and certificate identity checks are available for review.

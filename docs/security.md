# Security and operation

The bridge can type into the computer's focused application once explicitly enabled. Use a trusted local network and an IP allowlist limited to the C64. Keep native input paused while investigating unexpected state.

The C64 TCP connection carries a persistent, random 128-bit pairing token in HELLO. The host also checks the peer's IPv4 address and admits only one client at a time. Authentication precedes state synchronization and all keyboard events. Rotation requires a stopped host and redeployment. The REST password is separate: it stays on the host and is never inserted into either PRG.

This protocol provides no encryption, challenge-response, or replay protection across sessions. A network observer able to capture the token can impersonate a client if it can also satisfy the peer allowlist. REST uses the device's HTTP API with `X-Password`, so that password is also exposed to an observer on the path. Neither service should be forwarded onto the public Internet.

The REST client disables proxies, redirects and automatic request retries. It marks the password header sensitive, limits responses to 64 KiB, and requires both HTTP success and an empty JSON `errors` array. A failed disruptive request still requires device inspection before a deliberate retry.

The local control listener binds loopback on an ephemeral port and authenticates using the same token. Its location and PID are in the private runtime file. Unix state permissions are enforced; Windows relies on the private account directory's ACLs. Do not share `.env`, `pairing.key`, configured PRGs, or unredacted diagnostic output containing private device identifiers and addresses.

Native injection is off at each normal startup. Deployment is also off by default. A deployment request requires `--allow-reset`, including when `AUTO_DEPLOY=true`; both load and run reset the C64 and replace its software. No firmware update, permanent device setting change, or automatic recovery reset is performed. An upload timeout can mean the device already performed the action; inspect it before retrying.

Bridge-owned output is released on pause, session failure, disconnect, and host shutdown. Missing traffic expires after three seconds. Mapper conflicts reject a new incompatible ordinary chord while preserving previously admitted output; a modifier change that makes existing chords incompatible releases output and quarantines held ordinary keys until they are released. Backend failures disable input and attempt cleanup; if the OS refuses releases, the bridge reports the failure rather than proving that the application has no held keys.

The C64 client suspends forwarding on ambiguous matrix rectangles or controller interference. These precautions can reject legitimate rollover combinations. Commodore+RUN/STOP held for one second is a reserved local exit chord. RESTORE has no ordinary matrix transition; an abrupt warm start or reset bypasses local cleanup and relies on the host watchdog. Full behavioral acceptance is covered by [hardware testing](hardware-testing.md).

There is no host keyboard capture, keylogger, GUI, or password-bearing CLI argument. Default logs omit key text and authentication tokens. Diagnostic reports contain device and network metadata, so review them before sharing.

The retained session `diagnostics` object contains typed reasons, counters, phases, host timings and cleanup outcomes. It excludes credentials, peer addresses, device-provided text, key identities and matrix bytes. Only one failure record is retained in memory. The surrounding status and doctor reports still include their existing device/network metadata and must be reviewed before sharing. A successful cleanup outcome means bridge-owned native releases succeeded or no keys were owned; it does not prove delivery to a particular application.

`LOG_LEVEL=debug` records physical C64 key identities, worker decisions and accepted Windows native key events, including repeat. These traces can reveal text typed through the bridge. Keep them private and review them before sharing; credentials and Unicode action content are not logged by these traces.

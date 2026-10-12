# Configuration

Configuration comes from CLI overrides, then process environment variables, then the selected dotenv file, then defaults. The default file is `.env` in the current directory. Use `--env-file PATH` consistently for commands that manage the same bridge. Dotenv parsing does not execute a shell.

| Variable | Default | Meaning |
| --- | --- | --- |
| `C64_HOST` | unset | Required for device/network operations; IPv4 address or hostname, without a URL scheme or port. |
| `C64_PORT` | `80` | Device REST port, 1–65535. |
| `C64_NETWORK_PASSWORD` | empty | Device Network Password, sent only in `X-Password`. |
| `HOST_ADVERTISE_IP` | route-derived | Computer IPv4 address the C64 should connect to. |
| `BRIDGE_BIND_ADDRESS` | advertised address | Listener interface. An explicit `0.0.0.0` binds every IPv4 interface. |
| `BRIDGE_PORT` | `6502` | TCP listener port, 1–65535. |
| `C64_ALLOWED_IPS` | resolved device addresses | Optional comma-separated IPv4 allowlist. |
| `AUTO_CONNECT` | `true` | Fetch device information during normal startup. |
| `AUTO_DEPLOY` | `false` | Request deployment during startup; still requires `--allow-reset`. |
| `KEYBOARD_LAYOUT` | `US` | `US` or `UK`; must match the profile and supported active host input source. |
| `KEYBOARD_PROFILE` | `modern` | `modern` or `authentic`. |
| `MAPPINGS_FILE` | unset | Custom TOML profile, replacing the selected built-in profile. |
| `INPUT_BACKEND` | `auto` | `auto`, `recording`, `macos`, `windows`, `x11`, or `uinput`. |
| `REPEAT_DELAY_MS` | `500` | Host repeat delay, 100–5000 ms; macOS/Windows/recording only. |
| `REPEAT_INTERVAL_MS` | `40` | Host repeat interval, 10–1000 ms. Linux uses desktop repeat settings. |
| `LOG_LEVEL` | `info` | Tracing filter. Default logs omit key text; DEBUG key traces can reveal bridge typing and must remain private. Passwords are not logged. |
| `BRIDGE_STATE_DIR` | OS application data | Private directory for pairing and runtime state. |

Global CLI overrides are `--device-host`, `--bind-address`, `--advertise-ip`, `--bridge-port`, `--layout`, `--profile`, and `--backend`. They are available on every subcommand. There is deliberately no password option. Input authorization is separate from configuration: each host starts disabled unless `start --enable-input` is supplied, and `resume` deliberately enables it.

Custom profile layout and `KEYBOARD_LAYOUT` must agree. Native backends verify the active layout before admitting input and periodically while enabled. Unsupported variants, an unknown layout, or a switch away from the selected layout disables input and attempts release. The recording backend needs no OS layout. See [platform support](platforms.md) for exact supported input sources and Wayland limitations.

## Password quoting

Enter the password directly in an editor. Single quotes preserve literal dollar signs, hash signs, spaces, equals signs, backslashes, and double quotes. A single-quoted value cannot itself contain a single quote. For that case, surround the value with double quotes and escape every literal dollar sign as `\$`, double quote as `\"`, and backslash as `\\`; a single quote needs no escape inside double quotes. Newlines and control characters are rejected because the value must be a valid HTTP header.

Dotenv variable expansion is enabled outside single quotes, including inside double quotes. An unescaped `$NAME` can change the password. Do not run `source .env`; dotenv syntax and shell syntax are different. The file can contain punctuation safely when the above rules are followed. Process environment values override the file and may explain an unexpected setting.

## Interfaces and state

If automatic routing selects the wrong adapter, set both the advertised address and bind interface to the computer's actual reachable IPv4 interface. Open the chosen bridge port in the local firewall only for the C64's address. The device initiates the TCP connection to the computer. No router port forwarding is needed on a normal local network.

The host stores `pairing.key`, `runtime.json` while running, and optional `hardware-evidence.json` under its application data directory. On Unix the directory must have mode `0700` and the token mode `0600`; newly created files use `0600`. On Windows the state directory inherits account ACLs, which should restrict access to the current user. A stale runtime record is handled as described in [troubleshooting](troubleshooting.md).

`pair rotate` requires the host to be stopped, replaces the persistent token, and requires client redeployment. Changing the advertised address or bridge port also requires redeployment. The host patches a copy of the embedded PRG; no configured PRG or credential is written into the repository.

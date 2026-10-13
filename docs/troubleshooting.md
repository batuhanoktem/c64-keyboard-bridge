# Troubleshooting

Start with `c64-keyboard-bridge doctor` or `c64-keyboard-bridge doctor --json`, then `c64-keyboard-bridge status` if a host is running. The report distinguishes REST reachability, native authorization, listener availability, and saved UCI probe evidence. Successful API calls or permission checks cannot establish delivery to an application.

| Symptom | Check |
| --- | --- |
| `C64_HOST is missing` | Edit the chosen `.env`, or pass a non-secret `--device-host`. |
| Invalid dotenv/password | Quote the entire password using the [configuration rules](configuration.md). Check process environment overrides. Do not paste the password into logs. |
| REST authentication failed | Match the device's Network Password. HTTP 403/401 is distinct from TCP bridge authentication. |
| REST endpoint unavailable | Confirm installed firmware and REST service. No automatic update is attempted. |
| Listener port in use | Check `status`, stop an existing host, or choose another `BRIDGE_PORT` and redeploy. |
| C64 never connects | Check its displayed UCI identity/status, reachable advertised interface, allowlist, firewall, and client destination after redeployment. |
| C64 reconnects or times out intermittently | Inspect `status` diagnostics for the latest normalized failure, phase, valid-traffic age and cleanup outcome. The failure remains available after reconnect; keep input paused while investigating. |
| Client displays `NO CONFIG` | An unpatched PRG was run. Deploy through `device run --allow-reset` or startup deployment. |
| Probe fails after REST success | UCI mapping and Network Target are separate facilities. Verify the device's Command Interface setting and inspect exact C64 statuses. |
| Input disabled | Normal startup is disabled. Run `resume` after deliberate authorization; inspect any backend error first. |
| Enabled but no text | Focus the intended application, match its input source, and run the explicit `test-input` test. Check platform authorization/integrity requirements. |
| Windows reports missing `VCRUNTIME140.dll` | Install Microsoft's supported [x64 Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist/). The release archive requires this system runtime. |
| Host layout rejected or changed | Select a supported standard US/UK source matching configuration and profile, then deliberately resume. IMEs and unknown variants are rejected. |
| XTEST refuses `DISPLAY` | Use the intended local AF_UNIX display, such as `:0` or `unix:0`, in that desktop session. Remote/TCP and SSH-forwarded displays such as `localhost:10.0` are unsupported. |
| X11 operation exceeds 500 ms | Inspect the local X server and its authorization/session state. Connection/setup and request/reply waits use nonblocking I/O and finite polling; input disables on failure. Restore the connection before deliberately retrying cleanup/resume. |
| Wayland uinput has unverified layout | Use supported Sway/Hyprland and its query tool; configure the actual bridge device to standard US/UK. Other compositors are currently unsupported. |
| Wrong `@`, quotes, or symbols | Match US/UK input sources; macOS British differs from PC British. Authentic mapping has documented gaps. |
| Some simultaneous keys stop forwarding | Release every ordinary key. Check consumed-Shift conflicts, ghost rectangles, and controller interference. |
| Unicode action is blocked on Linux | Linux text support is absent; use physical-key mappings or a supported platform. Check the blocked-combination counter. |
| No repeat on Linux | Repeat is controlled by desktop/compositor settings. |
| Authentication failed after rotation | Redeploy the client with the new persistent token. |
| `runtime.json` already exists | Run `status`; confirm that PID/process is stopped before removing only the stale runtime file. Preserve `pairing.key`. |
| Private directory/token error | On Unix, set the state directory to `0700` and token to `0600`; do not use a shared or symlinked state location. |
| Timed-out OPEN or failed CLOSE | Inspect the C64. Retries halt when firmware socket ownership is uncertain; no recovery reset is automatic. |
| Upload request failed | The C64 may already have loaded/run the PRG. Inspect it before issuing another reset-bearing request. |

Pause and stop attempt to release every bridge-owned native key. If permission revocation or a native API failure prevents release, inspect the reported cleanup failure and the application's actual state. If the C64 client is reset or warm-started abruptly, wait for the three-second traffic watchdog. Graceful exit is Commodore+RUN/STOP held for one second.

On X11, a failed release keeps ownership recorded and disables input; a timeout is not evidence that the server received a key-up. The transport's mock tests and Linux cross-compilation do not verify real-server input or cleanup. Observe those cases in the intended Linux desktop before claiming acceptance.

For a transport-only reproduction, use the README's loopback recording host and simulator. `--fragment` sends frames one byte at a time. `disconnect-held`, `heartbeat-timeout`, and `malformed` exercise cleanup paths without synthetic native input. Reproducing those scenarios does not establish real UCI or OS behavior.

`status` retains failure diagnostics for the running process even when `detail` changes to a reconnect or pause message. `doctor` summarizes the same runtime information when the host is reachable. HELLO and initial synchronization timeouts are distinct from the three-second valid-traffic watchdog and a host write timeout. Transport, authentication, protocol and input-queue failures have separate normalized reasons. Cleanup may still be `pending`; a `failed` outcome must not be treated as release of all keys. Cleanup acknowledgments are associated with the failed generation so delayed work cannot overwrite a newer failure record.

Counters reset on process restart, and the latest record is replaced by a later failure. Capture the relevant status before stopping the process if you need it for a bug report, reviewing the surrounding metadata before sharing. These diagnostics cannot reconstruct the causes of warnings from older builds.

When reporting a bug, include the commit, host OS, selected backend/layout/profile, non-secret error text, firmware/FPGA/core versions, and which acceptance step failed. Review diagnostics for private addresses or identifiers. Never include `.env`, the pairing token, or configured PRGs.

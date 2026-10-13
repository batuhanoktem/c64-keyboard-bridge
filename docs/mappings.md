# Keyboard mappings

Choose `KEYBOARD_LAYOUT=US` or `UK` and select the matching supported host input source. The bridge verifies that source but does not switch it. `KEYBOARD_PROFILE=modern` is the default; `authentic` translates selected shifted C64 number legends and adds Unicode arrow actions. It does not implement a PETSCII terminal or every original graphics character. Authentic UK Shift+3 uses Option+3 on macOS and the physical hash key on Windows/Linux.

| Physical C64 key | Host action |
| --- | --- |
| Letters, digits, Space, Return | Matching physical native key; Return becomes Enter. |
| Left/Right Shift | Native Left/Right Shift unless a selected action consumes Shift. |
| CTRL | Control. |
| Commodore | Meta/Command on macOS; Control on Windows/Linux. |
| RUN/STOP | Escape, except the reserved Commodore+RUN/STOP exit chord. |
| DEL / Shift+DEL | Backspace / forward Delete. |
| Cursor right / Shift+cursor right | Right / Left. |
| Cursor down / Shift+cursor down | Down / Up. |
| F1, F3, F5, F7 / shifted | F1, F3, F5, F7 / F2, F4, F6, F8. |
| HOME | Home. |
| `+`, `=`, `*`, `@` | Native chords producing those symbols under the selected host layout. |
| `:` / shifted `:` | Colon / left bracket. |
| `;` / shifted `;` | Semicolon / right bracket. |
| `£` | UK Shift+3; US Unicode `£`. |
| Up-arrow / left-arrow, modern | Caret / grave key. |
| Up-arrow / left-arrow, authentic | Unicode `↑` / `←`. |

Shift used to select a cursor direction, even function key, or fixed symbol is consumed by that action, so Shift+cursor does not also select host text. Existing output must agree on a common native Shift state. If a new ordinary chord conflicts, that chord is blocked until its physical release and earlier output remains held. If a modifier transition makes existing chords incompatible, all output is released and the mapper waits until every ordinary key is up. Releasing one of two source keys that own the same native modifier does not release the other owner's modifier.

Unicode actions require a backend with text support and are separate from native shortcut chords. Overlapping native ordinary keys, CTRL, or Commodore block a new text action. Physical Shift alone can be suppressed for a text action and restored afterwards. Linux has no Unicode text support in this version; known unsupported text actions are blocked until physical release, incrementing the blocked-combination counter while preserving other held output. Choose UK or provide physical-key overrides when appropriate. An unexpected native API failure still disables input and attempts cleanup. Host repeat applies only to admitted ordinary actions; native desktop repeat is used on Linux.

## Custom TOML

The shipped templates are in [`config/layouts`](../config/layouts). Inspect and validate them with:

```sh
c64-keyboard-bridge mappings list
c64-keyboard-bridge mappings validate config/layouts/modern-us.toml
```

Set `MAPPINGS_FILE` to your profile path and validate it before startup. Its layout must match `KEYBOARD_LAYOUT`. Paths are relative to the current working directory. A profile uses a built-in base and optional ordinary-key overrides:

```toml
name = "Personal US"
profile = "modern"
layout = "US"
commodore_modifier = "Alt"
control_modifier = "Control"

[[bindings]]
key = "RunStop"
keys = ["Escape"]
shift = "suppress"

[[bindings]]
key = "A"
modifiers = ["Commodore"]
keys = ["B"]
shift = "physical"
```

That second override produces Alt+B because the selected Commodore modifier remains held. Source modifier roles apply globally; a binding adds native keys rather than removing physical CTRL/Commodore. Roles may be `Control`, `Alt`, or `Meta`. Bindings cannot replace the physical modifier keys themselves.

Source `key` names are the exact names in the [protocol matrix table](protocol.md). `modifiers` is an optional required subset of `Shift`, `Control`, and `Commodore`; the most specific matching rule wins. A rule can use either `keys` with 1–8 unique native enum names or `text` with 1–16 non-control characters. Native names include `A`–`Z`, `Num0`–`Num9`, `Enter`, `Backspace`, `Escape`, `ArrowLeft`, `F1`–`F12`, `LeftShift`, `RightShift`, `Control`, `Alt`, `Meta`, and punctuation names such as `Quote` and `IntlBackslash`; the complete enum is in `crates/bridge-core/src/lib.rs`.

`shift` is `physical`, `required`, or `suppress`. An explicit native Shift key implies `required`. Unknown fields, duplicate keys, impossible Shift declarations, and equally specific ambiguous rules are rejected. Output chosen on KeyDown is stored until KeyUp; changing physical Shift does not silently select a different release target.

# Third-party notices

C64 Keyboard Bridge's independently written Rust host, C64 application, tests and build scripts are licensed under **GPL-3.0-only**; see [LICENSE](LICENSE). Third-party components retain their own copyright notices and licenses. This document identifies the dependencies locked for version 0.1.0 and the linked C64 runtime. It does not replace the component license texts.

## Rust dependency inventory

The following inventory was checked against `Cargo.lock` and published crate manifests on 2026-10-11. It includes every registry package in the lockfile, including transitive, test-only and platform-specific packages. A particular binary links only the packages selected by its target and enabled features. The four `bridge-*` workspace packages are first-party GPL-3.0-only source and are excluded from this table.

The expressions preserve upstream metadata, including legacy `MIT/Apache-2.0` spelling. `OR` denotes alternatives; `AND` denotes cumulative requirements. Where MIT is offered as an alternative, this project's release distribution uses the MIT option while retaining supplied upstream license files. Apache-only, Unicode and MPL components retain those licenses. This choice does not remove other options offered by upstream.

| Package | Locked version | Declared license expression | Repository |
| --- | --- | --- | --- |
| aho-corasick | 1.1.5 | `Unlicense OR MIT` | [upstream](https://github.com/BurntSushi/aho-corasick) |
| anstream | 1.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle | 1.0.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle-parse | 1.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle-query | 1.1.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle-wincon | 3.0.11 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anyhow | 1.0.104 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/anyhow) |
| atomic-waker | 1.1.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/atomic-waker) |
| base64 | 0.23.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/marshallpierce/rust-base64) |
| bitflags | 2.13.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/bitflags/bitflags) |
| bumpalo | 3.20.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/fitzgen/bumpalo) |
| bytes | 1.12.1 | `MIT` | [upstream](https://github.com/tokio-rs/bytes) |
| cfg-if | 1.0.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/cfg-if) |
| clap | 4.6.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clap_builder | 4.6.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clap_derive | 4.6.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clap_lex | 1.1.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| colorchoice | 1.0.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| core-foundation | 0.10.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-foundation-sys | 0.8.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-graphics | 0.25.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-graphics-types | 0.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| directories | 6.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/soc/directories-rs) |
| dirs-sys | 0.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/dirs-dev/dirs-sys-rs) |
| displaydoc | 0.2.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/yaahc/displaydoc) |
| dotenvy | 0.15.7 | `MIT` | [upstream](https://github.com/allan2/dotenvy) |
| equivalent | 1.0.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/indexmap-rs/equivalent) |
| errno | 0.3.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/lambda-fairy/rust-errno) |
| fastrand | 2.5.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/fastrand) |
| foreign-types | 0.5.0 | `MIT/Apache-2.0` | [upstream](https://github.com/sfackler/foreign-types) |
| foreign-types-macros | 0.2.4 | `MIT/Apache-2.0` | [upstream](https://github.com/sfackler/foreign-types) |
| foreign-types-shared | 0.3.1 | `MIT/Apache-2.0` | [upstream](https://github.com/sfackler/foreign-types) |
| form_urlencoded | 1.2.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url) |
| futures-channel | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-core | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-task | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-util | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| gethostname | 1.1.0 | `Apache-2.0` | [upstream](https://codeberg.org/swsnr/gethostname.rs.git) |
| getrandom | 0.4.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-random/getrandom) |
| hashbrown | 0.17.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/hashbrown) |
| heck | 0.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/withoutboats/heck) |
| http | 1.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/hyperium/http) |
| http-body | 1.1.0 | `MIT` | [upstream](https://github.com/hyperium/http-body) |
| http-body-util | 0.1.5 | `MIT` | [upstream](https://github.com/hyperium/http-body) |
| httparse | 1.10.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/seanmonstar/httparse) |
| hyper | 1.12.0 | `MIT` | [upstream](https://github.com/hyperium/hyper) |
| hyper-util | 0.1.21 | `MIT` | [upstream](https://github.com/hyperium/hyper-util) |
| icu_collections | 2.3.0 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| icu_locale_core | 2.3.0 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| icu_normalizer | 2.3.0 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| icu_normalizer_data | 2.3.0 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| icu_properties | 2.3.0 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| icu_properties_data | 2.3.0 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| icu_provider | 2.3.1 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| idna | 1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url/) |
| idna_adapter | 1.2.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/hsivonen/idna_adapter) |
| indexmap | 2.14.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/indexmap-rs/indexmap) |
| ipnet | 2.12.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/krisprice/ipnet) |
| is_terminal_polyfill | 1.70.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/polyfill-rs/is_terminal_polyfill) |
| itoa | 1.0.18 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/itoa) |
| js-sys | 0.3.106 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) |
| lazy_static | 1.5.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang-nursery/lazy-static.rs) |
| libc | 0.2.190 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/libc) |
| libredox | 0.1.25 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/libredox.git) |
| linux-raw-sys | 0.12.1 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/sunfishcode/linux-raw-sys) |
| litemap | 0.8.3 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| log | 0.4.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/log) |
| matchers | 0.2.0 | `MIT` | [upstream](https://github.com/hawkw/matchers) |
| memchr | 2.8.3 | `Unlicense OR MIT` | [upstream](https://github.com/BurntSushi/memchr) |
| mio | 1.2.4 | `MIT` | [upstream](https://github.com/tokio-rs/mio) |
| nu-ansi-term | 0.50.3 | `MIT` | [upstream](https://github.com/nushell/nu-ansi-term) |
| once_cell | 1.21.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/matklad/once_cell) |
| once_cell_polyfill | 1.70.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/polyfill-rs/once_cell_polyfill) |
| option-ext | 0.2.0 | `MPL-2.0` | [upstream](https://github.com/soc/option-ext.git) |
| percent-encoding | 2.3.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url/) |
| pin-project-lite | 0.2.17 | `Apache-2.0 OR MIT` | [upstream](https://github.com/taiki-e/pin-project-lite) |
| potential_utf | 0.1.6 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| proc-macro2 | 1.0.107 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/proc-macro2) |
| quote | 1.0.47 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/quote) |
| r-efi | 6.0.0 | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [upstream](https://github.com/r-efi/r-efi) |
| redox_users | 0.5.3 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/users) |
| regex-automata | 0.4.18 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/regex) |
| regex-syntax | 0.8.11 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/regex) |
| reqwest | 0.13.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/seanmonstar/reqwest) |
| rustix | 1.1.5 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/rustix) |
| rustversion | 1.0.23 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/rustversion) |
| serde | 1.0.229 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/serde) |
| serde_core | 1.0.229 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/serde) |
| serde_derive | 1.0.229 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/serde) |
| serde_json | 1.0.154 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/json) |
| serde_spanned | 1.1.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| sharded-slab | 0.1.7 | `MIT` | [upstream](https://github.com/hawkw/sharded-slab) |
| signal-hook-registry | 1.4.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/vorner/signal-hook) |
| slab | 0.4.12 | `MIT` | [upstream](https://github.com/tokio-rs/slab) |
| smallvec | 1.16.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-smallvec) |
| socket2 | 0.6.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/socket2) |
| stable_deref_trait | 1.2.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/storyyeller/stable_deref_trait) |
| strsim | 0.11.1 | `MIT` | [upstream](https://github.com/rapidfuzz/strsim-rs) |
| syn | 2.0.119 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/syn) |
| syn | 3.0.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/syn) |
| sync_wrapper | 1.0.2 | `Apache-2.0` | [upstream](https://github.com/Actyx/sync_wrapper) |
| synstructure | 0.14.0 | `MIT` | [upstream](https://github.com/mystor/synstructure) |
| tempfile | 3.27.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/Stebalien/tempfile) |
| thiserror | 2.0.21 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/thiserror) |
| thiserror-impl | 2.0.21 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/thiserror) |
| thread_local | 1.1.10 | `MIT OR Apache-2.0` | [upstream](https://github.com/Amanieu/thread_local-rs) |
| tinystr | 0.8.4 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| tokio | 1.53.2 | `MIT` | [upstream](https://github.com/tokio-rs/tokio) |
| tokio-macros | 2.7.2 | `MIT` | [upstream](https://github.com/tokio-rs/tokio) |
| toml | 1.1.8+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| toml_datetime | 1.1.2+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| toml_parser | 1.1.5+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| toml_writer | 1.1.3+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| tower | 0.5.3 | `MIT` | [upstream](https://github.com/tower-rs/tower) |
| tower-http | 0.6.11 | `MIT` | [upstream](https://github.com/tower-rs/tower-http) |
| tower-layer | 0.3.3 | `MIT` | [upstream](https://github.com/tower-rs/tower) |
| tower-service | 0.3.3 | `MIT` | [upstream](https://github.com/tower-rs/tower) |
| tracing | 0.1.44 | `MIT` | [upstream](https://github.com/tokio-rs/tracing) |
| tracing-attributes | 0.1.31 | `MIT` | [upstream](https://github.com/tokio-rs/tracing) |
| tracing-core | 0.1.36 | `MIT` | [upstream](https://github.com/tokio-rs/tracing) |
| tracing-log | 0.2.0 | `MIT` | [upstream](https://github.com/tokio-rs/tracing) |
| tracing-subscriber | 0.3.23 | `MIT` | [upstream](https://github.com/tokio-rs/tracing) |
| try-lock | 0.2.5 | `MIT` | [upstream](https://github.com/seanmonstar/try-lock) |
| unicode-ident | 1.0.26 | `(MIT OR Apache-2.0) AND Unicode-3.0` | [upstream](https://github.com/dtolnay/unicode-ident) |
| url | 2.5.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url) |
| utf8_iter | 1.0.4 | `Apache-2.0 OR MIT` | [upstream](https://github.com/hsivonen/utf8_iter) |
| utf8parse | 0.2.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/alacritty/vte) |
| valuable | 0.1.1 | `MIT` | [upstream](https://github.com/tokio-rs/valuable) |
| want | 0.3.2 | `MIT` | [upstream](https://github.com/seanmonstar/want) |
| wasi | 0.11.1+wasi-snapshot-preview1 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/wasi) |
| wasm-bindgen | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen) |
| wasm-bindgen-futures | 0.4.79 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures) |
| wasm-bindgen-macro | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) |
| wasm-bindgen-macro-support | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support) |
| wasm-bindgen-shared | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) |
| web-sys | 0.3.106 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys) |
| windows-link | 0.2.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.61.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| winnow | 1.0.4 | `MIT` | [upstream](https://github.com/winnow-rs/winnow) |
| writeable | 0.6.4 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| x11rb | 0.13.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/psychon/x11rb) |
| x11rb-protocol | 0.13.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/psychon/x11rb) |
| yoke | 0.8.3 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| yoke-derive | 0.8.4 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerofrom | 0.1.8 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerofrom-derive | 0.1.8 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerotrie | 0.2.5 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerovec | 0.11.8 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerovec-derive | 0.11.6 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zmij | 1.0.23 | `MIT` | [upstream](https://github.com/dtolnay/zmij) |

The release source archive preserves complete Cargo-vendored package payloads, including manifests, source headers and embedded notices. Binary archives retain collected upstream license, copyright and notice files, including nested files. A package-level expression alone does not enumerate all embedded third-party notices.

Specific notice material that must be preserved:

| Package | Additional notice files |
| --- | --- |
| atomic-waker 1.1.2 | `LICENSE-THIRD-PARTY`, alongside MIT and Apache license files |
| core-graphics 0.25.0 | `COPYRIGHT`, alongside license files |
| linux-raw-sys 0.12.1 and rustix 1.1.5 | `COPYRIGHT` and all supplied license/exception files |
| regex-syntax 0.8.11 | `src/unicode_tables/LICENSE-UNICODE` |
| tracing-core 0.1.36 | `src/spin/LICENSE` |
| unicode-ident 1.0.26 | `LICENSE-UNICODE`, alongside MIT/Apache alternatives |
| utf8_iter 1.0.4 | `COPYRIGHT`, alongside license files |
| r-efi 6.0.0 | `AUTHORS` contains the complete MIT grant, disclaimer and copyright notices. Retain `AUTHORS`, `README.md` and manifest metadata; the published crate has no separate `LICENSE` file. |
| valuable 0.1.1 | Retain `README.md` and manifest metadata. Its published crate omits the `LICENSE` referenced by its README. The complete MIT notice is reproduced below from the exact upstream commit recorded in `.cargo_vcs_info.json`. |

`option-ext` 0.2.0 is distributed under MPL-2.0. Its source is included unchanged in the vendored source archive with `LICENSE.txt` and source notices preserved. Recipients may obtain that source from the corresponding source archive accompanying the binary release. MPL-covered source retains MPL-2.0 terms; the project license does not erase those rights. See [option-ext's MPL-2.0 license](https://github.com/soc/option-ext/blob/master/LICENSE.txt), sections 3.1–3.4.

### Valuable MIT notice

Source: [valuable LICENSE at commit 9efc29b6e58cef28f6566a47aa7e142a55fead77](https://github.com/tokio-rs/valuable/blob/9efc29b6e58cef28f6566a47aa7e142a55fead77/LICENSE). This supplements the omitted file in the published valuable 0.1.1 crate; it does not modify its source.

```text
Copyright (c) 2021 Valuable Contributors

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## Rust toolchain runtime

The host executable also uses Rust standard-library and compiler runtime components outside `Cargo.lock`. Their attribution is supplied by the exact Rust toolchain used to build the release. Binary archives preserve `share/doc/rust/COPYRIGHT-library.html` and its referenced `share/doc/rust/licenses/` directory from `rustc --print sysroot`. The project pins Rust 1.99.0; a different toolchain must supply its own notices and be recorded in build provenance. The crate inventory is not a substitute for runtime notices.

Operating-system frameworks and system libraries remain supplied by the operating system. The releases do not bundle Apple frameworks, Windows system DLLs, the Linux kernel or X server implementations.

## cc65 C64 runtime

The generated C64 PRGs link C64 startup and runtime code from **cc65 V2.19**. cc65 and its runtime use the **Zlib** license. Credit belongs to the cc65 authors and contributors, including original author Ullrich von Bassewitz; the bridge does not claim authorship of their code. Per-file notices identify further contributors.

The release source archive includes the exact, unmodified upstream cc65 source at commit `555282497c3ecf8b313d87d5973093af19c35bd5`, including original notices and build files. A changed runtime or alternative toolchain must be identified and its matching source/notices retained. Reference: [cc65 V2.19 LICENSE](https://github.com/cc65/cc65/blob/V2.19/LICENSE).

The complete cc65 V2.19 license is:

```text
This software is provided 'as-is', without any express or implied warranty.
In no event will the authors be held liable for any damages arising from
the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
claim that you wrote the original software. If you use this software in
a product, an acknowledgment in the product documentation would be
appreciated but is not required.

2. Altered source versions must be plainly marked as such, and must not
be misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.
```

## Reference material and original implementation

The UCI driver and matrix scanner were independently written using published interfaces and behavioral reference material. No source copied or adapted from Ultimate firmware or Open ROMs is incorporated. Those references retain their own licenses. This notice does not relicense or distribute the C64 Ultimate's installed firmware or Commodore ROMs.

Technical references are attributed in [c64/README.md](c64/README.md), including the [official UCI architecture](https://1541u-documentation.readthedocs.io/en/master/uci/core_uci_architecture.html), [Network Target protocol](https://1541u-documentation.readthedocs.io/en/master/uci/network_target.html), [historical Ultimate network implementation](https://github.com/GideonZ/1541ultimate/blob/v3.14/software/io/network/network_target.cc) and [Ultimate keyboard implementation](https://github.com/GideonZ/1541ultimate/blob/master/software/io/c64/keyboard_c64.cc). These identify consulted sources, not bundled dependencies or evidence of a firmware version's behavior.

## Corresponding source with binary releases

When conveying a GPL-covered executable or PRG, provide matching corresponding source by a method permitted by GPLv3 section 6. The release layout supplies a source archive alongside each binary archive. Preserve exact project source, lockfile, complete vendored crate sources, matching cc65 runtime source, build scripts and instructions needed to regenerate that release. Retain the GPL license and upstream notices. Do not label a source archive as matching a binary built from different inputs.

For download distribution, make matching source available with the binary, with clear instructions and equivalent access. An upstream link or this notice alone is not the project's corresponding source distribution. Full definitions and terms, including System Libraries and general-purpose tools, appear in [LICENSE](LICENSE), sections 1 and 6.

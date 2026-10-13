# Third-party notices

C64 Keyboard Bridge's independently written Rust host, C64 application, tests and build scripts are licensed under **GPL-3.0-only**; see [LICENSE](LICENSE). Third-party components retain their own copyright notices and licenses. This document identifies the dependencies locked for version 0.1.0 and the linked C64 runtime. It does not replace the component license texts.

## Rust dependency inventory

The following inventory is generated from the resolved `Cargo.lock` and published crate manifests with `scripts/update-dependency-notices.py`. It includes every registry package in the lockfile, including transitive, test-only and platform-specific packages. A particular binary links only the packages selected by its target and enabled features. The `bridge-*` workspace packages are first-party GPL-3.0-only source and are excluded from this table.

The expressions preserve upstream metadata, including legacy `MIT/Apache-2.0` spelling. `OR` denotes alternatives; `AND` denotes cumulative requirements. Where MIT is offered as an alternative, this project's release distribution uses the MIT option while retaining supplied upstream license files. Apache-only, Unicode and MPL components retain those licenses. This choice does not remove other options offered by upstream.

| Package | Locked version | Declared license expression | Repository |
| --- | --- | --- | --- |
| accesskit | 0.24.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_atspi_common | 0.18.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_consumer | 0.35.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_consumer | 0.36.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_consumer | 0.38.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_macos | 0.26.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_unix | 0.21.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_windows | 0.32.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| accesskit_winit | 0.32.2 | `Apache-2.0` | [upstream](https://github.com/AccessKit/accesskit) |
| adler2 | 2.0.1 | `0BSD OR MIT OR Apache-2.0` | [upstream](https://github.com/oyvindln/adler2) |
| aes | 0.9.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/block-ciphers) |
| ahash | 0.8.12 | `MIT OR Apache-2.0` | [upstream](https://github.com/tkaitchuck/ahash) |
| aho-corasick | 1.1.5 | `Unlicense OR MIT` | [upstream](https://github.com/BurntSushi/aho-corasick) |
| android-activity | 0.6.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-mobile/android-activity) |
| android-properties | 0.2.2 | `MIT` | [upstream](https://github.com/miklelappo/android-properties) |
| anstream | 1.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle | 1.0.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle-parse | 1.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle-query | 1.1.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anstyle-wincon | 3.0.11 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| anyhow | 1.0.104 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/anyhow) |
| apple-native-keyring-store | 1.0.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/open-source-cooperative/apple-native-keyring-store.git) |
| arboard | 3.6.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/1Password/arboard) |
| arrayvec | 0.7.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/bluss/arrayvec) |
| as-raw-xcb-connection | 1.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/psychon/as-raw-xcb-connection) |
| ashpd | 0.13.13 | `MIT` | [upstream](https://github.com/bilelmoussaoui/ashpd) |
| async-broadcast | 0.7.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/smol-rs/async-broadcast) |
| async-channel | 2.5.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-channel) |
| async-executor | 1.14.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-executor) |
| async-io | 2.6.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-io) |
| async-lock | 3.4.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-lock) |
| async-process | 2.5.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-process) |
| async-recursion | 1.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/dcchut/async-recursion) |
| async-signal | 0.2.14 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-signal) |
| async-task | 4.7.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/async-task) |
| async-trait | 0.1.92 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/async-trait) |
| atomic-waker | 1.1.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/atomic-waker) |
| atspi | 0.29.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/odilia-app/atspi) |
| atspi-common | 0.13.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/odilia-app/atspi) |
| atspi-proxies | 0.13.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/odilia-app/atspi) |
| autocfg | 1.5.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/cuviper/autocfg) |
| base64 | 0.23.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/marshallpierce/rust-base64) |
| bit-set | 0.10.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/contain-rs/bit-set) |
| bit-vec | 0.9.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/contain-rs/bit-vec) |
| bitflags | 1.3.2 | `MIT/Apache-2.0` | [upstream](https://github.com/bitflags/bitflags) |
| bitflags | 2.13.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/bitflags/bitflags) |
| block-buffer | 0.12.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/utils) |
| block-padding | 0.4.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/utils) |
| block2 | 0.5.1 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| block2 | 0.6.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| blocking | 1.7.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/blocking) |
| bumpalo | 3.20.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/fitzgen/bumpalo) |
| bytemuck | 1.25.2 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/Lokathor/bytemuck) |
| bytemuck_derive | 1.12.1 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/Lokathor/bytemuck) |
| byteorder | 1.5.0 | `Unlicense OR MIT` | [upstream](https://github.com/BurntSushi/byteorder) |
| byteorder-lite | 0.1.0 | `Unlicense OR MIT` | [upstream](https://github.com/image-rs/byteorder-lite) |
| bytes | 1.12.1 | `MIT` | [upstream](https://github.com/tokio-rs/bytes) |
| calloop | 0.13.0 | `MIT` | [upstream](https://github.com/Smithay/calloop) |
| calloop | 0.14.5 | `MIT` | [upstream](https://github.com/Smithay/calloop) |
| calloop-wayland-source | 0.3.0 | `MIT` | [upstream](https://github.com/smithay/calloop-wayland-source) |
| calloop-wayland-source | 0.4.1 | `MIT` | [upstream](https://github.com/smithay/calloop-wayland-source) |
| cbc | 0.2.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/block-modes) |
| cc | 1.8.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/cc-rs) |
| cfg-if | 1.0.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/cfg-if) |
| cfg_aliases | 0.2.2 | `MIT` | [upstream](https://github.com/katharostech/cfg_aliases) |
| cgl | 0.3.2 | `MIT / Apache-2.0` | [upstream](https://github.com/servo/cgl-rs) |
| cipher | 0.5.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/traits) |
| clap | 4.6.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clap_builder | 4.6.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clap_derive | 4.6.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clap_lex | 1.1.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/clap-rs/clap) |
| clipboard-win | 5.4.1 | `BSL-1.0` | [upstream](https://github.com/DoumanAsh/clipboard-win) |
| cmov | 0.5.4 | `Apache-2.0 OR MIT` | [upstream](https://github.com/RustCrypto/utils) |
| codespan-reporting | 0.13.1 | `Apache-2.0` | [upstream](https://github.com/brendanzab/codespan) |
| color | 0.3.3 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/color) |
| colorchoice | 1.0.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-cli/anstyle.git) |
| combine | 4.6.8 | `MIT` | [upstream](https://github.com/Marwes/combine) |
| concurrent-queue | 2.5.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/concurrent-queue) |
| const-oid | 0.10.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/RustCrypto/formats) |
| core-foundation | 0.10.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-foundation | 0.9.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-foundation-sys | 0.8.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-graphics | 0.23.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-graphics | 0.25.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-graphics-types | 0.1.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| core-graphics-types | 0.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/core-foundation-rs) |
| cpubits | 0.1.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/utils) |
| cpufeatures | 0.3.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/utils) |
| crc32fast | 1.5.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/srijs/rust-crc32fast) |
| crossbeam-channel | 0.5.17 | `MIT OR Apache-2.0` | [upstream](https://github.com/crossbeam-rs/crossbeam) |
| crossbeam-utils | 0.8.23 | `MIT OR Apache-2.0` | [upstream](https://github.com/crossbeam-rs/crossbeam) |
| crunchy | 0.2.4 | `MIT` | [upstream](https://github.com/eira-fransham/crunchy) |
| crypto-common | 0.2.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/traits) |
| ctutils | 0.4.3 | `Apache-2.0 OR MIT` | [upstream](https://github.com/RustCrypto/utils) |
| cursor-icon | 1.2.0 | `MIT OR Apache-2.0 OR Zlib` | [upstream](https://github.com/rust-windowing/cursor-icon) |
| digest | 0.11.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/traits) |
| directories | 6.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/soc/directories-rs) |
| dirs-sys | 0.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/dirs-dev/dirs-sys-rs) |
| dispatch | 0.2.0 | `MIT` | [upstream](http://github.com/SSheldon/rust-dispatch) |
| dispatch2 | 0.3.1 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/madsmtm/objc2) |
| displaydoc | 0.2.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/yaahc/displaydoc) |
| dlib | 0.5.3 | `MIT` | [upstream](https://github.com/elinorbgr/dlib) |
| document-features | 0.2.12 | `MIT OR Apache-2.0` | [upstream](https://github.com/slint-ui/document-features) |
| dotenvy | 0.15.7 | `MIT` | [upstream](https://github.com/allan2/dotenvy) |
| downcast-rs | 1.2.1 | `MIT/Apache-2.0` | [upstream](https://github.com/marcianx/downcast-rs) |
| dpi | 0.1.2 | `Apache-2.0 AND MIT` | [upstream](https://github.com/rust-windowing/winit) |
| ecolor | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui) |
| eframe | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/eframe) |
| egui | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui) |
| egui-wgpu | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/egui-wgpu) |
| egui-winit | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/egui-winit) |
| egui_glow | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/egui_glow) |
| either | 1.19.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rayon-rs/either) |
| emath | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/emath) |
| endi | 1.1.1 | `MIT` | [upstream](https://github.com/zeenix/endi) |
| enumflags2 | 0.7.12 | `MIT OR Apache-2.0` | [upstream](https://github.com/meithecatte/enumflags2) |
| enumflags2_derive | 0.7.12 | `MIT OR Apache-2.0` | [upstream](https://github.com/meithecatte/enumflags2) |
| epaint | 0.36.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/epaint) |
| epaint_default_fonts | 0.36.2 | `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` | [upstream](https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts) |
| equivalent | 1.0.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/indexmap-rs/equivalent) |
| errno | 0.3.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/lambda-fairy/rust-errno) |
| error-code | 3.4.0 | `BSL-1.0` | [upstream](https://github.com/DoumanAsh/error-code) |
| euclid | 0.22.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/euclid) |
| event-listener | 5.4.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/event-listener) |
| event-listener-strategy | 0.5.4 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/event-listener-strategy) |
| fastrand | 2.5.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/fastrand) |
| fax | 0.2.7 | `MIT` | [upstream](https://github.com/pdf-rs/fax) |
| fdeflate | 0.3.7 | `MIT OR Apache-2.0` | [upstream](https://github.com/image-rs/fdeflate) |
| fearless_simd | 0.4.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/fearless_simd) |
| find-msvc-tools | 0.1.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/cc-rs) |
| flate2 | 1.1.10 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/flate2-rs) |
| foldhash | 0.2.0 | `Zlib` | [upstream](https://github.com/orlp/foldhash) |
| font-types | 0.12.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/googlefonts/fontations) |
| foreign-types | 0.5.0 | `MIT/Apache-2.0` | [upstream](https://github.com/sfackler/foreign-types) |
| foreign-types-macros | 0.2.4 | `MIT/Apache-2.0` | [upstream](https://github.com/sfackler/foreign-types) |
| foreign-types-shared | 0.3.1 | `MIT/Apache-2.0` | [upstream](https://github.com/sfackler/foreign-types) |
| form_urlencoded | 1.2.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url) |
| futures-channel | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-core | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-io | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-lite | 2.6.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/futures-lite) |
| futures-macro | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-task | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| futures-util | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/futures-rs) |
| gethostname | 1.1.0 | `Apache-2.0` | [upstream](https://codeberg.org/swsnr/gethostname.rs.git) |
| getrandom | 0.3.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-random/getrandom) |
| getrandom | 0.4.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-random/getrandom) |
| gl_generator | 0.14.0 | `Apache-2.0` | [upstream](https://github.com/brendanzab/gl-rs/) |
| glifo | 0.2.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/vello) |
| glow | 0.17.0 | `MIT OR Apache-2.0 OR Zlib` | [upstream](https://github.com/grovesNL/glow) |
| glutin | 0.32.3 | `Apache-2.0` | [upstream](https://github.com/rust-windowing/glutin) |
| glutin-winit | 0.5.0 | `MIT` | [upstream](https://github.com/rust-windowing/glutin) |
| glutin_egl_sys | 0.7.1 | `Apache-2.0` | [upstream](https://github.com/rust-windowing/glutin) |
| glutin_glx_sys | 0.6.1 | `Apache-2.0` | [upstream](https://github.com/rust-windowing/glutin) |
| glutin_wgl_sys | 0.6.1 | `Apache-2.0` | [upstream](https://github.com/rust-windowing/glutin) |
| guillotiere | 0.7.0 | `MIT/Apache-2.0` | [upstream](https://github.com/nical/guillotiere) |
| half | 2.7.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/VoidStarKat/half-rs) |
| harfrust | 0.12.0 | `MIT` | [upstream](https://github.com/harfbuzz/harfrust) |
| hashbrown | 0.16.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/hashbrown) |
| hashbrown | 0.17.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/hashbrown) |
| heck | 0.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/withoutboats/heck) |
| hermit-abi | 0.5.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/hermit-os/hermit-rs) |
| hex | 0.4.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/KokaKiwi/rust-hex) |
| hkdf | 0.13.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/KDFs/) |
| hmac | 0.13.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/MACs) |
| http | 1.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/hyperium/http) |
| http-body | 1.1.0 | `MIT` | [upstream](https://github.com/hyperium/http-body) |
| http-body-util | 0.1.5 | `MIT` | [upstream](https://github.com/hyperium/http-body) |
| httparse | 1.10.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/seanmonstar/httparse) |
| hybrid-array | 0.4.15 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/hybrid-array) |
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
| image | 0.25.10 | `MIT OR Apache-2.0` | [upstream](https://github.com/image-rs/image) |
| indexmap | 2.14.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/indexmap-rs/indexmap) |
| inout | 0.2.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/utils) |
| ipnet | 2.12.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/krisprice/ipnet) |
| is_terminal_polyfill | 1.70.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/polyfill-rs/is_terminal_polyfill) |
| itertools | 0.15.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-itertools/itertools) |
| itoa | 1.0.18 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/itoa) |
| jni | 0.22.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/jni-rs/jni-rs) |
| jni-macros | 0.22.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/jni-rs/jni-rs) |
| jni-sys | 0.3.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/jni-rs/jni-sys) |
| jni-sys | 0.4.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/jni-rs/jni-sys) |
| jni-sys-macros | 0.4.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/jni-rs/jni-sys) |
| jobserver | 0.1.35 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/jobserver-rs) |
| js-sys | 0.3.106 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) |
| keyboard-types | 0.8.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-windowing/keyboard-types) |
| keyring | 4.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/open-source-cooperative/keyring-rs) |
| keyring-core | 1.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/open-source-cooperative/keyring-core.git) |
| khronos_api | 3.1.0 | `Apache-2.0` | [upstream](https://github.com/brendanzab/gl-rs/) |
| ksni | 0.3.6 | `Unlicense` | [upstream](https://github.com/iovxw/ksni) |
| kurbo | 0.13.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/kurbo) |
| lazy_static | 1.5.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang-nursery/lazy-static.rs) |
| libc | 0.2.190 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/libc) |
| libloading | 0.8.9 | `ISC` | [upstream](https://github.com/nagisa/rust_libloading/) |
| libm | 0.2.16 | `MIT` | [upstream](https://github.com/rust-lang/compiler-builtins) |
| libredox | 0.1.25 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/libredox.git) |
| linebender_resource_handle | 0.1.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/raw_resource_handle) |
| linux-raw-sys | 0.12.1 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/sunfishcode/linux-raw-sys) |
| linux-raw-sys | 0.4.15 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/sunfishcode/linux-raw-sys) |
| litemap | 0.8.3 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| litrs | 1.0.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/LukasKalbertodt/litrs) |
| lock_api | 0.4.14 | `MIT OR Apache-2.0` | [upstream](https://github.com/Amanieu/parking_lot) |
| log | 0.4.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/log) |
| matchers | 0.2.0 | `MIT` | [upstream](https://github.com/hawkw/matchers) |
| memchr | 2.8.3 | `Unlicense OR MIT` | [upstream](https://github.com/BurntSushi/memchr) |
| memmap2 | 0.9.11 | `MIT OR Apache-2.0` | [upstream](https://github.com/RazrFalcon/memmap2-rs) |
| memoffset | 0.9.1 | `MIT` | [upstream](https://github.com/Gilnaa/memoffset) |
| miniz_oxide | 0.8.9 | `MIT OR Zlib OR Apache-2.0` | [upstream](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) |
| miniz_oxide | 0.9.1 | `MIT OR Zlib OR Apache-2.0` | [upstream](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) |
| mio | 1.2.4 | `MIT` | [upstream](https://github.com/tokio-rs/mio) |
| moxcms | 0.8.1 | `BSD-3-Clause OR Apache-2.0` | [upstream](https://github.com/awxkee/moxcms.git) |
| muda | 0.21.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/tauri-apps/muda) |
| naga | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| naga-types | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| ndk | 0.9.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-mobile/ndk) |
| ndk-context | 0.1.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-windowing/android-ndk-rs) |
| ndk-sys | 0.6.0+11769913 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-mobile/ndk) |
| nohash-hasher | 0.2.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/paritytech/nohash-hasher) |
| nu-ansi-term | 0.50.3 | `MIT` | [upstream](https://github.com/nushell/nu-ansi-term) |
| num | 0.4.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num) |
| num-bigint | 0.4.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num-bigint) |
| num-complex | 0.4.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num-complex) |
| num-integer | 0.1.47 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num-integer) |
| num-iter | 0.1.46 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num-iter) |
| num-rational | 0.4.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num-rational) |
| num-traits | 0.2.19 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-num/num-traits) |
| num_enum | 0.7.6 | `BSD-3-Clause OR MIT OR Apache-2.0` | [upstream](https://github.com/illicitonion/num_enum) |
| num_enum_derive | 0.7.6 | `BSD-3-Clause OR MIT OR Apache-2.0` | [upstream](https://github.com/illicitonion/num_enum) |
| objc-sys | 0.3.5 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2 | 0.5.3 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2 | 0.6.5 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-app-kit | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-app-kit | 0.3.2 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-cloud-kit | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-contacts | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-core-data | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-core-foundation | 0.3.2 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-core-graphics | 0.3.2 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-core-image | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-core-location | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-encode | 4.1.0 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-foundation | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-foundation | 0.3.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-io-surface | 0.3.2 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-link-presentation | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-metal | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-quartz-core | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-symbols | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-ui-kit | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-ui-kit | 0.3.2 | `Zlib OR Apache-2.0 OR MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-uniform-type-identifiers | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| objc2-user-notifications | 0.2.2 | `MIT` | [upstream](https://github.com/madsmtm/objc2) |
| once_cell | 1.21.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/matklad/once_cell) |
| once_cell_polyfill | 1.70.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/polyfill-rs/once_cell_polyfill) |
| option-ext | 0.2.0 | `MPL-2.0` | [upstream](https://github.com/soc/option-ext.git) |
| orbclient | 0.3.55 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/orbclient) |
| ordered-stream | 0.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/danieldg/ordered-stream) |
| parking | 2.2.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/parking) |
| parking_lot | 0.12.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/Amanieu/parking_lot) |
| parking_lot_core | 0.9.12 | `MIT OR Apache-2.0` | [upstream](https://github.com/Amanieu/parking_lot) |
| pastey | 0.2.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/as1100k/pastey) |
| peniko | 0.6.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/peniko) |
| percent-encoding | 2.3.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url/) |
| phf | 0.13.1 | `MIT` | [upstream](https://github.com/rust-phf/rust-phf) |
| phf_generator | 0.13.1 | `MIT` | [upstream](https://github.com/rust-phf/rust-phf) |
| phf_macros | 0.13.1 | `MIT` | [upstream](https://github.com/rust-phf/rust-phf) |
| phf_shared | 0.13.1 | `MIT` | [upstream](https://github.com/rust-phf/rust-phf) |
| pin-project | 1.1.13 | `Apache-2.0 OR MIT` | [upstream](https://github.com/taiki-e/pin-project) |
| pin-project-internal | 1.1.13 | `Apache-2.0 OR MIT` | [upstream](https://github.com/taiki-e/pin-project) |
| pin-project-lite | 0.2.17 | `Apache-2.0 OR MIT` | [upstream](https://github.com/taiki-e/pin-project-lite) |
| piper | 0.2.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/smol-rs/piper) |
| pkg-config | 0.3.34 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/pkg-config-rs) |
| plain | 0.2.3 | `MIT/Apache-2.0` | [upstream](https://github.com/randomites/plain) |
| png | 0.18.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/image-rs/image-png) |
| polling | 3.11.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/smol-rs/polling) |
| polycool | 0.4.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/linebender/kurbo) |
| portable-atomic | 1.15.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/taiki-e/portable-atomic) |
| portable-atomic-util | 0.2.8 | `Apache-2.0 OR MIT` | [upstream](https://github.com/taiki-e/portable-atomic-util) |
| potential_utf | 0.1.6 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| proc-macro-crate | 3.5.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/bkchr/proc-macro-crate) |
| proc-macro2 | 1.0.107 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/proc-macro2) |
| profiling | 1.0.18 | `MIT OR Apache-2.0` | [upstream](https://github.com/aclysma/profiling) |
| pxfm | 0.1.30 | `BSD-3-Clause OR Apache-2.0` | [upstream](https://github.com/awxkee/pxfm) |
| quick-error | 2.0.1 | `MIT/Apache-2.0` | [upstream](http://github.com/tailhook/quick-error) |
| quick-xml | 0.41.0 | `MIT` | [upstream](https://github.com/tafia/quick-xml) |
| quote | 1.0.47 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/quote) |
| r-efi | 5.3.0 | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [upstream](https://github.com/r-efi/r-efi) |
| r-efi | 6.0.0 | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [upstream](https://github.com/r-efi/r-efi) |
| raw-window-handle | 0.6.2 | `MIT OR Apache-2.0 OR Zlib` | [upstream](https://github.com/rust-windowing/raw-window-handle) |
| read-fonts | 0.41.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/googlefonts/fontations) |
| redox_syscall | 0.4.1 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/syscall) |
| redox_syscall | 0.5.18 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/syscall) |
| redox_syscall | 0.9.4 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/kernel) |
| redox_users | 0.5.3 | `MIT` | [upstream](https://gitlab.redox-os.org/redox-os/users) |
| regex | 1.13.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/regex) |
| regex-automata | 0.4.18 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/regex) |
| regex-syntax | 0.8.11 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/regex) |
| reis | 0.7.1 | `MIT` | [upstream](https://github.com/ids1024/reis) |
| renderdoc-sys | 1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/ebkalderon/renderdoc-rs) |
| reqwest | 0.13.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/seanmonstar/reqwest) |
| rustc-hash | 1.1.0 | `Apache-2.0/MIT` | [upstream](https://github.com/rust-lang-nursery/rustc-hash) |
| rustc-hash | 2.1.3 | `Apache-2.0 OR MIT` | [upstream](https://github.com/rust-lang/rustc-hash) |
| rustc_version | 0.4.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/djc/rustc-version-rs) |
| rustix | 0.38.44 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/rustix) |
| rustix | 1.1.5 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/rustix) |
| rustversion | 1.0.23 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/rustversion) |
| same-file | 1.0.6 | `Unlicense/MIT` | [upstream](https://github.com/BurntSushi/same-file) |
| scoped-tls | 1.0.1 | `MIT/Apache-2.0` | [upstream](https://github.com/alexcrichton/scoped-tls) |
| scopeguard | 1.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/bluss/scopeguard) |
| secret-service | 5.2.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/hwchen/secret-service-rs.git) |
| security-framework | 3.7.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/kornelski/rust-security-framework) |
| security-framework-sys | 2.17.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/kornelski/rust-security-framework) |
| self_cell | 1.3.0 | `Apache-2.0 OR GPL-2.0-only` | [upstream](https://github.com/Voultapher/self_cell) |
| semver | 1.0.28 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/semver) |
| serde | 1.0.229 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/serde) |
| serde_core | 1.0.229 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/serde) |
| serde_derive | 1.0.229 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/serde) |
| serde_json | 1.0.154 | `MIT OR Apache-2.0` | [upstream](https://github.com/serde-rs/json) |
| serde_repr | 0.1.21 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/serde-repr) |
| serde_spanned | 1.1.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| sha2 | 0.11.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/RustCrypto/hashes) |
| sharded-slab | 0.1.7 | `MIT` | [upstream](https://github.com/hawkw/sharded-slab) |
| shlex | 2.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/comex/rust-shlex) |
| signal-hook-registry | 1.4.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/vorner/signal-hook) |
| simd-adler32 | 0.3.10 | `MIT` | [upstream](https://github.com/mcountryman/simd-adler32) |
| simd_cesu8 | 1.2.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/seancroach/simd_cesu8) |
| simdutf8 | 0.1.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rusticstuff/simdutf8) |
| siphasher | 1.0.4 | `MIT OR Apache-2.0` | [upstream](https://github.com/jedisct1/rust-siphash) |
| skrifa | 0.44.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/googlefonts/fontations) |
| slab | 0.4.12 | `MIT` | [upstream](https://github.com/tokio-rs/slab) |
| slotmap | 1.1.1 | `Zlib` | [upstream](https://github.com/orlp/slotmap) |
| smallvec | 1.16.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-smallvec) |
| smithay-client-toolkit | 0.19.2 | `MIT` | [upstream](https://github.com/smithay/client-toolkit) |
| smithay-client-toolkit | 0.20.0 | `MIT` | [upstream](https://github.com/smithay/client-toolkit) |
| smithay-clipboard | 0.7.3 | `MIT` | [upstream](https://github.com/smithay/smithay-clipboard) |
| smol_str | 0.2.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-analyzer/smol_str) |
| socket2 | 0.6.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/rust-lang/socket2) |
| stable_deref_trait | 1.2.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/storyyeller/stable_deref_trait) |
| static_assertions | 1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/nvzqz/static-assertions-rs) |
| strsim | 0.11.1 | `MIT` | [upstream](https://github.com/rapidfuzz/strsim-rs) |
| syn | 2.0.119 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/syn) |
| syn | 3.0.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/syn) |
| sync_wrapper | 1.0.2 | `Apache-2.0` | [upstream](https://github.com/Actyx/sync_wrapper) |
| synstructure | 0.14.0 | `MIT` | [upstream](https://github.com/mystor/synstructure) |
| tempfile | 3.27.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/Stebalien/tempfile) |
| thiserror | 1.0.69 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/thiserror) |
| thiserror | 2.0.21 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/thiserror) |
| thiserror-impl | 1.0.69 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/thiserror) |
| thiserror-impl | 2.0.21 | `MIT OR Apache-2.0` | [upstream](https://github.com/dtolnay/thiserror) |
| thread_local | 1.1.10 | `MIT OR Apache-2.0` | [upstream](https://github.com/Amanieu/thread_local-rs) |
| tiff | 0.11.3 | `MIT` | [upstream](https://github.com/image-rs/image-tiff) |
| tinystr | 0.8.4 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| tokio | 1.53.2 | `MIT` | [upstream](https://github.com/tokio-rs/tokio) |
| tokio-macros | 2.7.2 | `MIT` | [upstream](https://github.com/tokio-rs/tokio) |
| toml | 1.1.8+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| toml_datetime | 1.1.2+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
| toml_edit | 0.25.17+spec-1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/toml-rs/toml) |
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
| tray-icon | 0.26.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/tauri-apps/tray-icon) |
| try-lock | 0.2.5 | `MIT` | [upstream](https://github.com/seanmonstar/try-lock) |
| type-map | 0.5.1 | `MIT/Apache-2.0` | [upstream](https://github.com/kardeiz/type-map) |
| typenum | 1.20.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/paholg/typenum) |
| uds_windows | 1.2.1 | `MIT` | [upstream](https://github.com/haraldh/rust_uds_windows) |
| unicode-general-category | 1.1.0 | `Apache-2.0` | [upstream](https://github.com/yeslogic/unicode-general-category) |
| unicode-ident | 1.0.26 | `(MIT OR Apache-2.0) AND Unicode-3.0` | [upstream](https://github.com/dtolnay/unicode-ident) |
| unicode-segmentation | 1.13.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/unicode-rs/unicode-segmentation) |
| unicode-width | 0.2.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/unicode-rs/unicode-width) |
| url | 2.5.8 | `MIT OR Apache-2.0` | [upstream](https://github.com/servo/rust-url) |
| utf8_iter | 1.0.4 | `Apache-2.0 OR MIT` | [upstream](https://github.com/hsivonen/utf8_iter) |
| utf8parse | 0.2.2 | `Apache-2.0 OR MIT` | [upstream](https://github.com/alacritty/vte) |
| uuid | 1.28.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/uuid-rs/uuid) |
| valuable | 0.1.1 | `MIT` | [upstream](https://github.com/tokio-rs/valuable) |
| vello_common | 0.1.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/vello) |
| vello_cpu | 0.1.0 | `Apache-2.0 OR MIT` | [upstream](https://github.com/linebender/vello) |
| version_check | 0.9.5 | `MIT/Apache-2.0` | [upstream](https://github.com/SergioBenitez/version_check) |
| walkdir | 2.5.0 | `Unlicense/MIT` | [upstream](https://github.com/BurntSushi/walkdir) |
| want | 0.3.2 | `MIT` | [upstream](https://github.com/seanmonstar/want) |
| wasi | 0.11.1+wasi-snapshot-preview1 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/wasi) |
| wasip2 | 1.0.4+wasi-0.2.12 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/wasi-rs) |
| wasm-bindgen | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen) |
| wasm-bindgen-futures | 0.4.79 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures) |
| wasm-bindgen-macro | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) |
| wasm-bindgen-macro-support | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support) |
| wasm-bindgen-shared | 0.2.129 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) |
| wayland-backend | 0.3.17 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-client | 0.31.15 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-csd-frame | 0.3.0 | `MIT` | [upstream](https://github.com/rust-windowing/wayland-csd-frame) |
| wayland-cursor | 0.31.14 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-protocols | 0.32.13 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-protocols-experimental | 20250721.0.1 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-protocols-misc | 0.3.12 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-protocols-plasma | 0.3.12 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-protocols-wlr | 0.3.12 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-scanner | 0.31.11 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| wayland-sys | 0.31.11 | `MIT` | [upstream](https://github.com/smithay/wayland-rs) |
| web-sys | 0.3.106 | `MIT OR Apache-2.0` | [upstream](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys) |
| web-time | 1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/daxpedda/web-time) |
| weezl | 0.1.12 | `MIT OR Apache-2.0` | [upstream](https://github.com/image-rs/weezl) |
| wgpu | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| wgpu-core | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| wgpu-core-deps-windows-linux-android | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| wgpu-hal | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| wgpu-naga-bridge | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| wgpu-types | 30.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/gfx-rs/wgpu) |
| winapi-util | 0.1.11 | `Unlicense OR MIT` | [upstream](https://github.com/BurntSushi/winapi-util) |
| windows | 0.62.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-collections | 0.3.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-core | 0.62.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-future | 0.3.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-implement | 0.60.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-interface | 0.59.3 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-link | 0.2.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-native-keyring-store | 1.1.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/open-source-cooperative/windows-native-keyring-store.git) |
| windows-numerics | 0.3.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-result | 0.4.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-strings | 0.5.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.52.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.59.0 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.60.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.61.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-targets | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-targets | 0.53.5 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows-threading | 0.2.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_aarch64_gnullvm | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_aarch64_gnullvm | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_aarch64_msvc | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_aarch64_msvc | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_i686_gnu | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_i686_gnu | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_i686_gnullvm | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_i686_gnullvm | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_i686_msvc | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_i686_msvc | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnu | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnu | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnullvm | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnullvm | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_x86_64_msvc | 0.52.6 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| windows_x86_64_msvc | 0.53.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/microsoft/windows-rs) |
| winit | 0.30.13 | `Apache-2.0` | [upstream](https://github.com/rust-windowing/winit) |
| winnow | 1.0.4 | `MIT` | [upstream](https://github.com/winnow-rs/winnow) |
| wit-bindgen | 0.57.1 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [upstream](https://github.com/bytecodealliance/wit-bindgen) |
| writeable | 0.6.4 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| x11-dl | 2.21.0 | `MIT` | [upstream](https://github.com/AltF02/x11-rs.git) |
| x11rb | 0.13.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/psychon/x11rb) |
| x11rb-protocol | 0.13.2 | `MIT OR Apache-2.0` | [upstream](https://github.com/psychon/x11rb) |
| xcursor | 0.3.11 | `MIT` | [upstream](https://github.com/esposm03/xcursor-rs) |
| xkbcommon-dl | 0.4.2 | `MIT` | [upstream](https://github.com/rust-windowing/xkbcommon-dl) |
| xkeysym | 0.2.1 | `MIT OR Apache-2.0 OR Zlib` | [upstream](https://github.com/notgull/xkeysym) |
| xml-rs | 0.8.29 | `MIT` | [upstream](https://github.com/kornelski/xml-rs) |
| yoke | 0.8.3 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| yoke-derive | 0.8.4 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zbus | 5.19.0 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |
| zbus-lockstep | 0.5.2 | `MIT` | [upstream](https://github.com/luukvanderduim/zbus-lockstep) |
| zbus-lockstep-macros | 0.5.2 | `MIT` | [upstream](https://github.com/luukvanderduim/zbus-lockstep) |
| zbus-secret-service-keyring-store | 1.0.1 | `MIT OR Apache-2.0` | [upstream](https://github.com/open-source-cooperative/zbus-secret-service-keyring-store.git) |
| zbus_macros | 5.19.0 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |
| zbus_names | 4.3.4 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |
| zbus_xml | 5.2.1 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |
| zcheapstr | 1.1.0 | `MIT` | [upstream](https://github.com/z-galaxy/zcheapstr/) |
| zerocopy | 0.8.62 | `BSD-2-Clause OR Apache-2.0 OR MIT` | [upstream](https://github.com/google/zerocopy) |
| zerocopy-derive | 0.8.62 | `BSD-2-Clause OR Apache-2.0 OR MIT` | [upstream](https://github.com/google/zerocopy) |
| zerofrom | 0.1.8 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerofrom-derive | 0.1.8 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zeroize | 1.9.1 | `Apache-2.0 OR MIT` | [upstream](https://github.com/RustCrypto/utils) |
| zerotrie | 0.2.5 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerovec | 0.11.8 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zerovec-derive | 0.11.6 | `Unicode-3.0` | [upstream](https://github.com/unicode-org/icu4x) |
| zlib-rs | 0.6.8 | `Zlib` | [upstream](https://github.com/trifectatechfoundation/zlib-rs) |
| zmij | 1.0.23 | `MIT` | [upstream](https://github.com/dtolnay/zmij) |
| zune-core | 0.5.3 | `MIT OR Apache-2.0 OR Zlib` | [upstream](https://github.com/etemesi254/zune-image) |
| zune-jpeg | 0.5.15 | `MIT OR Apache-2.0 OR Zlib` | [upstream](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) |
| zvariant | 5.15.0 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |
| zvariant_derive | 5.15.0 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |
| zvariant_utils | 4.2.0 | `MIT` | [upstream](https://github.com/z-galaxy/zbus/) |

The release source archive preserves complete Cargo-vendored package payloads, including manifests, source headers and embedded notices. Binary archives retain collected upstream license, copyright and notice files, including nested files. A package-level expression alone does not enumerate all embedded third-party notices.

Some published GUI/transitive crates omit their standalone upstream licenses. [`third-party/notices/origins.json`](third-party/notices/origins.json) records the exact published `.cargo_vcs_info.json` commit, primary upstream URLs and SHA-256 hashes for retained notice supplements. `scripts/supplement-notices.py` recreates these files from those commits. Binary packages copy the validated supplements into each affected crate's `upstream-supplement/` directory; source packages retain the original crate payloads unchanged plus the supplements. This includes the egui, AccessKit, objc2, JNI/NDK and generated graphics dependency families where the published crate omitted the root license. Khronos API XML payloads are also retained in binary notices to preserve embedded API copyright and licensing text.

`dispatch` 0.2.0 declares MIT and author Steven Sheldon in its published Cargo manifest, but supplies no standalone license or copyright notice in the crate or its recorded upstream commit `82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748`. Its original manifest and README are retained in the notice supplement and corresponding source; no copyright year or authorship statement is invented. See the [exact upstream source](https://github.com/SSheldon/rust-dispatch/tree/82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748).

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

## Desktop companion artwork and fonts

The keyboard icon and its generator in `assets/` are independently written GPL-3.0-only project artwork and contain no official manufacturer logo. GUI dependencies retain their own terms, including eframe/egui, platform tray libraries and OS credential-store adapters.

The companion uses the bundled fonts supplied by `epaint_default_fonts`. Font licenses and copyright notices, including nested font notice files, are retained in the vendored corresponding source and `third-party/rust-crates/epaint_default_fonts-*/` in binary packages. Font terms remain their original terms; the project's GPL does not relicense them. The packaging collector includes every Markdown/text font notice as well as license-named files. Do not replace bundled font files without updating their notices.

## Windows installer runtime

Windows installers are built with the official portable **NSIS 3.13** compiler. `scripts/bootstrap-nsis.py` pins the official ZIP's SHA-256 and checks it before executing the compiler. The installer selects NSIS's Zlib compression explicitly. Its installer/uninstaller stubs and compression runtime are covered by the notices preserved verbatim in [`installers/nsis-notices/COPYING`](installers/nsis-notices/COPYING), which is also installed as `third-party/nsis/COPYING`. This includes the applicable NSIS and Zlib notices and their supplied third-party notices; it does not claim the project authored the installer runtime. The general-purpose compiler is not bundled in project release packages. Source and documentation: [NSIS official project](https://nsis.sourceforge.io/) and [official 3.13 distribution](https://sourceforge.net/projects/nsis/files/NSIS%203/3.13/).

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

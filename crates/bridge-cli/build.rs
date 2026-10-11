use std::{env, path::PathBuf, process::Command};
fn main() {
    let workspace = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let source = workspace.join("c64");
    println!("cargo:rerun-if-changed={}", source.join("src").display());
    println!(
        "cargo:rerun-if-changed={}",
        source.join("include").display()
    );
    println!("cargo:rerun-if-env-changed=C64_PRG_DIR");
    println!("cargo:rerun-if-env-changed=CL65");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    if let Some(prebuilt) = env::var_os("C64_PRG_DIR") {
        for name in ["KEYBRIDGE.PRG", "NETWORKTEST.PRG"] {
            std::fs::copy(PathBuf::from(&prebuilt).join(name), out.join(name)).expect(
                "C64_PRG_DIR must contain real KEYBRIDGE.PRG and NETWORKTEST.PRG templates",
            );
        }
    } else {
        let compiler = env::var_os("CL65").unwrap_or_else(|| "cl65".into());
        for (name, probe) in [("KEYBRIDGE.PRG", false), ("NETWORKTEST.PRG", true)] {
            let variant = out.join(if probe { "probe" } else { "keyboard" });
            std::fs::create_dir_all(&variant).expect("Cannot create C64 build directory");
            for file in ["main.c", "bridge.c", "uci.c", "scanner.s"] {
                std::fs::copy(source.join("src").join(file), variant.join(file))
                    .expect("Cannot copy C64 build source");
            }
            let mut command = Command::new(&compiler);
            command
                .current_dir(&variant)
                .args(["-t", "c64", "--cpu", "6502", "-Oirs", "--standard", "c99"])
                .arg("-I")
                .arg(source.join("include"))
                .arg("-o")
                .arg(out.join(name));
            if probe {
                command.arg("-DNETWORK_PROBE");
            }
            command.args(["main.c", "bridge.c", "uci.c", "scanner.s"]);
            let status=command.status().expect("cc65 is required: macOS brew install cc65; Linux install cc65; Windows use cc65 in PATH. Alternatively set C64_PRG_DIR to genuine prebuilt templates.");
            assert!(
                status.success(),
                "C64 compilation failed; run make -C c64 all for diagnostics"
            );
        }
    }
    for name in ["KEYBRIDGE.PRG", "NETWORKTEST.PRG"] {
        let data = std::fs::read(out.join(name)).unwrap();
        assert!(
            data.len() > 66
                && data[..2] == [1, 8]
                && data
                    .windows(8)
                    .filter(|bytes| *bytes == b"KBCFGv1\0")
                    .count()
                    == 1,
            "Invalid generated PRG template"
        );
        let offset = data
            .windows(8)
            .position(|bytes| bytes == b"KBCFGv1\0")
            .unwrap();
        assert!(
            offset + 64 <= data.len(),
            "Truncated PRG configuration block"
        );
        let config = &data[offset..offset + 64];
        assert!(
            config[8] == 1 && config[9] == 64,
            "Unsupported PRG configuration version or size"
        );
        assert!(
            config[10..44].iter().all(|&byte| byte == 0),
            "Configured PRGs cannot be embedded: use unpatched templates without addresses or pairing tokens"
        );
        assert!(
            (1..=19).contains(&config[44]),
            "Invalid PRG identity length"
        );
    }
}

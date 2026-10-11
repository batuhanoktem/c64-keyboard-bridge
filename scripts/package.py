#!/usr/bin/env python3
"""Create verified binary and matching vendored source archives without private configuration."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import runpy
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
PRGS = runpy.run_path(str(ROOT / "scripts/verify-prgs.py"))
REVISION = PRGS["CC65_REVISION"]
SUPPORTED_TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "aarch64-unknown-linux-gnu",
                     "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc")
ROOT_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "README.md", "LICENSE",
              "THIRD_PARTY_NOTICES.md", ".env.example", ".gitignore")
SOURCE_FOLDERS = {
    "crates": {".rs", ".toml", ".md"},
    "c64": {".c", ".h", ".s", ".md", ".py", ".sh", ".cfg", ".txt"},
    "docs": {".md"},
    "config/layouts": {".toml"},
    "scripts": {".sh", ".py", ".md"},
    ".github": {".yml", ".yaml", ".md"},
}


def run(*arguments, **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run([str(a) for a in arguments], cwd=ROOT, check=True, **kwargs)


def hash_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_target(host: str, target: str) -> None:
    if host not in SUPPORTED_TARGETS or target not in SUPPORTED_TARGETS:
        raise ValueError("Release packaging supports only the five documented native target triples")
    if target == host:
        return
    mac_targets = {"aarch64-apple-darwin", "x86_64-apple-darwin"}
    if platform.system() != "Darwin" or {host, target} != mac_targets:
        raise ValueError("Cross packaging supports only the alternate macOS architecture with an installed Rust target and native SDK")
    target_libraries = Path(run("rustc", "--print", "target-libdir", "--target", target,
                                capture_output=True, text=True).stdout.strip())
    if not target_libraries.is_dir() or not any(target_libraries.glob("libstd-*")):
        raise ValueError(f"Install the pinned Rust target first: rustup target add {target} --toolchain 1.99.0")
    sdk = Path(run("xcrun", "--sdk", "macosx", "--show-sdk-path", capture_output=True, text=True).stdout.strip())
    if not sdk.is_dir():
        raise ValueError("Cross packaging requires an installed native macOS SDK")


def configure_macos_deployment(target: str, environment: dict) -> str | None:
    floors = {"aarch64-apple-darwin": "11.0", "x86_64-apple-darwin": "10.15"}
    floor = floors.get(target)
    if floor is None:
        return None
    configured = environment.get("MACOSX_DEPLOYMENT_TARGET")
    if configured:
        if re.fullmatch(r"[0-9]+(?:\.[0-9]+){0,2}", configured) is None:
            raise ValueError("MACOSX_DEPLOYMENT_TARGET must be a numeric macOS version")

        def version(value: str) -> tuple[int, int, int]:
            parts = [int(part) for part in value.split(".")]
            return tuple(parts + [0] * (3 - len(parts)))

        if version(configured) > version(floor):
            floor = configured
    environment["MACOSX_DEPLOYMENT_TARGET"] = floor
    return floor


def source_files() -> list[Path]:
    result = [ROOT / name for name in ROOT_FILES]
    if (ROOT / ".gitattributes").is_file():
        result.append(ROOT / ".gitattributes")
    result.append(ROOT / "c64/Makefile")
    for folder, extensions in SOURCE_FOLDERS.items():
        for path in (ROOT / folder).rglob("*"):
            relative = path.relative_to(ROOT)
            if any(part in {"build", "target", ".git", "__pycache__", "node_modules"} for part in relative.parts):
                continue
            if path.is_file() and not path.is_symlink() and path.suffix.lower() in extensions:
                result.append(path)
    return sorted(set(result))


def project_hash(files: list[Path]) -> str:
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.relative_to(ROOT).as_posix().encode() + b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def copy_file(path: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(path, destination)


def revision_info() -> dict:
    try:
        revision = run("git", "rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
        dirty = run("git", "status", "--porcelain", capture_output=True, text=True).stdout != ""
    except subprocess.CalledProcessError:
        revision, dirty = "uncommitted", True
    return {"project_revision": revision, "working_tree_modified": dirty}


def prepare_prgs() -> tuple[Path, dict]:
    if platform.system() == "Windows":
        directory = os.environ.get("C64_PRG_DIR")
        if not directory:
            raise ValueError("Windows packaging requires C64_PRG_DIR from the pinned C64 build job")
        path = Path(directory).resolve()
    else:
        run("bash", ROOT / "scripts/build-prgs.sh")
        path = ROOT / "target/c64-prgs"
    return path, PRGS["verify"](path)


def vendor(destination: Path) -> None:
    result = run("cargo", "vendor", "--locked", "--versioned-dirs", destination,
                 stdout=subprocess.PIPE, text=True)
    lines = result.stdout.splitlines()
    if not any(line.startswith("[source.") for line in lines):
        raise ValueError("cargo vendor did not produce the expected source configuration")
    config = "\n".join('directory = "vendor"' if line.lstrip().startswith("directory = ") else line
                       for line in lines) + "\n"
    cargo_config = destination.parent / ".cargo/config.toml"
    cargo_config.parent.mkdir(parents=True, exist_ok=True)
    cargo_config.write_text(config)


def collect_crate_notices(vendored: Path, destination: Path) -> None:
    inventory = []
    for package in sorted(vendored.iterdir()):
        manifest = package / "Cargo.toml"
        if not package.is_dir() or not manifest.is_file():
            continue
        metadata = tomllib.loads(manifest.read_text())["package"]
        name, version = metadata["name"], metadata["version"]
        inventory.append({"name": name, "version": version, "license": metadata.get("license"),
                          "license_file": metadata.get("license-file"), "repository": metadata.get("repository")})
        for path in package.rglob("*"):
            if not path.is_file():
                continue
            lower = path.name.lower()
            is_notice = lower.startswith(("license", "licence", "copying", "notice", "copyright", "authors"))
            supplement = name in {"r-efi", "valuable"} and lower == "readme.md"
            if is_notice or supplement or path == manifest:
                copy_file(path, destination / f"{name}-{version}" / path.relative_to(package))
        license_file = metadata.get("license-file")
        if license_file:
            path = (package / license_file).resolve()
            if not path.is_relative_to(package.resolve()):
                raise ValueError(f"Escaping license-file metadata in {name}")
            copy_file(path, destination / f"{name}-{version}" / license_file)
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")


def collect_runtime_notices(destination: Path) -> None:
    sysroot = Path(run("rustc", "--print", "sysroot", capture_output=True, text=True).stdout.strip())
    docs = sysroot / "share/doc/rust"
    if not (docs / "COPYRIGHT-library.html").is_file() or not (docs / "licenses").is_dir():
        raise ValueError("Rust runtime notices are missing; install `rustup component add rust-docs --toolchain 1.99.0`")
    copy_file(docs / "COPYRIGHT-library.html", destination / "COPYRIGHT-library.html")
    shutil.copytree(docs / "licenses", destination / "licenses")


def archive_cc65(destination: Path) -> None:
    if (ROOT / "third-party/cc65").is_dir():
        existing = ROOT / "third-party/cc65"
        run("python" if platform.system() == "Windows" else sys.executable,
            ROOT / "scripts/verify-cc65-source.py", existing)
        manifest = json.loads((existing / ".c64bridge-source-sha256.json").read_text())
        for name in manifest["files"]:
            copy_file(existing / name, destination / name)
    else:
        source = Path(run("bash", ROOT / "scripts/bootstrap-cc65.sh", capture_output=True, text=True).stdout.strip())
        data = run("git", "-C", source, "archive", REVISION, stdout=subprocess.PIPE).stdout
        destination.mkdir(parents=True, exist_ok=True)
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as archive:
            for member in archive.getmembers():
                path = destination / member.name
                if not path.resolve().is_relative_to(destination.resolve()) or member.issym() or member.islnk():
                    raise ValueError("cc65 archive contains an unsupported link or escaping path")
                if member.isdir():
                    path.mkdir(parents=True, exist_ok=True)
                elif member.isfile():
                    path.parent.mkdir(parents=True, exist_ok=True)
                    stream = archive.extractfile(member)
                    if stream is None:
                        raise ValueError("Cannot read cc65 source archive")
                    path.write_bytes(stream.read())
                    path.chmod(member.mode & 0o777)
    hashes = {p.relative_to(destination).as_posix(): hash_file(p)
              for p in sorted(destination.rglob("*")) if p.is_file()}
    (destination / ".c64bridge-source-revision").write_text(REVISION + "\n")
    (destination / ".c64bridge-source-sha256.json").write_text(
        json.dumps({"revision": REVISION, "files": hashes}, indent=2) + "\n")


def write_archive(directory: Path, path: Path, windows: bool = False) -> None:
    if windows:
        with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for file in sorted(directory.rglob("*")):
                if file.is_file():
                    archive.write(file, Path(directory.name) / file.relative_to(directory))
    else:
        with tarfile.open(path, "w:gz") as archive:
            archive.add(directory, arcname=directory.name)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--binary-only", action="store_true")
    scope.add_argument("--source-only", action="store_true")
    parser.add_argument("--target", choices=SUPPORTED_TARGETS, help="Binary target (default: native Rust host)")
    parser.add_argument("--output", type=Path, default=ROOT / "target/dist")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    rustc = run("rustc", "-vV", capture_output=True, text=True).stdout
    if not rustc.startswith("rustc 1.99.0 "):
        raise ValueError("Release packaging requires the pinned Rust 1.99.0 toolchain")
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    target = args.target or host
    validate_target(host, target)
    files = source_files()
    before = project_hash(files)

    def require_stable_sources() -> None:
        if source_files() != files or project_hash(files) != before:
            raise ValueError("Project sources changed during packaging; rerun from a stable checkout")

    prg_directory, prg_manifest = prepare_prgs()
    environment = os.environ.copy()
    environment["C64_PRG_DIR"] = str(prg_directory)
    environment["CARGO_TARGET_DIR"] = str(ROOT / "target")
    macos_minimum = configure_macos_deployment(target, environment)
    if not args.source_only:
        run("cargo", "build", "--release", "--locked", "--bin", "c64bridge", "--target", target, env=environment)
    require_stable_sources()
    provenance = {"schema": 1, "version": version, **revision_info(), "project_source_sha256": before,
                  "rustc": rustc.strip(), "build_host_target": host, "binary_target": target, "prgs": prg_manifest,
                  "minimum_os": f"macOS {macos_minimum}" if macos_minimum else None,
                  "cc65_build_id": "Git 55528249",
                  "verification": "compiled; physical C64 and native input delivery are not asserted"}
    (ROOT / "target/package-staging").mkdir(parents=True, exist_ok=True)
    archives = []
    with tempfile.TemporaryDirectory(prefix="release-", dir=ROOT / "target/package-staging") as temporary:
        stage = Path(temporary)
        source = stage / f"c64-keyboard-bridge-{version}-source"
        source.mkdir()
        vendor(source / "vendor")
        if not args.binary_only:
            for path in files:
                copy_file(path, source / path.relative_to(ROOT))
            archive_cc65(source / "third-party/cc65")
            (source / "build-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
            (source / "SOURCE_BUILD.md").write_text(
                "# Building this corresponding source\n\n"
                "Install Rust 1.99.0, Python 3.11 or newer, Git, GNU Make and a native C compiler. "
                "The archive contains all locked Cargo sources in `vendor/`, `.cargo/config.toml`, and the exact cc65 V2.19 source in `third-party/cc65/`. "
                "The tools themselves and operating system SDKs are not bundled.\n\n"
                "On macOS or Linux run `scripts/check.sh` to compile cc65, both unconfigured PRGs, and the host, "
                "then run Rust/C tests and recording CLI smoke. Cargo uses the included vendored sources; "
                "an already installed Rust toolchain is required for an offline build.\n\n"
                "On Windows use the matching release archive's verified unconfigured PRGs as `C64_PRG_DIR` "
                "and run `bash scripts/check.sh --host-only`. The C64 source can also be rebuilt using cc65, "
                "GNU Make and a native C compiler under MSYS2.\n\n"
                "Use `scripts/package.sh` to rebuild release archives. See README.md, c64/README.md and "
                "THIRD_PARTY_NOTICES.md for configuration, provenance and licensing. Native input and "
                "physical hardware acceptance tests require explicit user actions.\n")
            path = output / f"{source.name}.tar.gz"
            require_stable_sources()
            write_archive(source, path)
            archives.append(path)
        if not args.source_only:
            binary = stage / f"c64-keyboard-bridge-{version}-{target}"
            binary.mkdir()
            suffix = ".exe" if target == "x86_64-pc-windows-msvc" else ""
            copy_file(ROOT / "target" / target / "release" / ("c64bridge" + suffix), binary / ("c64bridge" + suffix))
            for name in ("README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", ".env.example"):
                copy_file(ROOT / name, binary / name)
            for path in files:
                relative = path.relative_to(ROOT)
                if relative.parts[0] == "docs" or relative.parts[:2] == ("config", "layouts"):
                    copy_file(path, binary / relative)
            copy_file(ROOT / "c64/README.md", binary / "c64/README.md")
            for name in (*PRGS["NAMES"], "prg-manifest.json"):
                copy_file(prg_directory / name, binary / "c64" / name)
            collect_crate_notices(source / "vendor", binary / "third-party/rust-crates")
            collect_runtime_notices(binary / "third-party/rust-runtime")
            (binary / "build-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
            extension = ".zip" if suffix else ".tar.gz"
            path = output / (binary.name + extension)
            require_stable_sources()
            write_archive(binary, path, windows=bool(suffix))
            archives.append(path)
    checksums = output / ("SHA256SUMS-source" if args.source_only else f"SHA256SUMS-{target}")
    checksums.write_text("".join(f"{hash_file(path)}  {path.name}\n" for path in archives))
    for path in archives:
        print(path)
    print(checksums)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, StopIteration, subprocess.SubprocessError) as error:
        sys.exit(str(error))

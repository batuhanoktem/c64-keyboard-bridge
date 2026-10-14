"""Reviewed release promotion services; no package code is executed and no secrets are logged."""
from __future__ import annotations

import base64
from contextlib import contextmanager
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import posixpath
import re
import secrets
import shutil
import signal
import subprocess
import tarfile
import tempfile
import tomllib
import uuid
from urllib.parse import quote, unquote, urlsplit
import zipfile
import zlib

ROOT = Path(__file__).resolve().parent.parent
APP = "C64 Keyboard Bridge.app"
APP_ID = "com.flameliongames.c64keyboardbridge"
IDENTITY = "Developer ID Application: Flame Lion Ltd (TUGY329T35)"
TEAM = "TUGY329T35"
FINGERPRINT = "C27E877E5A439A74D83376ECA9CD3669EF88F945"
BINARIES = ("c64-keyboard-bridge", "c64-keyboard-bridge-gui")
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "aarch64-unknown-linux-gnu",
           "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc")
MAX_BYTES = 4 * 1024**3
MAX_FILES = 150000
ARTIFACT_EXTENSIONS = (".tar.gz", ".zip", ".dmg", ".exe", ".deb", ".metadata.json")
# Match the packaging allowlist, including the narrowly scoped screenshot addition.
ROOT_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "README.md", "LICENSE",
              "THIRD_PARTY_NOTICES.md", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md",
              ".env.example", ".gitignore", ".gitattributes", "c64/Makefile"}
FOLDERS = {"crates": {".rs", ".toml", ".md"}, "c64": {".c", ".h", ".s", ".md", ".py", ".sh", ".cfg", ".txt"},
           "docs": {".md"}, "config/layouts": {".toml"}, "scripts": {".sh", ".py", ".md"},
           ".github": {".yml", ".yaml", ".md"}, "assets": {".svg", ".png", ".ico", ".icns", ".py", ".md"},
           "installers": {".nsi", ".desktop", ".plist", ".md"}, "build-support": {".rs", ".md"},
           "third-party/notices": {".json", ".txt", ".md", ".orig"}}


class ReleaseError(Exception):
    """A fixed, credential-free diagnostic safe to display publicly."""


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


TOOL_OPERATIONS = {
    "git": {("rev-parse",): "resolve", ("merge-base",): "ancestry", ("ls-tree",): "inventory", ("show",): "read-blob"},
    "gh": {("api",): "api", ("run", "download"): "download-ci", ("release", "create"): "create-draft",
           ("release", "edit"): "edit-release", ("release", "upload"): "upload-assets",
           ("release", "download"): "download-release", ("release", "delete-asset"): "remove-draft-asset"},
    "security": {("find-identity",): "find-identity", ("delete-keychain",): "delete-keychain",
                 ("list-keychains",): "search-list"},
    "codesign": {("--force",): "sign", ("--verify",): "verify", ("--display",): "inspect"},
    "ditto": {("-c",): "archive"},
    "hdiutil": {("create",): "create-dmg"},
    "spctl": {("--assess",): "assess"},
    "xcrun": {("notarytool", "submit"): "submit-notarization", ("stapler", "staple"): "staple",
              ("stapler", "validate"): "validate-staple"},
    "lipo": {("-verify_arch",): "verify-architecture"},
}
KEYCHAIN_OPERATIONS = {"create-keychain", "set-keychain-settings", "unlock-keychain", "import", "set-key-partition-list",
                      "list-keychains"}
SIGNING_ROLES = {"portable-cli", "portable-gui", "app-bundle", "dmg"}
# Known Security/codesign literals only. Specific chain evidence precedes the
# generic errSecInternalComponent wrapper described by Apple DTS (thread 712005).
# Resource-fork wording is documented in Apple's QA1940.
CODESIGN_FAILURES = (
    ("chain-unavailable", (b"unable to build chain to self-signed root", b"errSecCreateChainFailed",
                           b"One or more certificates required to validate this certificate cannot be found.")),
    ("interaction-not-allowed", (b"User interaction is not allowed", b"errSecInteractionNotAllowed")),
    ("identity-unavailable", (b"no identity found", b"errSecItemNotFound",
                              b"The specified item could not be found in the keychain.")),
    ("keychain-unavailable", (b"The specified keychain could not be found.", b"errSecNoSuchKeychain")),
    ("keychain-invalid", (b"The specified keychain is not a valid keychain file.", b"errSecInvalidKeychain")),
    ("private-key-unavailable", (b"CSSMERR_CSP_INVALID_KEY_REFERENCE", b"errSecInvalidKeyRef")),
    ("timestamp-unavailable", (b"The timestamp service is not available", b"A timestamp was expected but was not found")),
    ("authentication-failed", (b"The user name or passphrase you entered is not correct.", b"errSecAuthFailed")),
    ("permission-denied", (b"Permission denied", b"Operation not permitted")),
    ("resource-fork", (b"resource fork, Finder information, or similar detritus not allowed", b"errSecCSInvalidAssociatedFileData")),
    ("internal-component", (b"errSecInternalComponent",)),
)


def tool_diagnostic(arguments, operation=None, exit_code=None, role=None, stderr=None):
    """Only fixed diagnostic labels and an integer status can reach a diagnostic."""
    tool = Path(str(arguments[0])).name if arguments else ""
    if tool not in TOOL_OPERATIONS:
        return ""
    if not (tool == "security" and isinstance(operation, str) and operation in KEYCHAIN_OPERATIONS):
        operation = next((label for prefix, label in TOOL_OPERATIONS[tool].items()
                          if tuple(arguments[1:1 + len(prefix)]) == prefix), None)
        if tool == "lipo" and len(arguments) > 2 and arguments[2] == "-verify_arch":
            operation = "verify-architecture"
    fields = ["tool=" + tool]
    if operation is not None:
        fields.append("operation=" + operation)
    if type(exit_code) is int:
        fields.append("exit_code=" + str(exit_code))
    if tool == "codesign":
        if isinstance(role, str) and role in SIGNING_ROLES:
            fields.append("role=" + role)
        if isinstance(stderr, bytes):
            reason = next((reason for reason, markers in CODESIGN_FAILURES
                           if any(marker in stderr for marker in markers)), None)
            if reason is not None:
                fields.append("reason=" + reason)
    return " (" + ", ".join(fields) + ")"


def run(*arguments, input=None, timeout=300, cwd=None, env=None, allow_nonzero=False,
        diagnostic_operation=None, diagnostic_role=None):
    """Never expose subprocess arguments, output, raw exceptions or credentials."""
    try:
        child_env = dict(os.environ if env is None else env)
        for name in ("MACOS_CERTIFICATE_P12_BASE64", "MACOS_CERTIFICATE_PASSWORD", "APPLE_NOTARY_KEY_P8_BASE64"):
            child_env.pop(name, None)
        if Path(str(arguments[0])).name != "gh":
            for name in ("GH_TOKEN", "GITHUB_TOKEN"):
                child_env.pop(name, None)
        result = subprocess.run([str(arg) for arg in arguments], input=input, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, check=False, timeout=timeout, cwd=cwd,
                                env=child_env)
    except (OSError, subprocess.SubprocessError):
        raise ReleaseError("Release tool could not complete; raw output was suppressed"
                           + tool_diagnostic(arguments, diagnostic_operation, role=diagnostic_role)) from None
    require(allow_nonzero or result.returncode == 0, "Release tool failed; raw output was suppressed"
            + tool_diagnostic(arguments, diagnostic_operation, result.returncode, diagnostic_role, result.stderr))
    return result


def json_output(result):
    try:
        return json.loads(result.stdout)
    except (ValueError, TypeError):
        raise ReleaseError("Release tool returned an invalid response; raw output was suppressed") from None


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def safe_name(value):
    require(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._ -]*", value) is not None,
            "Release asset has an unsafe name")
    return value


def archive_members(archive, root):
    members, names, total = [], set(), 0
    for member in archive:
        path = PurePosixPath(member.name)
        require(not path.is_absolute() and path.parts and path.parts[0] == root
                and ".." not in path.parts and "\\" not in member.name
                and all(ord(char) >= 32 and ord(char) < 127 for char in member.name),
                "Release archive contains an unsafe path")
        require(path.as_posix() not in names and (member.isdir() or member.isfile()),
                "Release archive contains duplicate files, links or special entries")
        require(not member.mode & 0o7000, "Release archive contains privileged file modes")
        require(not any(part in {".git", ".env"} or part.endswith((".p12", ".p8", ".key", ".pem"))
                        for part in path.parts) and not member.name.endswith(
                            ("/docs/secure-releases.md", "/docs/store-distribution.md")),
                "Release archive contains private configuration")
        require(member.size >= 0, "Release archive contains an invalid member size")
        total += member.size
        require(total <= MAX_BYTES and member.size <= 512 * 1024**2 and len(members) < MAX_FILES,
                "Release archive exceeds its resource limits")
        names.add(path.as_posix())
        members.append(member)
    return members


@contextmanager
def bounded_tar(path):
    """Decompress once to a private regular file; validation never seeks through gzip."""
    try:
        with tempfile.TemporaryFile(prefix="release-tar-", mode="w+b") as staged:
            with gzip.open(path, "rb") as compressed:
                total = 0
                while block := compressed.read(min(1024 * 1024, MAX_BYTES - total + 1)):
                    total += len(block)
                    require(total <= MAX_BYTES, "Release archive exceeds its decompression limit")
                    staged.write(block)
            staged.seek(0)
            with tarfile.open(fileobj=staged, mode="r:") as archive:
                yield archive
    except (OSError, EOFError, tarfile.TarError, zlib.error):
        raise ReleaseError("Release archive is malformed or unreadable") from None


def read_member(archive, name, maximum=16 * 1024**2):
    try:
        member = archive.getmember(name)
        require(member.isfile() and member.size <= maximum, "Release metadata is absent or oversized")
        with archive.extractfile(member) as stream:
            return stream.read(maximum + 1)
    except (KeyError, AttributeError, OSError):
        raise ReleaseError("Release metadata is absent or unreadable") from None


def archive_json(archive, name):
    try:
        value = json.loads(read_member(archive, name))
        require(isinstance(value, dict), "Release metadata must be an object")
        return value
    except (ValueError, UnicodeError):
        raise ReleaseError("Release metadata is malformed") from None


def extract_archive(path, destination, root):
    require(not destination.exists(), "Release extraction destination must be new")
    destination.mkdir(mode=0o700, parents=True)
    with tarfile.open(path, "r:gz") as archive:
        members = archive_members(archive, root)
        for member in members:
            target = destination.joinpath(*PurePosixPath(member.name).parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)
                target.chmod(member.mode & 0o777)
    return destination / root


def checksum_manifest(directory, name="SHA256SUMS"):
    directory = Path(directory)
    assets = sorted(p for p in directory.iterdir() if p.is_file() and p.name.endswith(ARTIFACT_EXTENSIONS))
    require(bool(assets), "Release has no assets")
    for asset in assets:
        safe_name(asset.name)
        require(not asset.is_symlink(), "Release assets may not be links")
    (directory / name).write_text("".join(f"{sha256(path)}  {path.name}\n" for path in assets))
    return {path.name: sha256(path) for path in assets}


def verify_manifest(directory, name="SHA256SUMS", exact=True):
    directory = Path(directory)
    manifest = directory / name
    require(manifest.is_file() and not manifest.is_symlink() and manifest.stat().st_size < 100000,
            "Release checksum manifest is absent or oversized")
    entries = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([a-f0-9]{64})  ([A-Za-z0-9][A-Za-z0-9._ -]*)", line)
        require(match is not None, "Release checksum manifest is malformed")
        digest, filename = match.groups()
        require(filename not in entries, "Release checksum manifest repeats an asset")
        path = directory / filename
        require(path.is_file() and not path.is_symlink() and sha256(path) == digest,
                "Release asset checksum mismatch")
        entries[filename] = digest
    require(bool(entries), "Release checksum manifest is empty")
    if exact:
        assets = {path.name for path in directory.iterdir()
                  if path.is_file() and path.name.endswith(ARTIFACT_EXTENSIONS)}
        require(assets == set(entries), "Release asset inventory and checksums disagree")
    return entries


def fetch_artifacts(github, run_id, destination):
    destination = Path(destination)
    require(not destination.exists(), "Artifact download directory must be new")
    with tempfile.TemporaryDirectory(prefix="release-fetch-") as temporary:
        stage = Path(temporary)
        run("gh", "run", "download", str(run_id), "--repo", github.repository,
            "--pattern", "release-*", "--dir", stage, timeout=1200)
        expected = {"release-" + target for target in TARGETS} | {"release-source"}
        require({path.name for path in stage.iterdir()} == expected,
                "CI artifact wrappers do not cover exactly five platforms and source")
        seen = set()
        for folder in sorted(stage.iterdir()):
            require(folder.is_dir() and not folder.is_symlink(), "CI artifact wrapper must be a directory")
            for path in folder.iterdir():
                require(path.is_file() and not path.is_symlink(), "CI artifact wrapper contains unexpected nested entries")
                safe_name(path.name)
                require(path.name.endswith(ARTIFACT_EXTENSIONS) or path.name.startswith("SHA256SUMS-"),
                        "CI artifact wrapper contains an unexpected file")
                require(path.name not in seen, "CI artifacts contain colliding filenames")
                seen.add(path.name)
        destination.mkdir(parents=True, mode=0o700)
        for folder in sorted(stage.iterdir()):
            for path in folder.iterdir():
                shutil.copy2(path, destination / path.name)


def zip_members(archive, root):
    names, total = set(), 0
    for member in archive.infolist():
        path = PurePosixPath(member.filename)
        normalized = path.as_posix()
        require(path.parts and path.parts[0] == root and not path.is_absolute()
                and ".." not in path.parts and "\\" not in member.filename
                and all(32 <= ord(char) < 127 for char in member.filename), "Windows artifact contains unsafe paths")
        mode = member.external_attr >> 16
        kind = mode & 0o170000
        require(kind in {0, 0o100000, 0o040000} and not mode & 0o7000
                and normalized not in names and not member.flag_bits & 1,
                "Windows artifact contains links, duplicate files, encryption or privileged entries")
        require(not any(part in {".git", ".env"} or part.endswith((".p12", ".p8", ".key", ".pem"))
                        for part in path.parts) and not member.filename.endswith(
                            ("/docs/secure-releases.md", "/docs/store-distribution.md")),
                "Windows artifact contains private configuration")
        total += member.file_size
        require(member.file_size >= 0 and member.file_size <= 512 * 1024**2
                and total <= MAX_BYTES and len(names) < MAX_FILES, "Windows artifact exceeds resource limits")
        names.add(normalized)
    return names


def read_zip(archive, name, maximum=512 * 1024**2):
    member = archive.getinfo(name)
    require(member.file_size <= maximum, "Windows artifact member exceeds resource limits")
    with archive.open(member) as stream:
        return stream.read(maximum + 1)


def validate_bundle_members(archive, root):
    files = {member.name[len(root) + 1:]: member for member in archive.getmembers() if member.isfile()}
    ordinary = {name: member for name, member in files.items() if not name.startswith(APP + "/")}
    allowed_root = set(BINARIES) | {"README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "CHANGELOG.md",
                                   "CONTRIBUTING.md", "SECURITY.md", ".env.example", "build-provenance.json"}
    for name, member in ordinary.items():
        path = PurePosixPath(name)
        require(name in allowed_root or name in {"assets/c64-keyboard-bridge.png", "assets/c64-keyboard-bridge.svg", "c64/README.md", "scripts/README.md",
                "c64/KEYBRIDGE.PRG", "c64/NETWORKTEST.PRG", "c64/prg-manifest.json"}
                or name.startswith(("docs/", "config/layouts/", "third-party/rust-crates/", "third-party/rust-runtime/")),
                "Mac payload contains files outside its expected resource inventory")
        require(name in BINARIES or not member.mode & 0o111,
                "Mac payload contains an unexpected executable resource")
        if name not in BINARIES:
            with archive.extractfile(member) as stream:
                prefix = stream.read(4)
            require(prefix not in {b"\x7fELF", b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe"}
                    and not prefix.startswith(b"MZ"), "Mac payload contains executable bytes in a resource")
    expected = {APP + "/Contents/Info.plist", APP + "/Contents/Resources/c64-keyboard-bridge.icns"}
    expected.update(APP + "/Contents/MacOS/" + name for name in BINARIES)
    expected.update(APP + "/Contents/Resources/" + name for name in ordinary if name not in BINARIES)
    actual = {name for name in files if name.startswith(APP + "/")}
    require(actual == expected, "Mac app bundle contains an unexpected executable or resource")
    for name in ordinary:
        copy = APP + ("/Contents/MacOS/" if name in BINARIES else "/Contents/Resources/") + name
        require(read_member(archive, root + "/" + name, 512 * 1024**2)
                == read_member(archive, root + "/" + copy, 512 * 1024**2),
                "Mac app bundle and portable resources differ")


def source_path(path):
    item = PurePosixPath(path)
    if path in {"docs/secure-releases.md", "docs/store-distribution.md"}:
        return False
    if any(part in {"build", "target", ".git", "__pycache__", "node_modules"} for part in item.parts):
        return False
    if path in ROOT_FILES:
        return True
    if path.startswith("docs/screenshots/") and item.suffix.lower() in {".png", ".json"}:
        return True
    for folder, extensions in FOLDERS.items():
        if path.startswith(folder + "/"):
            return (item.suffix.lower() in extensions or item.name == "COPYING"
                    or folder == "third-party/notices" and item.name.startswith(("LICENSE", "NOTICE")))
    return False


class GitHub:
    def __init__(self, repository):
        require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is not None,
                "Repository name is invalid")
        self.repository = repository

    def api(self, suffix):
        return json_output(run("gh", "api", f"repos/{self.repository}/{suffix}"))

    def release(self, tag):
        result = run("gh", "api", f"repos/{self.repository}/releases/tags/{tag}", allow_nonzero=True)
        if result.returncode:
            # An authenticated listing distinguishes a genuinely absent release from access/network errors.
            releases = self.api("releases?per_page=100")
            require(not any(item.get("tag_name") == tag for item in releases),
                    "Existing release could not be inspected")
            return None
        return json_output(result)


def validate_run(run_info, jobs, workflow, repository, revision):
    require(run_info.get("head_sha") == revision and run_info.get("status") == "completed"
            and run_info.get("conclusion") == "success", "CI run does not verify this exact release commit")
    require(run_info.get("event") in {"push", "workflow_dispatch"}
            and run_info.get("head_repository", {}).get("full_name") == repository
            and run_info.get("workflow_id") == workflow.get("id")
            and run_info.get("path", "").split("@")[0] == ".github/workflows/ci.yml",
            "CI run is not the trusted native workflow for this repository")
    required = set(TARGETS) | {"Matching corresponding source", "Pinned C64 PRG build"}
    successful = {job.get("name") for job in jobs if job.get("conclusion") == "success"}
    require(required <= successful, "CI run lacks successful checks for all five platforms and source")


def verify_ci_identity(run_id, github, revision):
    info = github.api(f"actions/runs/{run_id}")
    jobs = []
    page = 1
    while True:
        batch = github.api(f"actions/runs/{run_id}/jobs?filter=latest&per_page=100&page={page}")["jobs"]
        jobs.extend(batch)
        if len(batch) < 100:
            break
        page += 1
        require(page <= 10, "CI job inventory exceeds its limit")
    validate_run(info, jobs, github.api("actions/workflows/ci.yml"), github.repository, revision)


def resolve_development(revision, run_id, github, cwd=ROOT):
    require(not os.environ.get("CI") and not os.environ.get("GITHUB_ACTIONS"),
            "Untagged development promotion is local-only")
    require(re.fullmatch(r"[a-f0-9]{40}", revision) is not None
            and str(run_id).isdigit() and int(run_id) > 0, "Development promotion requires a full commit and CI run")
    run("git", "merge-base", "--is-ancestor", revision, "origin/main", cwd=cwd)
    manifest = tomllib.loads(run("git", "show", f"{revision}:Cargo.toml", cwd=cwd).stdout.decode())
    version = manifest["workspace"]["package"]["version"]
    verify_ci_identity(run_id, github, revision)
    return {"schema": 1, "tag": "unreleased-v" + version, "version": version, "revision": revision,
            "ci_run_id": int(run_id), "repository": github.repository}


def resolve_release(tag, run_id, github, cwd=ROOT):
    require(re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag) is not None, "Release tag must be a version tag")
    require(str(run_id).isdigit() and int(run_id) > 0, "CI run ID must be numeric")
    if os.environ.get("GITHUB_ACTIONS"):
        require(os.environ.get("GITHUB_REF") == "refs/heads/main"
                and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch",
                "Protected releases must be manually dispatched from main")
    revision = run("git", "rev-parse", f"refs/tags/{tag}^{{commit}}", cwd=cwd).stdout.decode().strip()
    require(re.fullmatch(r"[a-f0-9]{40}", revision) is not None, "Release tag does not resolve to a commit")
    run("git", "merge-base", "--is-ancestor", revision, "origin/main", cwd=cwd)
    manifest = tomllib.loads(run("git", "show", f"{revision}:Cargo.toml", cwd=cwd).stdout.decode())
    version = manifest["workspace"]["package"]["version"]
    require(tag == "v" + version, "Release tag does not match its Cargo version")
    verify_ci_identity(run_id, github, revision)
    existing = github.release(tag)
    require(existing is None or existing.get("draft") is True, "Published releases must never be overwritten")
    return {"schema": 1, "tag": tag, "version": version, "revision": revision,
            "ci_run_id": int(run_id), "repository": github.repository}


def validate_provenance(value, identity, target=None):
    require(value.get("project_revision") == identity["revision"] and value.get("version") == identity["version"]
            and value.get("working_tree_modified") is False,
            "Artifact provenance does not match the clean tagged commit")
    require(re.fullmatch(r"[a-f0-9]{64}", value.get("project_source_sha256", "")) is not None,
            "Artifact source hash is missing")
    if target:
        require(value.get("binary_target") == target and value.get("signing") == "unsigned",
                "Promotion accepts only the expected unsigned native artifacts")
        suffix = ".exe" if "windows" in target else ""
        require(set(value.get("executables", {})) == {name + suffix for name in BINARIES},
                "Artifact executable inventory is unexpected")
    return value["project_source_sha256"]


def verify_prg_members(archive, root, reader, provenance, c64_source_hash):
    marker = b"KBCFGv1\0"
    unconfigured = marker + bytes((1, 64)) + bytes(34) + bytes((12,)) + b"C64-Ultimate" + bytes(7)
    manifest = json.loads(reader(archive, root + "/c64/prg-manifest.json", 100000))
    actual = {}
    for name in ("KEYBRIDGE.PRG", "NETWORKTEST.PRG"):
        data = reader(archive, root + "/c64/" + name, 65537)
        require(66 < len(data) <= 65537 and data[:2] == b"\x01\x08" and data.count(marker) == 1,
                "Release PRG load address, size or configuration marker is invalid")
        offset = data.index(marker)
        require(data[offset:offset + 64] == unconfigured, "Release PRG is configured or has an unsupported version")
        actual[name] = hashlib.sha256(data).hexdigest()
    expected = {"schema": 1, "cc65_revision": "555282497c3ecf8b313d87d5973093af19c35bd5",
                "c64_source_sha256": c64_source_hash, "files": actual}
    require(manifest == expected and provenance.get("prgs") == expected,
            "Release PRGs, pinned compiler and tagged C64 source manifest disagree")


def verify_inputs(directory, identity, cwd=ROOT):
    directory = Path(directory)
    require(directory.is_dir(), "CI artifact directory is missing")
    # Per-job manifests are independent. Verify every one, including DMGs/installers and source.
    for target in TARGETS:
        verify_manifest(directory, f"SHA256SUMS-{target}", exact=False)
    verify_manifest(directory, "SHA256SUMS-source", exact=False)
    version = identity["version"]
    tracked = run("git", "ls-tree", "-rz", "--full-tree", identity["revision"], cwd=cwd).stdout.split(b"\0")
    c64_hash = hashlib.sha256()
    c64_paths = sorted(entry.split(b"\t", 1)[1].decode() for entry in tracked if entry and
                       (entry.split(b"\t", 1)[1] == b"c64/Makefile" or
                        entry.split(b"\t", 1)[1].startswith((b"c64/src/", b"c64/include/"))))
    for path in c64_paths:
        data = run("git", "show", f"{identity['revision']}:{path}", cwd=cwd).stdout
        c64_hash.update(path.encode() + b"\0" + data + b"\0")
    hashes = set()
    for target in TARGETS:
        root = f"c64-keyboard-bridge-{version}-{target}"
        if "windows" in target:
            with zipfile.ZipFile(directory / (root + ".zip")) as archive:
                zip_members(archive, root)
                provenance = json.loads(read_zip(archive, root + "/build-provenance.json", 1000000))
                hashes.add(validate_provenance(provenance, identity, target))
                verify_prg_members(archive, root, read_zip, provenance, c64_hash.hexdigest())
                for name, digest in provenance["executables"].items():
                    require(hashlib.sha256(read_zip(archive, root + "/" + name)).hexdigest() == digest,
                            "Windows executable does not match provenance")
        else:
            with bounded_tar(directory / (root + ".tar.gz")) as archive:
                archive_members(archive, root)
                if "apple" in target:
                    validate_bundle_members(archive, root)
                provenance = archive_json(archive, root + "/build-provenance.json")
                hashes.add(validate_provenance(provenance, identity, target))
                verify_prg_members(archive, root, read_member, provenance, c64_hash.hexdigest())
                for name, digest in provenance["executables"].items():
                    require(hashlib.sha256(read_member(archive, root + "/" + name, 512 * 1024**2)).hexdigest() == digest,
                            "Executable does not match provenance")
    source_root = f"c64-keyboard-bridge-{version}-source"
    with bounded_tar(directory / (source_root + ".tar.gz")) as archive:
        archive_members(archive, source_root)
        source = archive_json(archive, source_root + "/build-provenance.json")
        hashes.add(validate_provenance(source, identity))
        tracked = run("git", "ls-tree", "-rz", "--full-tree", identity["revision"], cwd=cwd).stdout.split(b"\0")
        paths = []
        for entry in tracked:
            if entry:
                metadata, encoded = entry.split(b"\t", 1)
                path = encoded.decode()
                if source_path(path):
                    require(metadata.startswith(b"100"), "Corresponding project source contains a link")
                    paths.append(path)
        digest = hashlib.sha256()
        for path in sorted(paths):
            expected = run("git", "show", f"{identity['revision']}:{path}", cwd=cwd).stdout
            packaged = read_member(archive, source_root + "/" + path, 512 * 1024**2)
            require(packaged == expected, "Corresponding source differs from the exact tagged commit")
            digest.update(path.encode() + b"\0" + expected + b"\0")
        require(len(hashes) == 1 and digest.hexdigest() in hashes,
                "Source and all five native artifact hashes disagree")
    expected = set()
    for target in TARGETS:
        expected.update(verify_manifest(directory, f"SHA256SUMS-{target}", exact=False))
    expected.update(verify_manifest(directory, "SHA256SUMS-source", exact=False))
    actual = {p.name for p in directory.iterdir() if p.is_file() and p.name.endswith(ARTIFACT_EXTENSIONS)}
    require(actual == expected, "Unexpected CI release assets were supplied")
    return next(iter(hashes))


def security_command(*arguments):
    # security's documented interactive mode keeps passwords out of process argv and shell tracing.
    require(all("\n" not in str(arg) and "\r" not in str(arg) and "\0" not in str(arg) for arg in arguments),
            "Credential contains characters unsupported by the Keychain importer")
    command = " ".join('"' + str(arg).replace("\\", "\\\\").replace('"', '\\"') + '"'
                       for arg in arguments) + "\n"
    require(len(command.encode()) < 4096 and len(arguments) <= 32,
            "Credential command exceeds the Keychain importer's limits")
    result = run("/usr/bin/security", "-i", input=command.encode(), diagnostic_operation=arguments[0])
    require(b"security:" not in result.stderr, "Temporary Keychain operation failed; details suppressed")
    return result


def validate_keychain_search_list(paths):
    require(isinstance(paths, list) and len(paths) <= 24
            and all(isinstance(path, str) and (PurePosixPath(path).is_absolute() or Path(path).is_absolute())
                    and not any(ord(character) < 32 or ord(character) == 127 for character in path)
                    and len(path.encode("utf-8")) <= 1024 for path in paths)
            and len(paths) == len(set(paths)), "Keychain search list is malformed or oversized")
    require(sum(len(path.encode("utf-8")) for path in paths) < 3000,
            "Keychain search list exceeds its recovery bounds")
    return paths


def keychain_search_list():
    output = run("/usr/bin/security", "list-keychains", "-d", "user").stdout
    require(isinstance(output, bytes) and len(output) <= 65536, "Keychain search list response is oversized")
    try:
        paths = []
        for line in output.decode("utf-8").splitlines():
            if line.strip():
                match = re.fullmatch(r'\s*"([^"\r\n]*)"\s*', line)
                require(match is not None, "Keychain search list response is malformed")
                paths.append(match[1])
        return validate_keychain_search_list(paths)
    except UnicodeError:
        raise ReleaseError("Keychain search list response is malformed") from None


def set_keychain_search_list(paths):
    paths = validate_keychain_search_list(paths)
    security_command("list-keychains", "-d", "user", "-s", *paths)
    require(canonical_keychain_paths(keychain_search_list()) == canonical_keychain_paths(paths),
            "Keychain search list change could not be verified")


def canonical_keychain_paths(paths):
    try:
        return [str(Path(path).resolve()) for path in validate_keychain_search_list(paths)]
    except (OSError, RuntimeError):
        raise ReleaseError("Keychain search paths could not be verified") from None


def cleanup_credentials(directory):
    directory = Path(directory)
    require(directory.name == "c64-keyboard-bridge-signing" and not directory.is_symlink(),
            "Credential cleanup path is invalid")
    if not directory.exists():
        return
    require(directory.is_dir(), "Credential cleanup path is invalid")
    keychain = directory / "release.keychain-db"
    journal = directory / "keychain-search-state.json"
    state = None
    failure = False
    if journal.exists() or journal.is_symlink():
        try:
            require(journal.is_file() and not journal.is_symlink() and journal.stat().st_size <= 4096,
                    "Keychain recovery journal is unsafe or oversized")
            candidate = json.loads(journal.read_text(encoding="utf-8"))
            require(isinstance(candidate, dict) and set(candidate) == {"schema", "domain", "prior_search_list"}
                    and type(candidate.get("schema")) is int and candidate.get("schema") == 1
                    and candidate.get("domain") == "user",
                    "Keychain recovery journal is malformed")
            validate_keychain_search_list(candidate["prior_search_list"])
            state = candidate
            set_keychain_search_list(state["prior_search_list"])
        except (ReleaseError, OSError, ValueError, UnicodeError, KeyboardInterrupt):
            failure = True
    if keychain.exists() or keychain.is_symlink():
        try:
            require(not keychain.is_symlink(), "Temporary signing Keychain is unsafe")
            run("/usr/bin/security", "delete-keychain", keychain)
        except (ReleaseError, OSError, KeyboardInterrupt):
            failure = True
    if state is not None:
        try:
            require(canonical_keychain_paths(keychain_search_list()) == canonical_keychain_paths(state["prior_search_list"]),
                    "Restored Keychain search list changed during temporary Keychain removal")
        except (ReleaseError, OSError, KeyboardInterrupt):
            failure = True
    # Scrub credentials even when scope restoration/deletion failed. A validated,
    # bounded path-only journal permits the separate always-cleanup step to retry.
    for path in directory.iterdir():
        if state is not None and path == journal:
            continue
        try:
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path)
            else:
                path.unlink()
        except OSError:
            failure = True
    if not failure:
        try:
            if state is not None:
                journal.unlink()
            directory.rmdir()
        except OSError:
            failure = True
    if failure and state is not None:
        try:
            if not journal.exists():
                journal.write_text(json.dumps(state) + "\n", encoding="utf-8")
            journal.chmod(0o600)
        except OSError:
            pass
    elif failure:
        try:
            directory.rmdir()
        except OSError:
            pass
    require(not directory.exists() and not failure, "Signing credential cleanup failed; publication is blocked")


@contextmanager
def signing_credentials(directory, environment=None):
    environment = os.environ if environment is None else environment
    directory = Path(directory)
    require(directory.name == "c64-keyboard-bridge-signing" and not directory.exists(),
            "Signing credential directory must be a new private directory")
    directory.mkdir(mode=0o700, parents=True)
    keychain = directory / "release.keychain-db"
    password = secrets.token_hex(32)
    if os.environ.get("GITHUB_ACTIONS"):
        print("::add-mask::" + password, flush=True)
    try:
        p12 = base64.b64decode(environment.get("MACOS_CERTIFICATE_P12_BASE64", ""), validate=True)
        api = base64.b64decode(environment.get("APPLE_NOTARY_KEY_P8_BASE64", ""), validate=True)
        p12_password = environment.get("MACOS_CERTIFICATE_PASSWORD", "")
        require(bool(p12) and bool(api) and bool(p12_password) and len(p12) < 1024**2 and len(api) < 65536,
                "Protected signing credentials are missing or oversized")
        require(b"-----BEGIN PRIVATE KEY-----" in api, "Notarization API key format is invalid")
        certificate, api_path = directory / "certificate.p12", directory / "notary.p8"
        for path, content in ((certificate, p12), (api_path, api)):
            path.write_bytes(content)
            path.chmod(0o600)
        prior = keychain_search_list()
        require(str(keychain.resolve()) not in canonical_keychain_paths(prior),
                "New temporary Keychain is already present in the original search scope")
        journal = directory / "keychain-search-state.json"
        encoded = (json.dumps({"schema": 1, "domain": "user", "prior_search_list": prior}) + "\n").encode("utf-8")
        require(len(encoded) <= 4096, "Keychain recovery journal exceeds its private storage bound")
        journal.write_bytes(encoded)
        journal.chmod(0o600)
        security_command("create-keychain", "-p", password, keychain)
        security_command("set-keychain-settings", "-lut", "7200", keychain)
        security_command("unlock-keychain", "-p", password, keychain)
        security_command("import", certificate, "-k", keychain, "-P", p12_password,
                         "-f", "pkcs12", "-x", "-T", "/usr/bin/codesign")
        security_command("set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
                         "-s", "-k", password, keychain)
        require(keychain.is_file(), "Temporary signing Keychain was not created")
        set_keychain_search_list([str(keychain), *prior])
        yield {"keychain": keychain, "api_path": api_path}
    except (ValueError, OSError):
        raise ReleaseError("Signing credential preparation failed; details suppressed") from None
    finally:
        cleanup_credentials(directory)


def validate_identity(keychain=None, fingerprint=FINGERPRINT, team=TEAM):
    require(fingerprint == FINGERPRINT and team == TEAM,
            "Requested signing certificate or team differs from the approved identity")
    arguments = ["/usr/bin/security", "find-identity", "-v", "-p", "codesigning"]
    if keychain:
        arguments.append(keychain)
    identities = run(*arguments).stdout.decode(errors="replace")
    require(f'{fingerprint} "{IDENTITY}"' in identities,
            "Approved Developer ID certificate is not available")


def sign(path, identifier, keychain=None, diagnostic_role=None):
    arguments = ["/usr/bin/codesign", "--force", "--sign", FINGERPRINT, "--options", "runtime",
                 "--timestamp", "--identifier", identifier]
    if keychain:
        arguments += ["--keychain", keychain]
    run(*arguments, path, diagnostic_role=diagnostic_role)
    run("/usr/bin/codesign", "--verify", "--strict", path)
    evidence = run("/usr/bin/codesign", "--display", "--verbose=4", path).stderr.decode(errors="replace")
    require(f"TeamIdentifier={TEAM}\n" in evidence and f"Identifier={identifier}\n" in evidence
            and "runtime" in evidence, "Signed artifact identity or hardened runtime is incorrect")


def notarize(path, credentials, profile=None):
    arguments = ["xcrun", "notarytool", "submit", path, "--wait", "--timeout", "30m",
                 "--no-progress", "--output-format", "json"]
    if profile:
        arguments += ["--keychain-profile", profile]
    else:
        key_id = os.environ.get("APPLE_NOTARY_KEY_ID", "")
        issuer = os.environ.get("APPLE_NOTARY_ISSUER_ID", "")
        require(re.fullmatch(r"[A-Za-z0-9]{10,}", key_id) is not None, "Notarization key ID is missing or invalid")
        try:
            uuid.UUID(issuer)
        except (ValueError, AttributeError):
            raise ReleaseError("Notarization issuer ID is missing or invalid") from None
        arguments += ["--key", credentials["api_path"], "--key-id", key_id, "--issuer", issuer]
    response = json_output(run(*arguments, timeout=1900))
    require(response.get("status") == "Accepted", "Apple has not accepted notarization; publication is blocked")
    try:
        submission = str(uuid.UUID(response.get("id", "")))
    except (ValueError, TypeError, AttributeError):
        raise ReleaseError("Notarization acceptance receipt is invalid") from None
    return {"status": "Accepted", "submission_id": submission}


def staple(path):
    run("xcrun", "stapler", "staple", path)
    run("xcrun", "stapler", "validate", path)


def verify_portable_macos(archive_path, destination, root, notarized):
    """Assess the shipped archive's extracted files; never launch package executables."""
    packaged = extract_archive(archive_path, destination, root)
    app = packaged / APP
    for name in BINARIES:
        run("/usr/bin/codesign", "--verify", "--strict", packaged / name)
        run("/usr/bin/codesign", "--verify", "--strict", app / "Contents/MacOS" / name)
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", app)
    if notarized:
        run("xcrun", "stapler", "validate", app)


def promote_macos(inputs, output, identity, credentials=None, profile=None, signed_only=False):
    require(not os.environ.get("CI") or not signed_only, "CI promotion requires successful notarization")
    output = Path(output)
    require(not output.exists(), "Promotion output must be a new directory")
    output.mkdir(mode=0o700, parents=True)
    keychain = credentials.get("keychain") if credentials else None
    validate_identity(keychain, os.environ.get("MACOS_CERTIFICATE_FINGERPRINT", FINGERPRINT),
                      os.environ.get("APPLE_TEAM_ID", TEAM))
    evidence = {"schema": 1, **identity, "signing": "signed-only" if signed_only else "notarized",
                "certificate_sha1": FINGERPRINT, "team": TEAM, "app_id": APP_ID, "targets": {}}
    with tempfile.TemporaryDirectory(prefix="release-promotion-") as temporary:
        stage = Path(temporary)
        for target in TARGETS[:2]:
            root = f"c64-keyboard-bridge-{identity['version']}-{target}"
            payload = extract_archive(Path(inputs) / (root + ".tar.gz"), stage / target, root)
            app = payload / APP
            plist = plistlib.loads((app / "Contents/Info.plist").read_bytes())
            require(plist.get("CFBundleIdentifier") == APP_ID
                    and plist.get("CFBundleExecutable") == BINARIES[1]
                    and plist.get("CFBundleShortVersionString") == identity["version"],
                    "Unsigned app bundle identity is incorrect")
            provenance = json.loads((payload / "build-provenance.json").read_text())
            for name in BINARIES:
                run("/usr/bin/lipo", payload / name, "-verify_arch", "arm64" if target.startswith("aarch64") else "x86_64")
                require(sha256(payload / name) == provenance["executables"][name]
                        and sha256(app / "Contents/MacOS" / name) == provenance["executables"][name],
                        "App executable does not match tested CLI/GUI provenance")
                sign(payload / name, APP_ID + (".gui" if name.endswith("-gui") else ".cli"), keychain,
                     diagnostic_role="portable-gui" if name == BINARIES[1] else "portable-cli")
                shutil.copy2(payload / name, app / "Contents/MacOS" / name)
            provenance["signing"] = "signed-only"
            provenance["executables"] = {name: sha256(payload / name) for name in BINARIES}
            provenance["executables_scope"] = "Portable root executables; final app executable hashes are in external Mac metadata"
            provenance["notarization_evidence"] = "Final acceptance and stapling evidence is in external release metadata"
            for path in (payload / "build-provenance.json", app / "Contents/Resources/build-provenance.json"):
                path.write_text(json.dumps(provenance, indent=2) + "\n")
            sign(app, APP_ID, keychain, diagnostic_role="app-bundle")
            run("/usr/bin/codesign", "--verify", "--deep", "--strict", app)
            record = {"payload": {"status": "not-submitted"}, "app_stapled": False,
                      "dmg": {"status": "not-submitted"}, "dmg_stapled": False,
                      "portable_app_validated": False}
            if not signed_only:
                notary_zip = stage / (target + "-notary.zip")
                run("/usr/bin/ditto", "-c", "-k", "--keepParent", payload, notary_zip)
                record["payload"] = notarize(notary_zip, credentials, profile)
                staple(app)
                record["app_stapled"] = True
                run("/usr/sbin/spctl", "--assess", "--type", "execute", app)
            provenance["signing"] = evidence["signing"]
            provenance["executables"] = {name: sha256(payload / name) for name in BINARIES}
            provenance["notarization"] = {"payload": record["payload"], "app_stapled": record["app_stapled"]}
            # The app resource records the accurate pre-notary signed state. Final acceptance
            # stays external, since app resources must not change after signing/stapling.
            (payload / "build-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
            archive_path = output / (root + ".tar.gz")
            with tarfile.open(archive_path, "w:gz") as archive:
                archive.add(payload, arcname=root)
            verify_portable_macos(archive_path, stage / (target + "-portable"), root, not signed_only)
            record["portable_app_validated"] = True
            dmg_stage = stage / (target + "-dmg")
            dmg_stage.mkdir()
            shutil.copytree(app, dmg_stage / APP)
            (dmg_stage / "Applications").symlink_to("/Applications")
            dmg = output / (root + ".dmg")
            run("/usr/bin/hdiutil", "create", "-volname", "C64 Keyboard Bridge", "-srcfolder", dmg_stage,
                "-format", "UDZO", "-ov", dmg, timeout=600)
            # DMG signatures do not use hardened-runtime options.
            args = ["/usr/bin/codesign", "--force", "--sign", FINGERPRINT, "--timestamp"]
            if keychain:
                args += ["--keychain", keychain]
            run(*args, dmg, diagnostic_role="dmg")
            run("/usr/bin/codesign", "--verify", "--strict", dmg)
            if not signed_only:
                record["dmg"] = notarize(dmg, credentials, profile)
                staple(dmg)
                record["dmg_stapled"] = True
                run("/usr/sbin/spctl", "--assess", "--type", "open", "--context", "context:primary-signature", dmg)
            record["files"] = {archive_path.name: sha256(archive_path), dmg.name: sha256(dmg)}
            metadata = {"schema": 2, "artifact": dmg.name, "project_revision": identity["revision"],
                        "version": identity["version"], "ci_run_id": identity["ci_run_id"],
                        "signing": evidence["signing"], "app_id": APP_ID, "certificate_sha1": FINGERPRINT,
                        "team": TEAM, "app_executables": {name: sha256(app / "Contents/MacOS" / name) for name in BINARIES},
                        "notarization": record}
            (output / (root + ".metadata.json")).write_text(json.dumps(metadata, indent=2) + "\n")
            evidence["targets"][target] = record
    (output / "promotion.metadata.json").write_text(json.dumps(evidence, indent=2) + "\n")
    checksum_manifest(output)
    return evidence


def verify_final(directory, identity, require_notarized=True):
    entries = verify_manifest(directory)
    receipt = json.loads((Path(directory) / "promotion.metadata.json").read_text())
    require(receipt.get("revision") == identity["revision"] and receipt.get("tag") == identity["tag"]
            and receipt.get("ci_run_id") == identity["ci_run_id"] and receipt.get("app_id") == APP_ID
            and receipt.get("version") == identity["version"] and receipt.get("repository") == identity["repository"]
            and receipt.get("source_sha256") == identity["source_sha256"],
            "Promotion evidence does not match this release")
    require(receipt.get("credential_cleanup") == "succeeded", "Credential cleanup is not confirmed; publication is blocked")
    version = identity["version"]
    expected = {f"c64-keyboard-bridge-{version}-source.tar.gz", "promotion.metadata.json"}
    for target in TARGETS:
        root = f"c64-keyboard-bridge-{version}-{target}"
        expected.add(root + (".zip" if "windows" in target else ".tar.gz"))
        if "apple" in target:
            expected.update({root + ".dmg", root + ".metadata.json"})
        elif "windows" in target:
            expected.add(root + "-setup.exe")
        else:
            expected.add(f"c64-keyboard-bridge_{version}_{'arm64' if target.startswith('aarch64') else 'amd64'}.deb")
    # A promoted-only directory is accepted for transfer; an assembled set must have all retained platforms.
    if any("unknown-linux" in name or "windows" in name or "-source.tar.gz" in name for name in entries):
        require(set(entries) == expected, "Final release inventory is incomplete or unexpected")
    require(receipt.get("certificate_sha1") == FINGERPRINT and receipt.get("team") == TEAM,
            "Promotion evidence uses an unexpected signing identity")
    require(set(receipt.get("targets", {})) == set(TARGETS[:2]), "Both Mac architectures must be promoted")
    for target, record in receipt["targets"].items():
        if require_notarized:
            require(receipt.get("signing") == "notarized" and record.get("payload", {}).get("status") == "Accepted"
                    and record.get("dmg", {}).get("status") == "Accepted" and record.get("app_stapled") is True
                    and record.get("dmg_stapled") is True and record.get("portable_app_validated") is True,
                    "Mac release is not fully notarized, stapled and verified after portable extraction")
        root = f"c64-keyboard-bridge-{version}-{target}"
        require(set(record.get("files", {})) == {root + ".tar.gz", root + ".dmg"},
                "Mac acceptance evidence must name that exact architecture's archive and DMG")
        if require_notarized:
            for kind in ("payload", "dmg"):
                try:
                    uuid.UUID(record[kind]["submission_id"])
                except (ValueError, TypeError, KeyError, AttributeError):
                    raise ReleaseError("Mac notarization submission evidence is invalid") from None
        for filename, digest in record.get("files", {}).items():
            require(entries.get(filename) == digest, "Promoted Mac artifact does not match acceptance evidence")
        require(len(record.get("files", {})) == 2, "Promoted Mac archive and DMG evidence is incomplete")
    return entries


def assemble_final(inputs, promoted, destination, identity):
    destination = Path(destination)
    require(not destination.exists(), "Final release directory must be new")
    verify_final(promoted, identity)
    destination.mkdir(parents=True)
    for path in Path(inputs).iterdir():
        if path.is_file() and path.name.endswith(ARTIFACT_EXTENSIONS) and "apple-darwin" not in path.name:
            shutil.copy2(path, destination / path.name)
    for path in Path(promoted).iterdir():
        if path.is_file() and path.name.endswith(ARTIFACT_EXTENSIONS):
            shutil.copy2(path, destination / path.name)
    checksum_manifest(destination)
    verify_final(destination, identity)


def release_notes(identity, cwd=ROOT):
    """Read prepared notes and resolve repository links against the exact release commit."""
    version, revision, tag, repository = (identity.get(name, "") for name in
                                          ("version", "revision", "tag", "repository"))
    require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is not None
            and re.fullmatch(r"[a-f0-9]{40}", revision) is not None and tag == "v" + version
            and re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is not None,
            "Release notes identity is invalid")
    name = f"docs/release-notes-{version}.md"
    tree = run("git", "ls-tree", "-rz", "--full-tree", revision, cwd=cwd).stdout
    files = set()
    try:
        for entry in tree.split(b"\0"):
            if entry:
                metadata, path = entry.split(b"\t", 1)
                mode, kind, _object = metadata.split(b" ")
                if mode in {b"100644", b"100755"} and kind == b"blob":
                    files.add(path.decode("utf-8"))
    except (ValueError, UnicodeError):
        raise ReleaseError("Release notes repository inventory is malformed") from None
    require(name in files, "Prepared release notes are missing from the exact release commit")
    content = run("git", "show", f"{revision}:{name}", cwd=cwd).stdout
    require(0 < len(content) <= 65536, "Prepared release notes are empty or oversized")
    try:
        body = content.decode("utf-8")
    except UnicodeError:
        raise ReleaseError("Prepared release notes are not UTF-8") from None
    require(bool(body.strip()) and "\0" not in body, "Prepared release notes are empty or malformed")

    def destination(value, image=False):
        angled = value.startswith("<") and value.endswith(">")
        target = value[1:-1] if angled else value
        try:
            parsed = urlsplit(target)
            path = unquote(parsed.path, errors="strict")
        except (ValueError, UnicodeError):
            raise ReleaseError("Prepared release notes contain an invalid link") from None
        if parsed.scheme:
            require(parsed.scheme in {"https", "http", "mailto"}, "Prepared release notes contain an unsupported link")
            return value
        require(not parsed.netloc and not any(ord(character) < 32 for character in target)
                and "\\" not in path, "Prepared release notes contain an invalid repository link")
        if not path:
            require(not parsed.query and bool(parsed.fragment), "Prepared release notes contain an empty link")
            return value
        normalized = posixpath.normpath(path.lstrip("/") if path.startswith("/")
                                        else posixpath.join("docs", path))
        require(normalized in files, "Prepared release notes link does not name a file in the exact release commit")
        base = (f"https://raw.githubusercontent.com/{repository}/{quote(tag, safe='')}" if image
                else f"https://github.com/{repository}/blob/{quote(tag, safe='')}")
        resolved = base + "/" + quote(normalized, safe="/")
        if parsed.query:
            resolved += "?" + parsed.query
        if parsed.fragment:
            resolved += "#" + parsed.fragment
        return "<" + resolved + ">" if angled else resolved

    # Prepared notes use ordinary Markdown links and images. Preserve their labels,
    # optional titles, external URLs and fragment-only navigation.
    inline = re.compile(r'(?P<open>!?\[[^\]\n]*\]\()(?P<target><[^>\n]+>|[^ \t\n)]+)'
                        r'(?P<title>[ \t]+(?:"[^"\n]*"|\'[^\'\n]*\'|\([^\)\n]*\)))?(?P<close>\))')
    body = inline.sub(lambda match: match["open"] + destination(match["target"], match["open"].startswith("!"))
                      + (match["title"] or "") + match["close"], body)
    image_references = {match.group(1).strip().casefold() for match in re.finditer(r'!\[[^\]\n]*\]\[([^\]\n]+)\]', body)}
    references = re.compile(r'^(?P<open>[ ]{0,3}\[(?P<label>[^\]\n]+)\]:[ \t]*)'
                            r'(?P<target><[^>\n]+>|[^ \t\n]+)(?P<tail>[^\n]*)$', re.MULTILINE)
    body = references.sub(lambda match: match["open"] + destination(match["target"],
                          match["label"].strip().casefold() in image_references) + match["tail"], body)
    require(len(body.encode("utf-8")) <= 65536, "Resolved release notes exceed GitHub's body limit")
    return body


def verify_draft_body(github, identity, body):
    existing = github.release(identity["tag"])
    require(existing is not None and existing.get("draft") is True, "Release must remain a draft while uploading")
    require(existing.get("body") == body and existing.get("prerelease") is False,
            "Release body or prerelease status differs from the prepared notes; publication is blocked")
    return existing


def upload_draft(directory, identity, github, publish=False, cwd=ROOT):
    entries = verify_final(directory, identity)
    body = release_notes(identity, cwd)
    existing = github.release(identity["tag"])
    require(existing is None or existing.get("draft") is True, "Published releases must never be overwritten")
    with tempfile.TemporaryDirectory(prefix="release-notes-") as temporary:
        notes_file = Path(temporary) / "release-notes.md"
        notes_file.write_text(body, encoding="utf-8", newline="")
        if existing is None:
            run("gh", "release", "create", identity["tag"], "--repo", github.repository,
                "--verify-tag", "--draft", "--prerelease=false", "--notes-file", notes_file, "--title", identity["tag"])
        else:
            existing = github.release(identity["tag"])
            require(existing is not None and existing.get("draft") is True,
                    "Release was published before the prepared draft notes could be applied")
            run("gh", "release", "edit", identity["tag"], "--repo", github.repository,
                "--prerelease=false", "--notes-file", notes_file)
    # Remove only stale draft assets. Each operation may be partial; the release remains a draft.
    existing = verify_draft_body(github, identity, body)
    expected = set(entries) | {"SHA256SUMS"}
    for asset in existing.get("assets", []):
        if asset["name"] not in expected:
            run("gh", "release", "delete-asset", identity["tag"], safe_name(asset["name"]),
                "--yes", "--repo", github.repository)
    for filename in sorted(expected):
        verify_draft_body(github, identity, body)
        run("gh", "release", "upload", identity["tag"], Path(directory) / filename,
            "--clobber", "--repo", github.repository, timeout=600)
    with tempfile.TemporaryDirectory(prefix="release-download-") as temporary:
        run("gh", "release", "download", identity["tag"], "--dir", temporary,
            "--repo", github.repository, timeout=600)
        downloaded = verify_final(temporary, identity)
        require(downloaded == entries and {p.name for p in Path(temporary).iterdir()} == expected,
                "Uploaded release inventory or checksums disagree; publication is blocked")
    verify_draft_body(github, identity, body)
    if publish:
        run("gh", "release", "edit", identity["tag"], "--draft=false", "--repo", github.repository)
    return {"draft": not publish, "verified_assets": len(expected)}

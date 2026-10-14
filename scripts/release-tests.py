#!/usr/bin/env python3
"""Deterministic protected-release tests; no signing key, Apple account or live release is used."""
import base64
from contextlib import redirect_stderr, redirect_stdout
import hashlib
import io
import json
import os
from pathlib import Path
import plistlib
import runpy
import shlex
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import release_service as service

SENTINEL = "secret-sentinel-never-log-7fa36"
RECEIPT = {"id": "31b1c12f-1e2b-41fc-9cfa-500dab72cfce", "status": "Accepted"}


def fixture_environment(values=None):
    # Keep PATH/SystemRoot and other OS launch state, especially for Windows subprocesses.
    # Isolate only release/event/credential values; real dispatch gates stay enabled in production.
    values = {} if values is None else values
    defaults = {"CI": "", "GITHUB_ACTIONS": "", "GITHUB_REF": "", "GITHUB_EVENT_NAME": "",
                "MACOS_CERTIFICATE_P12_BASE64": "", "MACOS_CERTIFICATE_PASSWORD": "",
                "APPLE_NOTARY_KEY_P8_BASE64": "", "GH_TOKEN": "", "GITHUB_TOKEN": ""}
    return patch.dict(os.environ, {**defaults, **values})


def completed(stdout=b"", stderr=b"", returncode=0):
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


def write_tar(path, files, root, mode=0o644):
    with tarfile.open(path, "w:gz") as archive:
        for name, data in files.items():
            member = tarfile.TarInfo(root + "/" + name)
            member.size = len(data)
            member.mode = mode if Path(name).name in service.BINARIES else 0o644
            archive.addfile(member, io.BytesIO(data))


class FakeGitHub:
    repository = "batuhanoktem/c64-keyboard-bridge"

    def __init__(self, revision):
        self.existing = None
        self.run = {"head_sha": revision, "status": "completed", "conclusion": "success", "event": "push",
                    "head_repository": {"full_name": self.repository}, "workflow_id": 42,
                    "path": ".github/workflows/ci.yml"}
        self.jobs = [{"name": name, "conclusion": "success"} for name in
                     (*service.TARGETS, "Matching corresponding source", "Pinned C64 PRG build")]

    def api(self, path):
        if "/jobs?" in path:
            return {"jobs": self.jobs}
        if path == "actions/workflows/ci.yml":
            return {"id": 42}
        return self.run

    def release(self, _tag):
        return self.existing


class ReleaseFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.repo, self.inputs = self.root / "repository", self.root / "inputs"
        self.repo.mkdir()
        self.inputs.mkdir()
        self.source = {"Cargo.toml": b'[workspace.package]\nversion="0.1.0"\n',
                       "README.md": b"release fixture\n", "c64/Makefile": b"fixture build input\n",
                       "c64/src/fixture.c": b"fixture C source\n", "docs/screenshots/demo.json": b'{"fixture":true}\n',
                       "docs/screenshots/demo.png": b"png fixture", "docs/platforms.md": b"# Platforms\n",
                       "docs/release-notes-0.1.0.md": b'# Version 0.1.0\n\nFirst release; Windows is unsigned.\n'
                       b'[platforms](platforms.md#gnome-wayland) and [readme](../README.md).\n'
                       b'![fixture](screenshots/demo.png)\n',
                       "docs/release-notes-0.2.0.md": b"Wrong version notes; never upload.\n"}
        for name, data in self.source.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        def git(*args):
            return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True).stdout.decode().strip()
        git("init", "-b", "main")
        git("config", "core.autocrlf", "false")
        git("-c", "user.name=Release fixture", "-c", "user.email=fixture@example.invalid", "add", ".")
        git("-c", "user.name=Release fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "fixture")
        self.revision = git("rev-parse", "HEAD")
        git("tag", "v0.1.0")
        git("update-ref", "refs/remotes/origin/main", self.revision)
        digest = hashlib.sha256()
        for name, data in sorted(self.source.items()):
            digest.update(name.encode() + b"\0" + data + b"\0")
        self.source_hash = digest.hexdigest()
        c64_digest = hashlib.sha256()
        for name, data in sorted(self.source.items()):
            if name.startswith("c64/"):
                c64_digest.update(name.encode() + b"\0" + data + b"\0")
        self.c64_hash = c64_digest.hexdigest()
        self.identity = {"schema": 1, "tag": "v0.1.0", "version": "0.1.0", "revision": self.revision,
                         "ci_run_id": 123, "repository": FakeGitHub.repository, "source_sha256": self.source_hash}
        self.github = FakeGitHub(self.revision)
        self.build_inputs()

    def tearDown(self):
        self.temporary.cleanup()

    def manifest(self, name, assets):
        (self.inputs / name).write_text("".join(f"{service.sha256(self.inputs / asset)}  {asset}\n" for asset in assets))

    def build_inputs(self):
        for target in service.TARGETS:
            root = f"c64-keyboard-bridge-0.1.0-{target}"
            suffix = ".exe" if "windows" in target else ""
            binaries = {name + suffix: (target + name).encode() for name in service.BINARIES}
            provenance = {"schema": 2, "version": "0.1.0", "project_revision": self.revision,
                          "working_tree_modified": False, "project_source_sha256": self.source_hash,
                          "binary_target": target, "signing": "unsigned",
                          "executables": {name: hashlib.sha256(data).hexdigest() for name, data in binaries.items()}}
            marker = b"KBCFGv1\0" + bytes((1, 64)) + bytes(34) + bytes((12,)) + b"C64-Ultimate" + bytes(7)
            prgs = {name: b"\x01\x08fixture-only" + marker for name in ("KEYBRIDGE.PRG", "NETWORKTEST.PRG")}
            prg_manifest = {"schema": 1, "cc65_revision": "555282497c3ecf8b313d87d5973093af19c35bd5",
                            "c64_source_sha256": self.c64_hash,
                            "files": {name: hashlib.sha256(data).hexdigest() for name, data in prgs.items()}}
            provenance["prgs"] = prg_manifest
            files = {**binaries, "build-provenance.json": json.dumps(provenance).encode(),
                     "c64/prg-manifest.json": json.dumps(prg_manifest).encode(),
                     **{"c64/" + name: data for name, data in prgs.items()}}
            assets = []
            if "apple" in target:
                files[service.APP + "/Contents/Resources/c64-keyboard-bridge.icns"] = b"icon fixture"
                files[service.APP + "/Contents/Info.plist"] = plistlib.dumps({"CFBundleIdentifier": service.APP_ID,
                    "CFBundleExecutable": service.BINARIES[1], "CFBundleShortVersionString": "0.1.0"})
                for name, data in binaries.items():
                    files[service.APP + "/Contents/MacOS/" + name] = data
                for name, data in list(files.items()):
                    if not name.startswith(service.APP + "/") and name not in binaries:
                        files[service.APP + "/Contents/Resources/" + name] = data
                write_tar(self.inputs / (root + ".tar.gz"), files, root, 0o755)
                for extension in (".dmg", ".metadata.json"):
                    (self.inputs / (root + extension)).write_bytes(b"unsigned fixture")
                assets = [root + extension for extension in (".tar.gz", ".dmg", ".metadata.json")]
            elif "windows" in target:
                with zipfile.ZipFile(self.inputs / (root + ".zip"), "w") as archive:
                    for name, data in files.items():
                        archive.writestr(root + "/" + name, data)
                (self.inputs / (root + "-setup.exe")).write_bytes(b"installer fixture")
                assets = [root + ".zip", root + "-setup.exe"]
            else:
                write_tar(self.inputs / (root + ".tar.gz"), files, root, 0o755)
                installer = "c64-keyboard-bridge_0.1.0_" + ("arm64" if target.startswith("aarch64") else "amd64") + ".deb"
                (self.inputs / installer).write_bytes(b"deb fixture")
                assets = [root + ".tar.gz", installer]
            self.manifest("SHA256SUMS-" + target, assets)
        source_root = "c64-keyboard-bridge-0.1.0-source"
        provenance = {"version": "0.1.0", "project_revision": self.revision, "working_tree_modified": False,
                      "project_source_sha256": self.source_hash}
        write_tar(self.inputs / (source_root + ".tar.gz"),
                  {**self.source, "build-provenance.json": json.dumps(provenance).encode()}, source_root)
        self.manifest("SHA256SUMS-source", [source_root + ".tar.gz"])

    def fake_mac(self, *args, **_kwargs):
        args = [str(value) for value in args]
        tool = Path(args[0]).name
        if tool == "security":
            return completed(f'  1) {service.FINGERPRINT} "{service.IDENTITY}"\n'.encode())
        if tool == "lipo":
            self.assertEqual(len(args), 4)
            self.assertTrue(Path(args[1]).is_file(), "lipo requires its existing input file before the command")
            self.assertEqual(args[2], "-verify_arch")
            architecture = "arm64" if Path(args[1]).read_bytes().startswith(b"aarch64-apple-darwin") else "x86_64"
            service.require(args[3] == architecture, "Fixture architecture mismatch")
        if tool == "codesign" and "--display" in args:
            target = Path(args[-1])
            identifier = service.APP_ID if target.name.endswith(".app") else service.APP_ID + (".gui" if target.name.endswith("-gui") else ".cli")
            return completed(stderr=f"Identifier={identifier}\nTeamIdentifier={service.TEAM}\nflags=0x10000(runtime)\n".encode())
        if tool == "ditto" or tool == "hdiutil":
            Path(args[-1]).write_bytes(b"signed fixture")
        if "stapler" in args and args[-1].endswith(".app"):
            ticket = Path(args[-1]) / "Contents/CodeResources"
            if "staple" in args:
                ticket.write_bytes(b"fixture-only ticket")
            elif "validate" in args:
                self.assertEqual(ticket.read_bytes(), b"fixture-only ticket")
        if "notarytool" in args:
            return completed(json.dumps(RECEIPT).encode())
        return completed()

    def promoted(self):
        output = self.root / "promoted"
        with patch.object(service, "run", side_effect=self.fake_mac), fixture_environment():
            service.promote_macos(self.inputs, output, self.identity, profile="fixture-profile")
        path = output / "promotion.metadata.json"
        receipt = json.loads(path.read_text())
        receipt["credential_cleanup"] = "succeeded"
        path.write_text(json.dumps(receipt))
        service.checksum_manifest(output)
        return output

    def fixture_upload(self, final, calls, fail_upload=False, tamper_download=False, tamper_body=False):
        original_run = service.run
        def recording(*args, **kwargs):
            calls.append(tuple(str(arg) for arg in args))
            if args[0] == "git":
                return original_run(*args, **kwargs)
            if "--notes-file" in args:
                body = Path(args[args.index("--notes-file") + 1]).read_text(encoding="utf-8")
                if self.github.existing is None:
                    self.github.existing = {"draft": True, "assets": []}
                self.github.existing.update(body=body, prerelease=False)
            if "upload" in args and fail_upload:
                raise service.ReleaseError("Fixture upload failed")
            if "download" in args:
                destination = Path(args[args.index("--dir") + 1])
                for path in final.iterdir():
                    (destination / path.name).write_bytes(path.read_bytes())
                if tamper_download:
                    (destination / "c64-keyboard-bridge_0.1.0_amd64.deb").write_bytes(b"tampered")
                if tamper_body:
                    self.github.existing["body"] = "Changed outside the verified upload."
            return completed()
        return recording

    def test_exact_commit_five_platform_ci_and_source_hashes(self):
        with fixture_environment():
            resolved = service.resolve_release("v0.1.0", 123, self.github, self.repo)
        self.assertEqual(resolved["revision"], self.revision)
        self.assertEqual(service.verify_inputs(self.inputs, self.identity, self.repo), self.source_hash)

    def test_ci_requires_all_native_jobs_and_rejects_pr_or_wrong_commit(self):
        for mutate in (lambda: self.github.run.update(event="pull_request"),
                       lambda: self.github.run.update(head_sha="f" * 40),
                       lambda: self.github.jobs.pop()):
            github = FakeGitHub(self.revision)
            self.github = github
            mutate()
            with self.assertRaises(service.ReleaseError):
                service.validate_run(github.run, github.jobs, {"id": 42}, github.repository, self.revision)

    def test_release_requires_main_dispatch_and_version_tag(self):
        with fixture_environment({"GITHUB_ACTIONS": "true", "GITHUB_REF": "refs/heads/other",
                                  "GITHUB_EVENT_NAME": "workflow_dispatch"}):
            with self.assertRaisesRegex(service.ReleaseError, "main"):
                service.resolve_release("v0.1.0", 123, self.github, self.repo)
        for tag in ("--help", "v0.2.0", "v0.1.0\nsecret", "v0.1.0^{}"):
            with self.assertRaises(service.ReleaseError):
                service.resolve_release(tag, 123, self.github, self.repo)

    def test_untagged_development_identity_is_local_only_and_never_a_release_tag(self):
        with fixture_environment():
            identity = service.resolve_development(self.revision, 123, self.github, self.repo)
            self.assertEqual(identity["tag"], "unreleased-v0.1.0")
            with self.assertRaises(service.ReleaseError):
                service.resolve_release(identity["tag"], 123, self.github, self.repo)
        with fixture_environment({"GITHUB_ACTIONS": "true"}):
            with self.assertRaisesRegex(service.ReleaseError, "local-only"):
                service.resolve_development(self.revision, 123, self.github, self.repo)

    def test_published_release_is_never_replaced(self):
        self.github.existing = {"draft": False}
        with fixture_environment(), self.assertRaisesRegex(service.ReleaseError, "never"):
            service.resolve_release("v0.1.0", 123, self.github, self.repo)

    def test_source_bytes_must_match_git_even_with_updated_archive_checksum(self):
        source = "c64-keyboard-bridge-0.1.0-source"
        path = self.inputs / (source + ".tar.gz")
        with tarfile.open(path) as archive:
            provenance = archive.extractfile(source + "/build-provenance.json").read()
        write_tar(path, {**self.source, "README.md": b"tampered", "build-provenance.json": provenance}, source)
        self.manifest("SHA256SUMS-source", [path.name])
        with self.assertRaisesRegex(service.ReleaseError, "exact tagged"):
            service.verify_inputs(self.inputs, self.identity, self.repo)

    def test_configured_prg_is_rejected_even_when_platform_checksum_is_updated(self):
        root = "c64-keyboard-bridge-0.1.0-aarch64-apple-darwin"
        path = self.inputs / (root + ".tar.gz")
        with tarfile.open(path) as archive:
            files = {member.name[len(root)+1:]: archive.extractfile(member).read()
                     for member in archive if member.isfile()}
        name = "c64/KEYBRIDGE.PRG"
        data = bytearray(files[name])
        data[data.index(b"KBCFGv1\0") + 10] = 1
        files[name] = bytes(data)
        files[service.APP + "/Contents/Resources/" + name] = bytes(data)
        write_tar(path, files, root)
        self.manifest("SHA256SUMS-aarch64-apple-darwin", [root + extension for extension in (".tar.gz", ".dmg", ".metadata.json")])
        with self.assertRaisesRegex(service.ReleaseError, "configured"):
            service.verify_inputs(self.inputs, self.identity, self.repo)

    def test_malicious_archive_paths_links_duplicates_and_modes(self):
        for name, kind, mode in (("root/../../escape", tarfile.REGTYPE, 0o644),
                                 ("/root/absolute", tarfile.REGTYPE, 0o644),
                                 ("root/symlink", tarfile.SYMTYPE, 0o644),
                                 ("root/hardlink", tarfile.LNKTYPE, 0o644),
                                 ("root/fifo", tarfile.FIFOTYPE, 0o644),
                                 ("root/root-command", tarfile.REGTYPE, 0o4755),
                                 ("root/.env", tarfile.REGTYPE, 0o600),
                                 ("root/AuthKey.p8", tarfile.REGTYPE, 0o600)):
            path = self.root / "malicious.tar.gz"
            with tarfile.open(path, "w:gz") as archive:
                member = tarfile.TarInfo(name)
                member.type, member.mode, member.linkname = kind, mode, "../../escape"
                archive.addfile(member)
            with tarfile.open(path) as archive, self.assertRaises(service.ReleaseError):
                service.archive_members(archive, "root")
        with tarfile.open(path, "w:gz") as archive:
            for _ in range(2):
                archive.addfile(tarfile.TarInfo("root/duplicate"))
        with tarfile.open(path) as archive, self.assertRaises(service.ReleaseError):
            service.archive_members(archive, "root")
        self.assertFalse((self.root / "escape").exists())

    def test_artifact_wrappers_are_flattened_and_collisions_rejected(self):
        for collide in (False, True):
            destination = self.root / ("collision-inputs" if collide else "fetched-inputs")
            def downloaded(*args, **_kwargs):
                stage = Path(args[args.index("--dir") + 1])
                for wrapper in {"release-" + target for target in service.TARGETS} | {"release-source"}:
                    folder = stage / wrapper
                    folder.mkdir()
                    (folder / (wrapper + ".tar.gz")).write_bytes(b"fixture")
                    if collide:
                        (folder / "duplicate.tar.gz").write_bytes(b"collision")
                return completed()
            with patch.object(service, "run", side_effect=downloaded):
                if collide:
                    with self.assertRaisesRegex(service.ReleaseError, "colliding"):
                        service.fetch_artifacts(self.github, 123, destination)
                    self.assertFalse(destination.exists())
                else:
                    service.fetch_artifacts(self.github, 123, destination)
                    self.assertEqual(len(list(destination.iterdir())), 6)
                    self.assertTrue(all(path.is_file() for path in destination.iterdir()))

    def test_zip_private_paths_links_duplicates_and_resource_limits(self):
        for name, mode in (("root/../escape", 0o100644), ("root/.env", 0o100600),
                           ("root/key.p8", 0o100600), ("root/link", 0o120777),
                           ("root/setuid", 0o104755)):
            path = self.root / "bad.zip"
            with zipfile.ZipFile(path, "w") as archive:
                member = zipfile.ZipInfo(name)
                member.external_attr = mode << 16
                archive.writestr(member, b"fixture")
            with zipfile.ZipFile(path) as archive, self.assertRaises(service.ReleaseError):
                service.zip_members(archive, "root")
        member = zipfile.ZipInfo("root/oversized")
        member.file_size = service.MAX_BYTES + 1
        class Oversized:
            def infolist(self):
                return [member]
        with self.assertRaisesRegex(service.ReleaseError, "resource"):
            service.zip_members(Oversized(), "root")
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("root/file", b"fixture")
            archive.writestr("root/./file", b"fixture")
        with zipfile.ZipFile(path) as archive, self.assertRaises(service.ReleaseError):
            service.zip_members(archive, "root")

    def test_unexpected_bundle_executables_and_cross_target_receipts_are_rejected(self):
        root = "c64-keyboard-bridge-0.1.0-aarch64-apple-darwin"
        path = self.inputs / (root + ".tar.gz")
        with tarfile.open(path) as archive:
            files = {member.name[len(root)+1:]: archive.extractfile(member).read()
                     for member in archive if member.isfile()}
        files[service.APP + "/Contents/MacOS/unreviewed"] = b"unexpected executable"
        write_tar(path, files, root)
        with tarfile.open(path) as archive, self.assertRaisesRegex(service.ReleaseError, "unexpected"):
            service.validate_bundle_members(archive, root)
        self.build_inputs()
        output = self.promoted()
        receipt_path = output / "promotion.metadata.json"
        receipt = json.loads(receipt_path.read_text())
        receipt["targets"][service.TARGETS[0]]["files"] = receipt["targets"][service.TARGETS[1]]["files"]
        receipt_path.write_text(json.dumps(receipt))
        service.checksum_manifest(output)
        with self.assertRaisesRegex(service.ReleaseError, "exact architecture"):
            service.verify_final(output, self.identity)

    def test_public_community_documents_allowed_and_private_guides_rejected(self):
        for name in ("CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md"):
            self.assertTrue(service.source_path(name))
        for name in ("docs/secure-releases.md", "docs/store-distribution.md"):
            self.assertFalse(service.source_path(name))
            path = self.root / "private-guide.tar.gz"
            write_tar(path, {name: b"private operational fixture"}, "root")
            with tarfile.open(path) as archive, self.assertRaisesRegex(service.ReleaseError, "private"):
                service.archive_members(archive, "root")
            zip_path = self.root / "private-guide.zip"
            with zipfile.ZipFile(zip_path, "w") as archive:
                archive.writestr("root/" + name, b"private operational fixture")
            with zipfile.ZipFile(zip_path) as archive, self.assertRaisesRegex(service.ReleaseError, "private"):
                service.zip_members(archive, "root")

    def test_resource_limit_checked_before_extraction(self):
        path = self.root / "oversized.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            member = tarfile.TarInfo("root/huge")
            member.size = service.MAX_BYTES + 1
            # Test member metadata directly, without allocating a multi-gigabyte fixture.
            with self.assertRaises(service.ReleaseError):
                service.archive_members([member], "root")

    def test_signing_uses_approved_certificate_and_never_executes_package(self):
        calls = []
        def recording(*args, **kwargs):
            calls.append(tuple(str(arg) for arg in args))
            return self.fake_mac(*args, **kwargs)
        with patch.object(service, "run", side_effect=recording), fixture_environment():
            service.promote_macos(self.inputs, self.root / "checked", self.identity, profile="fixture")
        self.assertTrue(any("notarytool" in args for args in calls))
        self.assertTrue(all(Path(args[0]).name in {"security", "codesign", "ditto", "hdiutil", "spctl", "xcrun", "lipo"}
                            for args in calls))
        with self.assertRaisesRegex(service.ReleaseError, "approved"):
            service.validate_identity(fingerprint="0" * 40)

    def test_promotion_architecture_checks_use_existing_input_file_first_for_both_frontends(self):
        calls = []
        def recording(*args, **kwargs):
            calls.append(tuple(str(argument) for argument in args))
            return self.fake_mac(*args, **kwargs)
        with patch.object(service, "run", side_effect=recording), fixture_environment():
            service.promote_macos(self.inputs, self.root / "canonical-lipo", self.identity, profile="fixture")
        checks = [args for args in calls if Path(args[0]).name == "lipo"]
        self.assertEqual(len(checks), 4)
        self.assertEqual({(Path(args[1]).parent.name, Path(args[1]).name, args[2], args[3]) for args in checks},
                         {(f"c64-keyboard-bridge-0.1.0-{target}", binary, "-verify_arch", architecture)
                          for target, architecture in zip(service.TARGETS[:2], ("arm64", "x86_64"))
                          for binary in service.BINARIES})
        for check in checks:
            check_index = calls.index(check)
            sign_index = next(index for index, args in enumerate(calls)
                              if Path(args[0]).name == "codesign" and "--sign" in args and args[-1] == check[1])
            self.assertLess(check_index, sign_index)

    def test_mismatched_architecture_blocks_signing_and_promotion_evidence(self):
        root = "c64-keyboard-bridge-0.1.0-aarch64-apple-darwin"
        path = self.inputs / (root + ".tar.gz")
        with tarfile.open(path) as archive:
            files = {member.name[len(root)+1:]: archive.extractfile(member).read()
                     for member in archive if member.isfile()}
        files[service.BINARIES[0]] = b"x86_64-apple-darwin wrong architecture fixture"
        write_tar(path, files, root)
        calls = []
        def recording(*args, **kwargs):
            calls.append(tuple(str(argument) for argument in args))
            return self.fake_mac(*args, **kwargs)
        output = self.root / "mismatched-lipo"
        with patch.object(service, "run", side_effect=recording), fixture_environment(), \
                self.assertRaisesRegex(service.ReleaseError, "architecture mismatch"):
            service.promote_macos(self.inputs, output, self.identity, profile="fixture")
        self.assertTrue(any(Path(args[0]).name == "lipo" for args in calls))
        self.assertFalse(any(Path(args[0]).name == "codesign" for args in calls))
        self.assertFalse((output / "promotion.metadata.json").exists())

    def test_rejected_and_pending_notarization_block_promotion(self):
        for status in ("Invalid", "In Progress", "Rejected", None):
            with patch.object(service, "run", return_value=completed(json.dumps({**RECEIPT, "status": status}).encode())):
                with self.assertRaisesRegex(service.ReleaseError, "publication"):
                    service.notarize(self.root / "fixture.zip", None, "fixture")

    def test_app_then_archive_then_dmg_notary_order_and_cleanup_gate(self):
        calls = []
        def recording(*args, **kwargs):
            calls.append(tuple(str(arg) for arg in args))
            return self.fake_mac(*args, **kwargs)
        output = self.root / "ordering"
        with patch.object(service, "run", side_effect=recording), fixture_environment():
            service.promote_macos(self.inputs, output, self.identity, profile="fixture")
        with self.assertRaisesRegex(service.ReleaseError, "cleanup"):
            service.verify_final(output, self.identity)
        submit = [i for i, args in enumerate(calls) if "notarytool" in args]
        stapled_app = [i for i, args in enumerate(calls) if "stapler" in args and "staple" in args and args[-1].endswith(".app")]
        dmgs = [i for i, args in enumerate(calls) if Path(args[0]).name == "hdiutil"]
        for n in range(2):
            self.assertLess(submit[2*n], stapled_app[n])
            self.assertLess(stapled_app[n], dmgs[n])
            self.assertLess(dmgs[n], submit[2*n+1])

    def test_signed_only_local_artifacts_cannot_be_public(self):
        output = self.root / "signed-only"
        with patch.object(service, "run", side_effect=self.fake_mac), fixture_environment():
            service.promote_macos(self.inputs, output, self.identity, signed_only=True)
        receipt_path = output / "promotion.metadata.json"
        receipt = json.loads(receipt_path.read_text())
        receipt["credential_cleanup"] = "succeeded"
        receipt_path.write_text(json.dumps(receipt))
        service.checksum_manifest(output)
        with self.assertRaisesRegex(service.ReleaseError, "notarized"):
            service.verify_final(output, self.identity)

    def test_shipped_portable_signature_or_ticket_failure_blocks_promotion(self):
        for failed_tool in ("codesign", "stapler"):
            output = self.root / ("failed-portable-" + failed_tool)
            def recording(*args, **kwargs):
                tool = Path(str(args[0])).name
                if any(part.endswith("-portable") for part in Path(args[-1]).parts) and (
                        tool == "codesign" and failed_tool == "codesign" or
                        tool == "xcrun" and "stapler" in args and failed_tool == "stapler"):
                    raise service.ReleaseError("Fixture shipped portable validation failed")
                return self.fake_mac(*args, **kwargs)
            with patch.object(service, "run", side_effect=recording), fixture_environment():
                with self.assertRaisesRegex(service.ReleaseError, "portable"):
                    service.promote_macos(self.inputs, output, self.identity, profile="fixture")
            self.assertFalse((output / "promotion.metadata.json").exists())

    def test_publication_requires_shipped_portable_app_validation(self):
        output = self.promoted()
        path = output / "promotion.metadata.json"
        receipt = json.loads(path.read_text())
        self.assertTrue(all(record["portable_app_validated"] for record in receipt["targets"].values()))
        receipt["targets"][service.TARGETS[0]]["portable_app_validated"] = False
        path.write_text(json.dumps(receipt))
        service.checksum_manifest(output)
        with self.assertRaisesRegex(service.ReleaseError, "portable extraction"):
            service.verify_final(output, self.identity)

    def test_scripts_readme_mirror_is_accepted_and_mismatch_rejected(self):
        root = "c64-keyboard-bridge-0.1.0-aarch64-apple-darwin"
        path = self.inputs / (root + ".tar.gz")
        with tarfile.open(path) as archive:
            files = {member.name[len(root)+1:]: archive.extractfile(member).read()
                     for member in archive if member.isfile()}
        readme = (service.ROOT / "scripts/README.md").read_bytes()
        name = "scripts/README.md"
        mirror = service.APP + "/Contents/Resources/" + name
        files[name] = files[mirror] = readme
        write_tar(path, files, root)
        with tarfile.open(path) as archive:
            service.archive_members(archive, root)
            service.validate_bundle_members(archive, root)
        files[mirror] = b"mismatched app resource"
        write_tar(path, files, root)
        with tarfile.open(path) as archive, self.assertRaisesRegex(service.ReleaseError, "resources differ"):
            service.validate_bundle_members(archive, root)

    def test_final_inventory_and_checksum_mismatch(self):
        promoted = self.promoted()
        final = self.root / "final"
        service.assemble_final(self.inputs, promoted, final, self.identity)
        self.assertEqual(len(service.verify_final(final, self.identity)), 14)
        (final / "c64-keyboard-bridge_0.1.0_amd64.deb").write_bytes(b"tampered")
        with self.assertRaisesRegex(service.ReleaseError, "checksum"):
            service.verify_final(final, self.identity)

    def test_partial_upload_leaves_draft_and_never_publishes(self):
        promoted = self.promoted()
        final = self.root / "upload-final"
        service.assemble_final(self.inputs, promoted, final, self.identity)
        self.github.existing = {"draft": True, "assets": []}
        calls = []
        with patch.object(service, "run", side_effect=self.fixture_upload(final, calls, fail_upload=True)), \
                self.assertRaisesRegex(service.ReleaseError, "Fixture upload failed"):
            service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
        self.assertFalse(any("--draft=false" in args for args in calls))
        self.assertTrue(self.github.existing["draft"])

    def test_download_mismatch_prevents_publication_and_success_stays_draft(self):
        promoted = self.promoted()
        final = self.root / "download-final"
        service.assemble_final(self.inputs, promoted, final, self.identity)
        self.github.existing = {"draft": True, "assets": []}
        for tamper in (True, False):
            calls = []
            with patch.object(service, "run", side_effect=self.fixture_upload(final, calls, tamper_download=tamper)):
                if tamper:
                    with self.assertRaises(service.ReleaseError):
                        service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
                else:
                    self.assertTrue(service.upload_draft(final, self.identity, self.github, cwd=self.repo)["draft"])
            self.assertFalse(any("--draft=false" in args for args in calls))

    def test_prepared_notes_select_exact_tagged_version_and_ignore_working_tree(self):
        expected = service.release_notes(self.identity, self.repo)
        self.assertIn("First release; Windows is unsigned.", expected)
        self.assertNotIn("Wrong version", expected)
        (self.repo / "docs/release-notes-0.1.0.md").write_text("Uncommitted wrong notes.")
        (self.repo / "docs/platforms.md").unlink()
        self.assertEqual(service.release_notes(self.identity, self.repo), expected)

    def test_prepared_notes_normalize_document_images_and_reference_links(self):
        body = (b'# Release\n[platforms](platforms.md#gnome-wayland "Platform guide")\n'
                b'![fixture](screenshots/demo.png)\n[space](<../README.md#overview>)\n'
                b'[external](https://example.invalid/guide#part) [mail](mailto:fixture@example.invalid)\n'
                b'[self](#release)\n[guide][manual]\n![capture][picture]\n'
                b'[manual]: platforms.md#gnome-wayland "Platform guide"\n'
                b'[picture]: screenshots/demo.png\n')
        original_run = service.run
        def notes(*args, **kwargs):
            if args[:2] == ("git", "show"):
                return completed(body)
            return original_run(*args, **kwargs)
        with patch.object(service, "run", side_effect=notes):
            resolved = service.release_notes(self.identity, self.repo)
        base = "https://github.com/" + self.github.repository + "/blob/v0.1.0/"
        raw = "https://raw.githubusercontent.com/" + self.github.repository + "/v0.1.0/"
        self.assertIn(f'({base}docs/platforms.md#gnome-wayland "Platform guide")', resolved)
        self.assertIn(f'![fixture]({raw}docs/screenshots/demo.png)', resolved)
        self.assertIn(f'(<{base}README.md#overview>)', resolved)
        self.assertIn(f'[manual]: {base}docs/platforms.md#gnome-wayland "Platform guide"', resolved)
        self.assertIn(f'[picture]: {raw}docs/screenshots/demo.png', resolved)
        for unchanged in ('https://example.invalid/guide#part', 'mailto:fixture@example.invalid', '[self](#release)'):
            self.assertIn(unchanged, resolved)

    def test_missing_or_invalid_prepared_notes_block_draft_creation(self):
        final = self.root / "missing-notes-final"
        service.assemble_final(self.inputs, self.promoted(), final, self.identity)
        original_run = service.run
        for corrupt in ("missing", "empty", "oversized", "utf8", "missing-link", "escape", "symlink", "unsafe"):
            calls = []
            def failure(*args, **kwargs):
                calls.append(tuple(str(arg) for arg in args))
                if args[:2] == ("git", "ls-tree") and corrupt == "missing":
                    return completed()
                if args[:2] == ("git", "ls-tree") and corrupt == "symlink":
                    tree = original_run(*args, **kwargs).stdout
                    return completed(tree + b"120000 blob " + b"0" * 40 + b"\toutside-link\0")
                if args[:2] == ("git", "show"):
                    value = {"empty": b" \n", "oversized": b"x" * 65537, "utf8": b"\xff",
                             "missing-link": b"[missing](unknown.md)", "escape": b"[escape](../../README.md)",
                             "symlink": b"[link](../outside-link)", "unsafe": b"[unsafe](javascript:alert)"}
                    return completed(value[corrupt])
                return original_run(*args, **kwargs)
            with self.subTest(corrupt=corrupt), patch.object(service, "run", side_effect=failure), \
                    self.assertRaises(service.ReleaseError):
                service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
            self.assertFalse(any(args[0] == "gh" for args in calls))

    def test_new_and_existing_drafts_receive_exact_prepared_body(self):
        final = self.root / "notes-final"
        service.assemble_final(self.inputs, self.promoted(), final, self.identity)
        expected = service.release_notes(self.identity, self.repo)
        for existing in (False, True):
            self.github.existing = {"draft": True, "prerelease": True, "body": "old notes", "assets": []} if existing else None
            calls = []
            with patch.object(service, "run", side_effect=self.fixture_upload(final, calls)):
                result = service.upload_draft(final, self.identity, self.github, cwd=self.repo)
            self.assertTrue(result["draft"])
            self.assertEqual(self.github.existing["body"], expected)
            self.assertIs(self.github.existing["prerelease"], False)
            notes_command = next(args for args in calls if "--notes-file" in args)
            self.assertEqual(notes_command[2], "edit" if existing else "create")
            self.assertIn("--prerelease=false", notes_command)
            if existing:
                self.assertNotIn("--draft", notes_command)
            self.assertNotIn("--generate-notes", notes_command)
            self.assertFalse(Path(notes_command[notes_command.index("--notes-file") + 1]).exists())
            self.assertFalse(any("--draft=false" in args for args in calls))

    def test_changed_body_after_upload_download_blocks_publication(self):
        final = self.root / "changed-notes-final"
        service.assemble_final(self.inputs, self.promoted(), final, self.identity)
        calls = []
        with patch.object(service, "run", side_effect=self.fixture_upload(final, calls, tamper_body=True)), \
                self.assertRaisesRegex(service.ReleaseError, "body"):
            service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
        self.assertTrue(any("download" in args for args in calls))
        self.assertFalse(any("--draft=false" in args for args in calls))
        self.assertTrue(self.github.existing["draft"])

    def test_body_mismatch_before_upload_blocks_assets_and_publication(self):
        final = self.root / "rejected-notes-final"
        service.assemble_final(self.inputs, self.promoted(), final, self.identity)
        calls = []
        recording = self.fixture_upload(final, calls)
        def ignored_body(*args, **kwargs):
            result = recording(*args, **kwargs)
            if "--notes-file" in args:
                self.github.existing["body"] = "Server did not retain the expected body."
            return result
        with patch.object(service, "run", side_effect=ignored_body), self.assertRaisesRegex(service.ReleaseError, "body"):
            service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
        self.assertFalse(any("upload" in args or "--draft=false" in args for args in calls))

    def test_draft_published_before_notes_update_is_never_reopened(self):
        final = self.root / "published-during-notes-final"
        service.assemble_final(self.inputs, self.promoted(), final, self.identity)
        calls = []
        states = [{"draft": True, "assets": [], "body": "old notes"}, {"draft": False, "assets": []}]
        with patch.object(self.github, "release", side_effect=states), \
                patch.object(service, "run", side_effect=self.fixture_upload(final, calls)), \
                self.assertRaisesRegex(service.ReleaseError, "published before"):
            service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
        self.assertFalse(any(args[0] == "gh" for args in calls))

    def test_verified_body_and_assets_allow_explicit_normal_publication(self):
        final = self.root / "published-with-notes-final"
        service.assemble_final(self.inputs, self.promoted(), final, self.identity)
        calls = []
        with patch.object(service, "run", side_effect=self.fixture_upload(final, calls)):
            result = service.upload_draft(final, self.identity, self.github, publish=True, cwd=self.repo)
        self.assertFalse(result["draft"])
        download = next(index for index, args in enumerate(calls) if "download" in args)
        publication = next(index for index, args in enumerate(calls) if "--draft=false" in args)
        self.assertLess(download, publication)
        self.assertIs(self.github.existing["prerelease"], False)


class CredentialTests(unittest.TestCase):
    def test_fixture_environment_preserves_os_process_launch_state(self):
        original_path = os.environ.get("PATH")
        with patch.dict(os.environ, {"GITHUB_ACTIONS": "true", "GITHUB_EVENT_NAME": "push",
                                     "SystemRoot": "fixture-system-root"}):
            with fixture_environment():
                self.assertEqual(os.environ.get("PATH"), original_path)
                self.assertEqual(os.environ.get("SystemRoot"), "fixture-system-root")
                self.assertEqual(os.environ["GITHUB_ACTIONS"], "")
                self.assertEqual(os.environ["GITHUB_EVENT_NAME"], "")
            self.assertEqual(os.environ["GITHUB_ACTIONS"], "true")

    def test_sensitive_process_output_and_environment_are_never_exposed(self):
        with patch.dict(os.environ, {"MACOS_CERTIFICATE_PASSWORD": SENTINEL, "GH_TOKEN": SENTINEL}):
            with patch.object(service.subprocess, "run", return_value=completed(SENTINEL.encode(), SENTINEL.encode(), 1)) as child:
                with self.assertRaises(service.ReleaseError) as result:
                    service.run("codesign", "fixture")
                self.assertNotIn(SENTINEL, str(result.exception))
                self.assertNotIn("MACOS_CERTIFICATE_PASSWORD", child.call_args.kwargs["env"])
                self.assertNotIn("GH_TOKEN", child.call_args.kwargs["env"])
        with patch.object(service.subprocess, "run", side_effect=subprocess.TimeoutExpired(SENTINEL, 10)):
            with self.assertRaises(service.ReleaseError) as result:
                service.run("fixture")
            self.assertNotIn(SENTINEL, str(result.exception))

    def test_known_tool_failure_reports_only_fixed_phase_and_integer_status(self):
        cases = (("codesign", "--force", "sign"), ("codesign", "--verify", "verify"),
                 ("hdiutil", "create", "create-dmg"), ("gh", "api", "api"),
                 ("security", "find-identity", "find-identity"), ("lipo", "-verify_arch", "verify-architecture"))
        for tool, verb, operation in cases:
            stdout, stderr = io.StringIO(), io.StringIO()
            with self.subTest(tool=tool, verb=verb), redirect_stdout(stdout), redirect_stderr(stderr), \
                    patch.dict(os.environ, {"MACOS_CERTIFICATE_PASSWORD": SENTINEL, "GH_TOKEN": SENTINEL}), \
                    patch.object(service.subprocess, "run", return_value=completed(SENTINEL.encode(), SENTINEL.encode(), 17)):
                with self.assertRaises(service.ReleaseError) as failure:
                    service.run("/private/" + SENTINEL + "/" + tool, verb, SENTINEL, input=SENTINEL.encode(),
                                env={"PRIVATE_VALUE": SENTINEL}, diagnostic_operation=SENTINEL)
            message = str(failure.exception)
            self.assertEqual(message, "Release tool failed; raw output was suppressed "
                             f"(tool={tool}, operation={operation}, exit_code=17)")
            self.assertEqual(stdout.getvalue() + stderr.getvalue(), "")
            self.assertNotIn(SENTINEL, message)

    def test_unknown_tools_operations_and_noninteger_status_never_reach_diagnostics(self):
        for executable, verb, status in ((SENTINEL, SENTINEL, 9), ("/private/" + SENTINEL, "--force", 9),
                                         ("codesign", SENTINEL, 9), ("codesign", "--verify", SENTINEL)):
            with self.subTest(executable=executable, status=status), \
                    patch.object(service.subprocess, "run", return_value=completed(SENTINEL.encode(), SENTINEL.encode(), status)):
                with self.assertRaises(service.ReleaseError) as failure:
                    service.run(executable, verb, SENTINEL, diagnostic_operation=SENTINEL)
            self.assertNotIn(SENTINEL, str(failure.exception))
            self.assertNotIn("/private/", str(failure.exception))
            if Path(executable).name != "codesign":
                self.assertEqual(str(failure.exception), "Release tool failed; raw output was suppressed")
            if status == SENTINEL:
                self.assertNotIn("exit_code", str(failure.exception))

    def test_notary_and_staple_failures_have_distinct_safe_phases(self):
        for arguments, operation in ((('notarytool', 'submit'), 'submit-notarization'),
                                     (('stapler', 'staple'), 'staple'), (('stapler', 'validate'), 'validate-staple')):
            with self.subTest(operation=operation), \
                    patch.object(service.subprocess, "run", return_value=completed(SENTINEL.encode(), SENTINEL.encode(), 3)):
                with self.assertRaises(service.ReleaseError) as failure:
                    service.run("xcrun", *arguments, SENTINEL, "--key-id", SENTINEL)
            self.assertIn("operation=" + operation, str(failure.exception))
            self.assertIn("exit_code=3", str(failure.exception))
            self.assertNotIn(SENTINEL, str(failure.exception))

    def test_timeout_and_launch_errors_suppress_exception_details_and_have_no_exit_code(self):
        for error in (subprocess.TimeoutExpired(SENTINEL, 10, output=SENTINEL.encode(), stderr=SENTINEL.encode()),
                      OSError(SENTINEL)):
            for executable in ("codesign", SENTINEL):
                with self.subTest(executable=executable, error=type(error).__name__), \
                        patch.object(service.subprocess, "run", side_effect=error):
                    with self.assertRaises(service.ReleaseError) as failure:
                        service.run(executable, "--verify", SENTINEL)
                expected = "Release tool could not complete; raw output was suppressed"
                if executable == "codesign":
                    expected += " (tool=codesign, operation=verify)"
                self.assertEqual(str(failure.exception), expected)
                self.assertNotIn(SENTINEL, str(failure.exception))
                self.assertNotIn("exit_code", str(failure.exception))

    def test_success_and_explicit_nonzero_return_keep_existing_behavior(self):
        for status, allowed in ((0, False), (17, True)):
            result = completed(b"ordinary result", b"ordinary stderr", status)
            with patch.object(service.subprocess, "run", return_value=result):
                self.assertIs(service.run("codesign", "--verify", allow_nonzero=allowed), result)

    def test_keychain_stdin_failure_names_only_the_allowlisted_operation(self):
        with patch.object(service.subprocess, "run", return_value=completed(SENTINEL.encode(), SENTINEL.encode(), 11)):
            with self.assertRaises(service.ReleaseError) as failure:
                service.security_command("import", SENTINEL, "-P", SENTINEL)
        self.assertEqual(str(failure.exception), "Release tool failed; raw output was suppressed "
                         "(tool=security, operation=import, exit_code=11)")
        self.assertNotIn(SENTINEL, str(failure.exception))

    def test_canonical_lipo_failure_phase_never_includes_input_path(self):
        with patch.object(service.subprocess, "run", return_value=completed(SENTINEL.encode(), SENTINEL.encode(), 1)):
            with self.assertRaises(service.ReleaseError) as failure:
                service.run("/usr/bin/lipo", "/private/" + SENTINEL, "-verify_arch", "arm64")
        self.assertEqual(str(failure.exception), "Release tool failed; raw output was suppressed "
                         "(tool=lipo, operation=verify-architecture, exit_code=1)")
        self.assertNotIn(SENTINEL, str(failure.exception))

    def test_credentials_private_stdin_only_and_cleanup_on_success_failure_cancel(self):
        environment = {"MACOS_CERTIFICATE_P12_BASE64": base64.b64encode(b"fixture certificate").decode(),
                       "MACOS_CERTIFICATE_PASSWORD": SENTINEL,
                       "APPLE_NOTARY_KEY_P8_BASE64": base64.b64encode(b"-----BEGIN PRIVATE KEY-----\nfixture\n").decode()}
        for failure in (None, "operation", "cancel"):
            with tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary) / "c64-keyboard-bridge-signing"
                calls = []
                def recording(*args, **kwargs):
                    calls.append((args, kwargs))
                    if args[:2] == ("/usr/bin/security", "-i"):
                        command = shlex.split(kwargs["input"].decode())
                        if command[0] == "create-keychain":
                            Path(command[-1]).write_bytes(b"temporary fixture keychain")
                        if command[0] == "import":
                            self.assertNotIn("-A", command)
                            self.assertIn("-x", command)
                    if args[:2] == ("/usr/bin/security", "delete-keychain"):
                        Path(args[-1]).unlink()
                    return completed()
                with patch.object(service, "run", side_effect=recording), fixture_environment():
                    try:
                        with service.signing_credentials(directory, environment):
                            if os.name != "nt":
                                self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
                                self.assertEqual((directory / "notary.p8").stat().st_mode & 0o777, 0o600)
                            if failure == "operation":
                                raise service.ReleaseError("Fixture operation failed")
                            if failure == "cancel":
                                raise KeyboardInterrupt()
                    except (service.ReleaseError, KeyboardInterrupt):
                        self.assertIsNotNone(failure)
                self.assertFalse(directory.exists())
                self.assertTrue(all(SENTINEL not in str(args) for args, _kwargs in calls))
                self.assertTrue(any(SENTINEL in kwargs.get("input", b"").decode() for _args, kwargs in calls))

    def test_cleanup_failure_is_retained_and_blocks_promotion(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "c64-keyboard-bridge-signing"
            directory.mkdir()
            (directory / "release.keychain-db").write_bytes(b"fixture")
            with patch.object(service, "run", side_effect=service.ReleaseError("cleanup fixture")):
                with self.assertRaisesRegex(service.ReleaseError, "publication"):
                    service.cleanup_credentials(directory)
            self.assertFalse(directory.exists())

    def test_invalid_credentials_cleanup_without_plaintext_fallback(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "c64-keyboard-bridge-signing"
            with fixture_environment(), self.assertRaises(service.ReleaseError):
                with service.signing_credentials(directory, {"MACOS_CERTIFICATE_P12_BASE64": "not-base64"}):
                    self.fail("Invalid credentials accepted")
            self.assertFalse(directory.exists())
            self.assertEqual(list(Path(temporary).iterdir()), [])

    def test_cli_unexpected_parse_failure_is_sanitized(self):
        # The entrypoint's broad handler is exercised in a subprocess without a live API call.
        driver = "import runpy,sys;sys.path.insert(0,'scripts');import release_service;release_service.resolve_release=lambda *a,**k: (_ for _ in ()).throw(Exception('" + SENTINEL + "'));sys.argv=['release.py','validate','--tag','v0.1.0','--ci-run','123','--inputs','unused'];runpy.run_path('scripts/release.py',run_name='__main__')"
        result = subprocess.run([sys.executable, "-c", driver], cwd=service.ROOT, capture_output=True)
        self.assertEqual(result.returncode, 1)
        self.assertNotIn(SENTINEL.encode(), result.stdout + result.stderr)
        self.assertNotIn(b"Traceback", result.stderr)

    def test_keychain_parser_quotes_one_token_and_bounds_input(self):
        password = "punctuation single' double\" slash\\ dollar$ ; spaces"
        with patch.object(service, "run", return_value=completed()) as recording:
            service.security_command("import", "fixture.p12", "-P", password)
        command = recording.call_args.kwargs["input"].decode()
        self.assertEqual(shlex.split(command), ["import", "fixture.p12", "-P", password])
        self.assertIn('"-P" "punctuation', command)
        for value in ("bad\ncommand", "bad\rcommand", "bad\0command", "x" * 4096):
            with self.assertRaises(service.ReleaseError):
                service.security_command("import", "-P", value)

    def test_github_missing_release_uses_bounded_scrubbed_runner(self):
        github = service.GitHub(FakeGitHub.repository)
        with patch.object(service, "run", side_effect=[completed(stderr=b"HTTP 404", returncode=1), completed(b"[]")]) as tools:
            self.assertIsNone(github.release("v0.1.0"))
            self.assertTrue(tools.call_args_list[0].kwargs["allow_nonzero"])

    def test_provision_helper_requires_protected_environment_and_pipes_secrets(self):
        helper = runpy.run_path(str(service.ROOT / "scripts/release-secrets.py"))
        provision = helper["provision"]
        class Protection:
            def __init__(self, _repo):
                pass
            def api(self, path):
                if path.endswith("deployment-branch-policies"):
                    return {"branch_policies": [{"name": "main", "type": "branch"}]}
                return {"can_admins_bypass": False, "deployment_branch_policy": {"custom_branch_policies": True},
                        "protection_rules": [{"type": "required_reviewers", "prevent_self_review": False,
                                              "reviewers": [{"reviewer": {"login": "batuhanoktem"}}]}]}
        calls = []
        def recording(*args, **kwargs):
            calls.append((args, kwargs))
            return completed()
        with patch.dict(provision.__globals__, {"GitHub": Protection, "run": recording}):
            provision(FakeGitHub.repository, "macos-release", b"p12", SENTINEL, b"p8", "ABCDEFGHIJ", RECEIPT["id"])
        self.assertEqual(sum("secret" in args for args, _kwargs in calls), 3)
        self.assertTrue(all(SENTINEL not in str(args) for args, _kwargs in calls))
        self.assertTrue(any(kwargs.get("input") == SENTINEL.encode() for _args, kwargs in calls))
        with self.assertRaises(service.ReleaseError):
            provision(FakeGitHub.repository, "unprotected", b"p12", SENTINEL, b"p8", "ID", "UUID")
        calls.clear()
        with patch.dict(provision.__globals__, {"GitHub": Protection, "run": recording}):
            with self.assertRaisesRegex(service.ReleaseError, "48KB"):
                provision(FakeGitHub.repository, "macos-release", b"x" * 37000, SENTINEL, b"p8", "ID", "UUID")
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()

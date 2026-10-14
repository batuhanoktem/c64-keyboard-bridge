#!/usr/bin/env python3
"""Credential-free recovery fixtures: no live API, keys, signing or input injection."""
import copy
from datetime import datetime, timedelta, timezone
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch
import uuid
import zipfile

import release_recovery as recovery
import release_service as service

spec = importlib.util.spec_from_file_location("release_fixture_support", Path(__file__).with_name("release-tests.py"))
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)
PUBLIC_CERTIFICATE = b"public fixture certificate, not a signing key"
TOKEN = "GH_scoped_fixture_token_never_print"


def iso(value):
    return value.isoformat(timespec="seconds").replace("+00:00", "Z")


class RecoveryGitHub(fixtures.FakeGitHub):
    def __init__(self, revision):
        super().__init__(revision)
        now = datetime.now(timezone.utc)
        self.promotion = {"id": 77, "event": "workflow_dispatch", "head_branch": "main",
                          "path": recovery.PROMOTION_WORKFLOW, "workflow_id": 88, "run_attempt": 1,
                          "head_sha": revision, "status": "completed", "conclusion": "failure",
                          "repository": {"id": 7, "full_name": self.repository},
                          "head_repository": {"id": 7, "full_name": self.repository}}
        self.promotion_jobs = []
        for index, (name, names) in enumerate(zip(recovery.JOBS, (recovery.VALIDATE_STEPS, recovery.SIGN_STEPS,
                                                               (recovery.UPLOAD_FETCH_STEP, recovery.UPLOAD_STEP)))):
            self.promotion_jobs.append({"id": index + 1, "name": name, "run_id": 77, "run_attempt": 1,
                "head_sha": revision, "status": "completed", "conclusion": "failure" if index == 2 else "success",
                "started_at": iso(now - timedelta(minutes=5)), "completed_at": iso(now - timedelta(minutes=2)),
                "steps": [{"name": step, "number": number + 1, "status": "completed",
                           "conclusion": "failure" if step == recovery.UPLOAD_STEP else "success"}
                          for number, step in enumerate(names)]})
        self.reviews = [{"state": "approved", "user": {"login": recovery.REVIEWER}, "comment": fixtures.SENTINEL,
                         "environments": [{"id": 23, "name": "macos-release"}]}]
        self.artifacts = [{"id": 91, "name": recovery.PROMOTED_ARTIFACT, "expired": False,
            "created_at": iso(now - timedelta(minutes=3)), "expires_at": iso(now + timedelta(days=14)),
            "size_in_bytes": 1, "digest": "sha256:" + "a" * 64,
            "workflow_run": {"id": 77, "repository_id": 7, "head_repository_id": 7,
                             "head_branch": "main", "head_sha": revision}}]

    def api(self, suffix):
        if suffix == "":
            return {"id": 7, "full_name": self.repository}
        if suffix == "actions/workflows/release.yml":
            return {"id": 88, "path": recovery.PROMOTION_WORKFLOW}
        if suffix == "actions/runs/77":
            return self.promotion
        if suffix.startswith("actions/runs/77/jobs?"):
            return {"jobs": self.promotion_jobs}
        if suffix == "actions/runs/77/approvals":
            return self.reviews
        if suffix.startswith("actions/runs/77/artifacts?"):
            return {"artifacts": self.artifacts}
        return super().api(suffix)


class RecoveryFixture(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.ReleaseFixture()
        self.fixture.setUp()
        self.addCleanup(self.fixture.tearDown)
        self.root = self.fixture.root
        self.repo = self.fixture.repo
        workflow = (service.ROOT / recovery.PROMOTION_WORKFLOW).read_bytes()
        self.fixture.source[recovery.PROMOTION_WORKFLOW] = workflow
        path = self.repo / recovery.PROMOTION_WORKFLOW
        path.parent.mkdir(parents=True)
        path.write_bytes(workflow)
        def git(*args):
            return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True).stdout.decode().strip()
        git("add", ".")
        git("-c", "user.name=Release fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "workflow fixture")
        revision = git("rev-parse", "HEAD")
        git("tag", "-f", "v0.1.0")
        git("update-ref", "refs/remotes/origin/main", revision)
        digest = hashlib.sha256()
        for name, data in sorted(self.fixture.source.items()):
            digest.update(name.encode() + b"\0" + data + b"\0")
        self.fixture.source_hash = digest.hexdigest()
        self.fixture.revision = revision
        self.fixture.identity.update(revision=revision, source_sha256=self.fixture.source_hash)
        self.identity = self.fixture.identity
        self.github = RecoveryGitHub(revision)
        self.fixture.github = self.github
        self.fixture.build_inputs()
        self.fingerprint = patch.object(service, "FINGERPRINT", hashlib.sha1(PUBLIC_CERTIFICATE).hexdigest().upper())
        self.fingerprint.start()
        self.addCleanup(self.fingerprint.stop)
        self.promoted = self.fixture.promoted()
        receipt_path = self.promoted / "promotion.metadata.json"
        receipt = json.loads(receipt_path.read_text())
        for index, record in enumerate(receipt["targets"].values()):
            for kind_index, kind in enumerate(("payload", "dmg")):
                record[kind]["submission_id"] = str(uuid.UUID(int=1 + index * 2 + kind_index))
        receipt_path.write_text(json.dumps(receipt))
        service.checksum_manifest(self.promoted)
        self.outer = self.root / "public.zip"
        self.make_outer()
        self.original_run = service.run
        self.calls = []

    def make_outer(self, mutate=None):
        with zipfile.ZipFile(self.outer, "w") as archive:
            for index, path in enumerate(sorted(self.promoted.iterdir())):
                member = zipfile.ZipInfo(path.name)
                member.external_attr = 0o100644 << 16
                if mutate and index == 0:
                    mutate(member)
                archive.writestr(member, path.read_bytes())
        self.github.artifacts[0].update(size_in_bytes=self.outer.stat().st_size,
                                        digest="sha256:" + service.sha256(self.outer))

    def environment(self, values=None):
        return fixtures.fixture_environment({"GITHUB_ACTIONS": "true", "GITHUB_REF": "refs/heads/main",
            "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REPOSITORY": self.github.repository,
            "GITHUB_SHA": self.identity["revision"], "GITHUB_RUN_ID": "99", "GH_TOKEN": TOKEN, **(values or {})})

    def recording(self, *args, **kwargs):
        args = tuple(str(argument) for argument in args)
        self.calls.append(args)
        if args[0] == "git":
            return self.original_run(*args, **kwargs)
        tool = Path(args[0]).name
        self.assertNotIn("--sign", args)
        self.assertNotIn("notarytool", args)
        self.assertNotEqual(tool, "security")
        self.assertNotIn(TOKEN, " ".join(args))
        if tool == "curl":
            if "--version" in args:
                return fixtures.completed(stdout=b"curl 8.7.1 fixture\n")
            self.assertEqual(args[1], "--disable")
            self.assertNotIn("--location-trusted", args)
            self.assertNotIn("--verbose", args)
            self.assertIn(b"Authorization: Bearer " + TOKEN.encode(), kwargs["input"])
            self.assertEqual(args[args.index("--proto") + 1], "=https")
            self.assertEqual(args[args.index("--proto-redir") + 1], "=https")
            self.assertEqual(int(args[args.index("--max-filesize") + 1]), self.outer.stat().st_size)
            Path(args[args.index("--output") + 1]).write_bytes(self.outer.read_bytes())
            return fixtures.completed()
        if tool == "codesign":
            path = Path(args[-1])
            if "--verbose=4" in args:
                identifier = service.APP_ID if path.name.endswith(".app") or path.name == service.BINARIES[1] and "Contents" in path.parts else \
                    service.APP_ID + (".gui" if path.name.endswith("-gui") else ".cli")
                return fixtures.completed(stderr=(f"Identifier={identifier}\nTeamIdentifier={service.TEAM}\n"
                    "CodeDirectory v=20500 size=128 flags=0x10000(runtime) hashes=1+1 location=embedded\n"
                    "Timestamp=Oct 14, 2026 at 5:00:00 AM\n").encode())
            prefix = next((argument.split("=", 1)[1] for argument in args if argument.startswith("--extract-certificates=")), None)
            if prefix:
                Path(prefix + "0").write_bytes(PUBLIC_CERTIFICATE)
            return fixtures.completed()
        if tool in {"xcrun", "spctl"}:
            self.assertNotIn("staple", args)
            return fixtures.completed()
        self.fail("Unexpected recovery subprocess")

    def proof(self):
        return recovery.verify_promotion_run(self.github, 77, self.repo)

    def test_repository_root_api_has_no_trailing_slash(self):
        with patch.object(service, "run", return_value=fixtures.completed(stdout=b'{}')) as run:
            service.GitHub(self.github.repository).api("")
        self.assertEqual(run.call_args.args, ("gh", "api", "repos/" + self.github.repository))

    def test_approved_cleaned_promotion_can_recover_only_credential_free_upload_failure(self):
        proof = self.proof()
        self.assertEqual(proof["promotion_run_id"], 77)
        self.assertTrue(proof["human_approval_verified"])
        self.assertNotIn(fixtures.SENTINEL, json.dumps(proof))
        self.github.promotion["conclusion"] = "success"
        upload = self.github.promotion_jobs[2]
        upload["conclusion"] = "success"
        upload["steps"][-1]["conclusion"] = "success"
        self.assertEqual(self.proof()["artifact_id"], 91)

    def test_untrusted_ref_repository_workflow_status_and_attempt_are_rejected(self):
        original = copy.deepcopy(self.github.promotion)
        for name, value in (("event", "pull_request"), ("head_branch", "feature"), ("path", "other.yml"),
                            ("workflow_id", 99), ("head_sha", "bad"), ("status", "in_progress"),
                            ("conclusion", "cancelled"), ("run_attempt", 2), ("run_attempt", True),
                            ("repository", None), ("head_repository", {"id": 8, "full_name": "other/project"})):
            with self.subTest(name=name, value=value):
                self.github.promotion = {**original, name: value}
                with self.assertRaises(service.ReleaseError):
                    self.proof()
        self.github.promotion = original

    def test_untrusted_main_ancestry_and_changed_workflow_are_rejected(self):
        with patch.object(service, "run", side_effect=service.ReleaseError("Fixture untrusted ancestry")), self.assertRaises(service.ReleaseError):
            self.proof()
        def changed(*args, **kwargs):
            if args[1:3] == ("show", "HEAD:" + recovery.PROMOTION_WORKFLOW):
                return fixtures.completed(stdout=b"changed contract")
            return self.original_run(*args, **kwargs)
        with patch.object(service, "run", side_effect=changed), self.assertRaisesRegex(service.ReleaseError, "reviewed recovery contract"):
            self.proof()

    def test_missing_ambiguous_partial_sign_cleanup_and_extra_upload_failure_are_rejected(self):
        original = copy.deepcopy(self.github.promotion_jobs)
        mutations = (lambda jobs: jobs.pop(), lambda jobs: jobs.append(copy.deepcopy(jobs[1])),
                     lambda jobs: jobs[1].update(conclusion="failure"),
                     lambda jobs: jobs[1]["steps"][2].update(conclusion="failure"),
                     lambda jobs: jobs[1]["steps"][3].update(conclusion="skipped"),
                     lambda jobs: jobs[1]["steps"][0].update(number=5),
                     lambda jobs: jobs[1].update(run_attempt=2),
                     lambda jobs: jobs[2]["steps"].append({"name": "Unrelated failure", "status": "completed", "conclusion": "failure"}),
                     lambda jobs: jobs[2]["steps"][0].update(conclusion="failure"))
        for mutate in mutations:
            self.github.promotion_jobs = copy.deepcopy(original)
            mutate(self.github.promotion_jobs)
            with self.assertRaises(service.ReleaseError):
                self.proof()
        self.github.promotion_jobs = original

    def test_missing_wrong_reviewer_rejected_and_malformed_review_history_are_rejected(self):
        good = copy.deepcopy(self.github.reviews)
        for value in ([], {}, [None], [{**good[0], "state": "rejected"}],
                      [{**good[0], "user": {"login": "other-reviewer"}}], [{**good[0], "user": None}],
                      [{**good[0], "environments": [{"name": "other", "id": 23}]}]):
            self.github.reviews = value
            with self.assertRaises(service.ReleaseError):
                self.proof()

    def test_missing_duplicate_expired_digest_origin_and_old_attempt_artifacts_are_rejected(self):
        original = copy.deepcopy(self.github.artifacts)
        for mutate in (lambda items: items.clear(), lambda items: items.append(copy.deepcopy(items[0])),
                       lambda items: items[0].update(expired=True), lambda items: items[0].update(digest="missing"),
                       lambda items: items[0].update(expires_at="2000-01-01T00:00:00Z"),
                       lambda items: items[0].update(created_at="2000-01-01T00:00:00Z"),
                       lambda items: items[0].update(workflow_run=None),
                       lambda items: items[0]["workflow_run"].update(head_repository_id=8),
                       lambda items: items[0].update(size_in_bytes=service.MAX_BYTES + 1)):
            self.github.artifacts = copy.deepcopy(original)
            mutate(self.github.artifacts)
            with self.assertRaises(service.ReleaseError):
                self.proof()

    def test_outer_digest_verified_before_private_flat_extraction(self):
        destination = self.root / "downloaded"
        with self.environment(), patch.object(service, "run", side_effect=self.recording):
            recovery.download_public_promotion(self.github, self.proof(), destination, self.identity)
        self.assertEqual({path.name for path in destination.iterdir()}, recovery.promoted_names(self.identity))
        proof = self.proof()
        proof["artifact_sha256"] = "0" * 64
        failed = self.root / "digest-failed"
        with self.environment(), patch.object(service, "run", side_effect=self.recording), self.assertRaisesRegex(service.ReleaseError, "digest"):
            recovery.download_public_promotion(self.github, proof, failed, self.identity)
        self.assertFalse(failed.exists())

    def test_malicious_outer_entries_are_rejected_without_partial_destination(self):
        for index, mutate in enumerate((lambda member: setattr(member, "filename", "../escape"),
                                        lambda member: setattr(member, "external_attr", 0o120777 << 16),
                                        lambda member: setattr(member, "external_attr", 0o104755 << 16),
                                        lambda member: setattr(member, "filename", "secret.p8"))):
            self.make_outer(mutate)
            destination = self.root / ("unsafe-" + str(index))
            with self.environment(), patch.object(service, "run", side_effect=self.recording), self.assertRaises(service.ReleaseError):
                recovery.download_public_promotion(self.github, self.proof(), destination, self.identity)
            self.assertFalse(destination.exists())
        self.assertFalse((self.root / "escape").exists())

    def test_download_failure_cleanup_and_inadequate_curl_bound_are_enforced(self):
        paths = []
        def failed(*args, **kwargs):
            if "--version" in args:
                return self.recording(*args, **kwargs)
            path = Path(args[args.index("--output") + 1])
            path.write_bytes(b"partial")
            paths.append(path)
            raise service.ReleaseError("Fixture partial download failed")
        with self.environment(), patch.object(service, "run", side_effect=failed), self.assertRaises(service.ReleaseError):
            recovery.download_public_promotion(self.github, {"artifact_id": 91, "artifact_size": 1, "artifact_sha256": "a" * 64},
                                               self.root / "failed-download", self.identity)
        self.assertTrue(paths)
        self.assertTrue(all(not path.exists() for path in paths))
        with self.environment(), patch.object(service, "run", return_value=fixtures.completed(stdout=b"curl 8.3.0\n")), self.assertRaisesRegex(service.ReleaseError, "curl 8.4"):
            recovery.download_public_promotion(self.github, {"artifact_id": 91, "artifact_size": 1, "artifact_sha256": "a" * 64},
                                               self.root / "old-curl", self.identity)

    def test_bearer_punctuation_and_jwt_tokens_use_stdin_only_without_config_injection(self):
        proof = self.proof()
        tokens = ("ghs_scoped-fixture-token/with+valid.punctuation~",
                  "eyJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJmaXh0dXJlIn0.signature-_", "token.with~valid+/padding==", "x" * 8192)
        for index, token in enumerate(tokens):
            calls = []
            def recording(*args, **kwargs):
                arguments = tuple(str(value) for value in args)
                calls.append(arguments)
                self.assertNotIn(token, " ".join(arguments))
                if "--version" in arguments:
                    return fixtures.completed(stdout=b"curl 8.7.1 fixture\n")
                self.assertEqual(kwargs["input"], ('header = "Authorization: Bearer ' + token
                    + '"\nheader = "Accept: application/vnd.github+json"\n').encode())
                self.assertNotIn("--location-trusted", arguments)
                self.assertNotIn("--verbose", arguments)
                Path(arguments[arguments.index("--output") + 1]).write_bytes(self.outer.read_bytes())
                return fixtures.completed()
            destination = self.root / ("punctuated-token-" + str(index))
            with self.environment({"GH_TOKEN": token}), patch.object(service, "run", side_effect=recording):
                recovery.download_public_promotion(self.github, proof, destination, self.identity)
            self.assertEqual(len(calls), 2)
            self.assertEqual({path.name for path in destination.iterdir()}, recovery.promoted_names(self.identity))

    def test_missing_oversized_and_config_injection_bearer_tokens_fail_before_curl_without_value_leaks(self):
        proof = self.proof()
        tokens = ("", "x" * 8193, "token\x00", "token\x01", "token\t", "token\n", "token\r", 'token"',
                  "token\\", "token with space", "token=interior", "=padding-only", "token:colon", "token\u00e9",
                  'fixture\"\nheader = \"Authorization: injected', 'fixture\nurl = \"https://example.invalid/\"')
        for index, token in enumerate(tokens):
            destination = self.root / ("invalid-token-" + str(index))
            # A plain fixture mapping also covers NUL, which real OS environments reject.
            with patch.object(recovery.os, "environ", {"GH_TOKEN": token}), patch.object(service, "run") as run:
                with self.assertRaises(service.ReleaseError) as caught:
                    recovery.download_public_promotion(self.github, proof, destination, self.identity)
            self.assertEqual(str(caught.exception), "Scoped GitHub upload token is missing or malformed")
            if token:
                self.assertNotIn(token, str(caught.exception))
            run.assert_not_called()
            self.assertFalse(destination.exists())
        with self.environment(), patch.dict(os.environ):
            os.environ.pop("GH_TOKEN", None)
            with patch.object(service, "run") as run, self.assertRaisesRegex(service.ReleaseError, "missing or malformed"):
                recovery.download_public_promotion(self.github, proof, self.root / "missing-token", self.identity)
            run.assert_not_called()

    def test_native_signature_staple_and_leaf_checks_cover_both_frontends_without_signing(self):
        with patch.object(recovery.platform, "system", return_value="Darwin"), patch.object(service, "run", side_effect=self.recording):
            checks = recovery.verify_public_macos(self.promoted, self.identity)
        self.assertEqual(checks["signature_checks"], 12)
        self.assertEqual(checks["accepted_notarization_receipts"], 4)
        self.assertEqual(sum("--extract-certificates=" in " ".join(args) for args in self.calls), 12)
        self.assertEqual(sum("stapler" in args for args in self.calls), 4)
        self.assertTrue(all(Path(args[0]).name in {"codesign", "xcrun", "spctl"} for args in self.calls))
        self.assertTrue(all("--sign" not in args and "notarytool" not in args for args in self.calls))
        self.assertFalse(checks["package_binaries_executed"])

    def test_wrong_main_gui_identifier_runtime_team_timestamp_and_leaf_are_rejected(self):
        for substitute in (lambda data: data.replace(service.APP_ID.encode(), b"wrong.bundle"),
                           lambda data: data.replace(b"runtime", b"library"),
                           lambda data: data.replace(service.TEAM.encode(), b"wrong-team"),
                           lambda data: data.replace(b"Timestamp=Oct 14, 2026 at 5:00:00 AM", b"Timestamp=none")):
            def tampered(*args, **kwargs):
                result = self.recording(*args, **kwargs)
                if "--verbose=4" in args:
                    return fixtures.completed(stderr=substitute(result.stderr))
                return result
            with patch.object(recovery.platform, "system", return_value="Darwin"), patch.object(service, "run", side_effect=tampered), self.assertRaises(service.ReleaseError):
                recovery.verify_public_macos(self.promoted, self.identity)
        def wrong_leaf(*args, **kwargs):
            result = self.recording(*args, **kwargs)
            for argument in args:
                if str(argument).startswith("--extract-certificates="):
                    Path(str(argument).split("=", 1)[1] + "0").write_bytes(b"wrong public certificate")
            return result
        with patch.object(recovery.platform, "system", return_value="Darwin"), patch.object(service, "run", side_effect=wrong_leaf), self.assertRaisesRegex(service.ReleaseError, "leaf"):
            recovery.verify_public_macos(self.promoted, self.identity)

    def test_shared_final_gate_rejects_unaccepted_receipts_failed_cleanup_and_wrong_source(self):
        path = self.promoted / "promotion.metadata.json"
        original = json.loads(path.read_text())
        for mutate in (lambda value: value.update(credential_cleanup="failed"),
                       lambda value: value.update(source_sha256="0" * 64),
                       lambda value: value["targets"][service.TARGETS[0]]["payload"].update(status="In Progress"),
                       lambda value: value["targets"][service.TARGETS[0]].update(app_stapled=False)):
            value = copy.deepcopy(original)
            mutate(value)
            path.write_text(json.dumps(value))
            service.checksum_manifest(self.promoted)
            with self.assertRaises(service.ReleaseError):
                recovery.verify_public_macos(self.promoted, self.identity)
        path.write_text(json.dumps(original))
        service.checksum_manifest(self.promoted)
        original["targets"][service.TARGETS[1]]["dmg"]["submission_id"] = original["targets"][service.TARGETS[0]]["dmg"]["submission_id"]
        path.write_text(json.dumps(original))
        service.checksum_manifest(self.promoted)
        with patch.object(recovery.platform, "system", return_value="Darwin"), self.assertRaisesRegex(service.ReleaseError, "distinct"):
            recovery.verify_public_macos(self.promoted, self.identity)

    def test_recovery_refuses_apple_credentials_other_refs_and_published_releases(self):
        for values in ({"MACOS_CERTIFICATE_PASSWORD": fixtures.SENTINEL}, {"GITHUB_REF": "refs/tags/v0.1.0"},
                       {"GITHUB_EVENT_NAME": "pull_request"}, {"GITHUB_REPOSITORY": "other/repo"}):
            with self.environment(values), self.assertRaises(service.ReleaseError) as caught:
                recovery.recover_upload(self.github, "v0.1.0", 123, 77, self.root / "blocked.json", cwd=self.repo)
            self.assertNotIn(fixtures.SENTINEL, str(caught.exception))
        self.github.existing = {"id": 44, "draft": False}
        with self.environment(), self.assertRaisesRegex(service.ReleaseError, "never"):
            recovery.recover_upload(self.github, "v0.1.0", 123, 77, self.root / "published.json", cwd=self.repo)

    def test_recovered_default_draft_retains_bounded_public_proof_and_exact_body_without_credentials(self):
        self.github.existing = {"id": 44, "draft": True, "prerelease": False, "assets": []}
        upload_calls = []
        uploader = self.fixture.fixture_upload(None, upload_calls)
        def upload(directory, identity, github, publish=False, cwd=service.ROOT):
            nonlocal uploader
            uploader = self.fixture.fixture_upload(directory, upload_calls)
            with patch.object(service, "run", side_effect=uploader):
                return original_upload(directory, identity, github, publish, cwd)
        original_upload = service.upload_draft
        def fetch(_github, _run_id, destination):
            import shutil
            shutil.copytree(self.fixture.inputs, destination)
        record_path = self.root / "recovery.json"
        with self.environment(), patch.object(service, "fetch_artifacts", side_effect=fetch), \
                patch.object(service, "run", side_effect=self.recording), patch.object(service, "upload_draft", side_effect=upload), \
                patch.object(recovery.platform, "system", return_value="Darwin"):
            record = recovery.recover_upload(self.github, "v0.1.0", 123, 77, record_path, cwd=self.repo)
        self.assertTrue(record["draft"])
        self.assertFalse(record["publish"])
        self.assertFalse(record["prerelease"])
        self.assertEqual(record["release_id"], 44)
        self.assertEqual(record["recovery_run_id"], 99)
        self.assertEqual(record["promotion_run_id"], 77)
        self.assertEqual(len(record["asset_checksums"]), 14)
        self.assertTrue(record["download_verification_succeeded"])
        self.assertEqual(record["body_sha256"], hashlib.sha256(service.release_notes(self.identity, self.repo).encode()).hexdigest())
        self.assertLess(record_path.stat().st_size, 65536)
        self.assertNotIn(TOKEN, record_path.read_text())
        self.assertNotIn(fixtures.SENTINEL, record_path.read_text())
        self.assertFalse(any("--draft=false" in arguments for arguments in upload_calls))
        self.assertEqual(sum("upload" in arguments for arguments in upload_calls), 15)

    def test_interrupted_upload_leaves_no_recovery_record_or_temporary_payload_and_never_publishes(self):
        self.github.existing = {"id": 44, "draft": True, "prerelease": False, "assets": []}
        upload_calls, temporary_inputs = [], []
        original_upload = service.upload_draft
        def upload(directory, identity, github, publish=False, cwd=service.ROOT):
            uploader = self.fixture.fixture_upload(directory, upload_calls, fail_upload=True)
            with patch.object(service, "run", side_effect=uploader):
                return original_upload(directory, identity, github, publish, cwd)
        def fetch(_github, _run_id, destination):
            import shutil
            temporary_inputs.append(destination)
            shutil.copytree(self.fixture.inputs, destination)
        record_path = self.root / "interrupted.json"
        with self.environment(), patch.object(service, "fetch_artifacts", side_effect=fetch), \
                patch.object(service, "run", side_effect=self.recording), patch.object(service, "upload_draft", side_effect=upload), \
                patch.object(recovery.platform, "system", return_value="Darwin"), self.assertRaises(service.ReleaseError):
            recovery.recover_upload(self.github, "v0.1.0", 123, 77, record_path, cwd=self.repo)
        self.assertFalse(record_path.exists())
        self.assertTrue(temporary_inputs)
        self.assertTrue(all(not path.exists() for path in temporary_inputs))
        self.assertTrue(self.github.existing["draft"])
        self.assertFalse(any("--draft=false" in args for args in upload_calls))
        self.assertEqual(sum("upload" in args for args in upload_calls), 1)


if __name__ == "__main__":
    unittest.main()

"""Recover uploads only from an approved, cleaned and verified public promotion."""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import tempfile
import zipfile

import release_service as service

PROMOTION_WORKFLOW = ".github/workflows/release.yml"
PROMOTED_ARTIFACT = "notarized-macos-release"
REVIEWER = "batuhanoktem"
JOBS = (
    "Validate exact tag, five-platform CI and corresponding source",
    "Approve, sign and notarize both tested Mac architectures",
    "Prepare verified draft or explicitly publish",
)
VALIDATE_STEPS = (
    "Run credential-free release safety fixtures",
    "Download tested artifacts and verify every source and native hash",
    "Retain verified public release identity for review",
)
SIGN_STEPS = (
    "Revalidate unsigned artifacts before exposing Apple credentials",
    "Promote tested packages without building or executing their binaries",
    "Remove temporary credentials even after failure or cancellation",
    "Upload only public promoted assets after successful credential cleanup",
)
UPLOAD_FETCH_STEP = "Revalidate retained Windows, Linux and source artifacts without Apple credentials"
UPLOAD_STEP = "Upload, download and verify the final exact asset set"
APPLE_SECRETS = ("MACOS_CERTIFICATE_P12_BASE64", "MACOS_CERTIFICATE_PASSWORD", "APPLE_NOTARY_KEY_P8_BASE64")


def positive_integer(value):
    return type(value) is int and value > 0


def revision(value):
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{40}", value) is not None


def timestamp(value):
    service.require(isinstance(value, str) and re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z", value),
                    "Promotion timestamp is malformed")
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        raise service.ReleaseError("Promotion timestamp is malformed") from None


def paginated(github, suffix, key):
    values = []
    for page in range(1, 11):
        separator = "&" if "?" in suffix else "?"
        response = github.api(f"{suffix}{separator}per_page=100&page={page}")
        batch = response.get(key) if isinstance(response, dict) else None
        service.require(isinstance(batch, list) and len(batch) <= 100 and all(isinstance(item, dict) for item in batch),
                        "Promotion API inventory is malformed")
        values.extend(batch)
        if len(batch) < 100:
            return values
    raise service.ReleaseError("Promotion API inventory exceeds its limit")


def successful_steps(job, names):
    steps = job.get("steps")
    service.require(isinstance(steps, list) and len(steps) <= 1000 and all(isinstance(step, dict) for step in steps),
                    "Promotion job steps are missing or malformed")
    positions = []
    for name in names:
        matches = [step for step in steps if isinstance(step, dict) and step.get("name") == name]
        service.require(len(matches) == 1 and matches[0].get("status") == "completed"
                        and matches[0].get("conclusion") == "success" and positive_integer(matches[0].get("number")),
                        "Promotion validation, signing, cleanup or public upload did not succeed")
        positions.append(matches[0]["number"])
    service.require(positions == sorted(set(positions)), "Promotion safety steps ran in an unexpected order")


def verify_promotion_run(github, run_id, cwd=service.ROOT, now=None):
    service.require(str(run_id).isdigit() and int(run_id) > 0, "Promotion run ID must be numeric")
    run_id = int(run_id)
    info = github.api(f"actions/runs/{run_id}")
    workflow = github.api("actions/workflows/release.yml")
    repository = github.api("")
    service.require(isinstance(info, dict) and isinstance(workflow, dict) and isinstance(repository, dict),
                    "Promotion origin metadata is malformed")
    repository_id = repository.get("id")
    service.require(positive_integer(repository_id) and repository.get("full_name") == github.repository
                    and isinstance(info.get("repository"), dict) and isinstance(info.get("head_repository"), dict)
                    and info.get("repository", {}).get("id") == repository_id
                    and info.get("repository", {}).get("full_name") == github.repository
                    and info.get("head_repository", {}).get("id") == repository_id
                    and info.get("head_repository", {}).get("full_name") == github.repository,
                    "Promotion origin is not this repository")
    service.require(info.get("id") == run_id and info.get("event") == "workflow_dispatch"
                    and info.get("head_branch") == "main" and info.get("path") == PROMOTION_WORKFLOW
                    and info.get("workflow_id") == workflow.get("id") and positive_integer(workflow.get("id"))
                    and workflow.get("path") == PROMOTION_WORKFLOW and type(info.get("run_attempt")) is int and info["run_attempt"] == 1
                    and revision(info.get("head_sha")) and info.get("status") == "completed"
                    and isinstance(info.get("conclusion"), str) and info["conclusion"] in {"success", "failure"},
                    "Promotion must be a completed, unambiguous main dispatch of the official workflow")
    source_revision = info["head_sha"]
    service.run("git", "merge-base", "--is-ancestor", source_revision, "origin/main", cwd=cwd)
    original_workflow = service.run("git", "show", f"{source_revision}:{PROMOTION_WORKFLOW}", cwd=cwd).stdout
    reviewed_workflow = service.run("git", "show", f"HEAD:{PROMOTION_WORKFLOW}", cwd=cwd).stdout
    service.require(original_workflow == reviewed_workflow, "Promotion workflow differs from the reviewed recovery contract")
    jobs = paginated(github, f"actions/runs/{run_id}/jobs?filter=latest", "jobs")
    service.require(len(jobs) == 3 and {job.get("name") for job in jobs} == set(JOBS)
                    and all(job.get("run_id") == run_id and type(job.get("run_attempt")) is int and job["run_attempt"] == 1
                            and job.get("head_sha") == source_revision and job.get("status") == "completed" for job in jobs),
                    "Promotion job origin or inventory is incomplete or ambiguous")
    records = {job["name"]: job for job in jobs}
    service.require(all(records[name].get("conclusion") == "success" for name in JOBS[:2]),
                    "Promotion validation and signing must succeed before upload recovery")
    successful_steps(records[JOBS[0]], VALIDATE_STEPS)
    successful_steps(records[JOBS[1]], SIGN_STEPS)
    upload = records[JOBS[2]]
    successful_steps(upload, (UPLOAD_FETCH_STEP,))
    if info["conclusion"] == "success":
        service.require(upload.get("conclusion") == "success", "Successful promotion has an unsuccessful upload job")
        successful_steps(upload, (UPLOAD_STEP,))
    else:
        failures = [step for step in upload.get("steps", []) if isinstance(step, dict) and step.get("conclusion") == "failure"]
        service.require(upload.get("conclusion") == "failure" and len(failures) == 1
                        and failures[0].get("name") == UPLOAD_STEP and failures[0].get("status") == "completed"
                        and all(step.get("conclusion") in {"success", "skipped", "failure"} for step in upload.get("steps", []) if isinstance(step, dict)),
                        "Only the credential-free final upload may have failed")
    reviews = github.api(f"actions/runs/{run_id}/approvals")
    service.require(isinstance(reviews, list) and 0 < len(reviews) <= 1000 and all(isinstance(review, dict) for review in reviews),
                    "Promotion environment review history is unavailable or malformed")
    relevant = [review for review in reviews if isinstance(review.get("environments"), list)
                and any(isinstance(environment, dict) and environment.get("name") == "macos-release"
                        and positive_integer(environment.get("id")) for environment in review["environments"])]
    service.require(any(review.get("state") == "approved" and isinstance(review.get("user"), dict)
                        and review["user"].get("login") == REVIEWER for review in relevant)
                    and not any(review.get("state") == "rejected" for review in relevant),
                    "Promotion lacks the required human approval")
    artifacts = paginated(github, f"actions/runs/{run_id}/artifacts", "artifacts")
    selected = [item for item in artifacts if item.get("name") == PROMOTED_ARTIFACT]
    service.require(len(selected) == 1, "Public promotion artifact is missing or ambiguous")
    artifact = selected[0]
    origin = artifact.get("workflow_run")
    now = datetime.now(timezone.utc) if now is None else now
    sign = records[JOBS[1]]
    service.require(isinstance(origin, dict) and positive_integer(artifact.get("id")) and artifact.get("expired") is False
                    and positive_integer(artifact.get("size_in_bytes")) and artifact["size_in_bytes"] <= service.MAX_BYTES
                    and isinstance(artifact.get("digest"), str) and re.fullmatch(r"sha256:[a-f0-9]{64}", artifact["digest"])
                    and origin.get("id") == run_id and origin.get("repository_id") == repository_id
                    and origin.get("head_repository_id") == repository_id and origin.get("head_branch") == "main"
                    and origin.get("head_sha") == source_revision and timestamp(artifact.get("expires_at")) > now
                    and timestamp(sign.get("started_at")) <= timestamp(artifact.get("created_at")) <= timestamp(sign.get("completed_at")),
                    "Public promotion artifact is expired, oversized or has an untrusted origin")
    return {"promotion_run_id": run_id, "promotion_workflow_revision": source_revision,
            "artifact_id": artifact["id"], "artifact_sha256": artifact["digest"][7:],
            "artifact_size": artifact["size_in_bytes"], "human_approval_verified": True,
            "credential_cleanup_and_public_upload_verified": True}


def promoted_names(identity):
    expected = {"promotion.metadata.json", "SHA256SUMS"}
    for target in service.TARGETS[:2]:
        root = f"c64-keyboard-bridge-{identity['version']}-{target}"
        expected.update(root + suffix for suffix in (".tar.gz", ".dmg", ".metadata.json"))
    return expected


def download_public_promotion(github, proof, destination, identity):
    destination = Path(destination)
    service.require(not destination.exists(), "Public promotion destination must be new")
    service.require(positive_integer(proof.get("artifact_id")) and positive_integer(proof.get("artifact_size"))
                    and proof["artifact_size"] <= service.MAX_BYTES and isinstance(proof.get("artifact_sha256"), str)
                    and re.fullmatch(r"[a-f0-9]{64}", proof["artifact_sha256"]), "Public promotion download identity is malformed")
    token = os.environ.get("GH_TOKEN", "")
    # RFC 6750 section 2.1 permits bearer punctuation; curl configuration syntax stays excluded.
    service.require(0 < len(token) <= 8192 and re.fullmatch(r"[A-Za-z0-9._~+/-]+=*", token) is not None,
                    "Scoped GitHub upload token is missing or malformed")
    version = service.run("curl", "--disable", "--version").stdout
    match = re.match(rb"curl ([0-9]+)\.([0-9]+)\.([0-9]+)", version)
    service.require(match is not None and tuple(int(part) for part in match.groups()) >= (8, 4, 0),
                    "Upload recovery needs curl 8.4 or later for bounded downloads")
    with tempfile.TemporaryDirectory(prefix="release-public-download-") as temporary:
        outer = Path(temporary) / "public-promotion.zip"
        config = ('header = "Authorization: Bearer ' + token + '"\nheader = "Accept: application/vnd.github+json"\n').encode()
        # curl strips Authorization on a cross-origin redirect. Never use location-trusted.
        service.run("curl", "--disable", "--config", "-", "--fail", "--silent", "--show-error", "--location",
                    "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "3", "--max-time", "1200",
                    "--max-filesize", str(proof["artifact_size"]), "--output", outer,
                    f"https://api.github.com/repos/{github.repository}/actions/artifacts/{proof['artifact_id']}/zip",
                    input=config, timeout=1210)
        service.require(outer.is_file() and not outer.is_symlink() and outer.stat().st_size == proof["artifact_size"]
                        and service.sha256(outer) == proof["artifact_sha256"], "Public promotion outer artifact digest or size differs")
        try:
            with zipfile.ZipFile(outer) as archive:
                members = archive.infolist()
                expected = promoted_names(identity)
                service.require(len(members) == len(expected) and {member.filename for member in members} == expected,
                                "Public promotion archive has an unexpected file inventory")
                total = 0
                for member in members:
                    mode = member.external_attr >> 16
                    total += member.file_size
                    service.require(not member.is_dir() and stat.S_IFMT(mode) in {0, stat.S_IFREG}
                                    and not mode & 0o7000 and not member.flag_bits & 1
                                    and 0 < member.file_size <= 512 * 1024**2 and total <= service.MAX_BYTES,
                                    "Public promotion archive has unsafe types, modes or sizes")
                    if member.filename.endswith(".metadata.json"):
                        service.require(member.file_size <= 65536, "Public promotion evidence exceeds its limit")
                stage = Path(temporary) / "files"
                stage.mkdir(mode=0o700)
                for member in members:
                    with archive.open(member) as source, (stage / member.filename).open("xb") as output:
                        shutil.copyfileobj(source, output, 1024 * 1024)
                service.verify_final(stage, identity)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(stage, destination)
        except (OSError, zipfile.BadZipFile, RuntimeError):
            raise service.ReleaseError("Public promotion archive is malformed or unreadable") from None
    return destination


def signature(path, identifier, certificates, hardened):
    service.run("/usr/bin/codesign", "--verify", "--deep", "--strict", path)
    evidence = service.run("/usr/bin/codesign", "--display", "--verbose=4", path).stderr.decode(errors="replace")
    flags = re.search(r"(?:^|\s)flags=0x[0-9a-fA-F]+\(([^)]*)\)", evidence)
    stamp = re.search(r"^Timestamp=(.+)$", evidence, re.MULTILINE)
    service.require(f"TeamIdentifier={service.TEAM}\n" in evidence
                    and (identifier is None or f"Identifier={identifier}\n" in evidence)
                    and stamp is not None and stamp[1].strip().lower() not in {"none", "not set", ""}
                    and (not hardened or flags is not None and "runtime" in flags[1].split(",")),
                    "Public Mac signature identity, timestamp or hardened runtime is incorrect")
    prefix = certificates / "leaf-"
    service.run("/usr/bin/codesign", "--display", "--extract-certificates=" + str(prefix), path)
    leaf = certificates / "leaf-0"
    service.require(leaf.is_file() and not leaf.is_symlink() and 0 < leaf.stat().st_size <= 1024 * 1024
                    and hashlib.sha1(leaf.read_bytes()).hexdigest().upper() == service.FINGERPRINT,
                    "Public Mac signing leaf certificate differs from the approved identity")
    for certificate in certificates.iterdir():
        service.require(certificate.is_file() and not certificate.is_symlink(), "Public signature extraction produced unexpected entries")
        certificate.unlink()


def verify_public_macos(directory, identity):
    service.require(platform.system() == "Darwin", "Public Mac signature verification needs a fresh Mac runner")
    service.verify_final(directory, identity)
    receipt = json.loads((Path(directory) / "promotion.metadata.json").read_text())
    submissions = {record[kind]["submission_id"] for record in receipt["targets"].values() for kind in ("payload", "dmg")}
    service.require(len(submissions) == 4, "Four distinct accepted notarization receipts are required")
    signature_count = 0
    with tempfile.TemporaryDirectory(prefix="release-public-verification-") as temporary:
        stage = Path(temporary)
        certificates = stage / "certificates"
        certificates.mkdir(mode=0o700)
        for target in service.TARGETS[:2]:
            root = f"c64-keyboard-bridge-{identity['version']}-{target}"
            payload = service.extract_archive(Path(directory) / (root + ".tar.gz"), stage / target, root)
            app = payload / service.APP
            for name in service.BINARIES:
                identifier = service.APP_ID + (".gui" if name.endswith("-gui") else ".cli")
                signature(payload / name, identifier, certificates, True)
                signature_count += 1
                signature(app / "Contents/MacOS" / name,
                          service.APP_ID if name == service.BINARIES[1] else identifier, certificates, True)
                signature_count += 1
            signature(app, service.APP_ID, certificates, True)
            signature_count += 1
            service.run("xcrun", "stapler", "validate", app)
            service.run("/usr/sbin/spctl", "--assess", "--type", "execute", app)
            dmg = Path(directory) / (root + ".dmg")
            signature(dmg, None, certificates, False)
            signature_count += 1
            service.run("xcrun", "stapler", "validate", dmg)
            service.run("/usr/sbin/spctl", "--assess", "--type", "open", "--context", "context:primary-signature", dmg)
    return {"architectures": 2, "signature_checks": signature_count, "accepted_notarization_receipts": 4,
            "stapled_apps": 2, "stapled_dmgs": 2, "certificate_sha1": service.FINGERPRINT,
            "team": service.TEAM, "package_binaries_executed": False}


def recover_upload(github, tag, ci_run_id, promotion_run_id, record_path, publish=False, cwd=service.ROOT):
    service.require(not any(os.environ.get(name) for name in APPLE_SECRETS),
                    "Upload recovery must run without Apple credentials")
    service.require(os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("GITHUB_REF") == "refs/heads/main"
                    and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
                    and os.environ.get("GITHUB_REPOSITORY") == github.repository
                    and revision(os.environ.get("GITHUB_SHA")) and os.environ.get("GITHUB_RUN_ID", "").isdigit()
                    and int(os.environ["GITHUB_RUN_ID"]) > 0, "Upload recovery must be manually dispatched from main")
    identity = service.resolve_release(tag, ci_run_id, github, cwd)
    proof = verify_promotion_run(github, promotion_run_id, cwd)
    with tempfile.TemporaryDirectory(prefix="release-upload-recovery-") as temporary:
        stage = Path(temporary)
        inputs, promoted, final = stage / "unsigned", stage / "promoted", stage / "final"
        service.fetch_artifacts(github, ci_run_id, inputs)
        identity["source_sha256"] = service.verify_inputs(inputs, identity, cwd)
        download_public_promotion(github, proof, promoted, identity)
        native = verify_public_macos(promoted, identity)
        service.assemble_final(inputs, promoted, final, identity)
        result = service.upload_draft(final, identity, github, publish, cwd)
        release = github.release(identity["tag"])
        body = service.release_notes(identity, cwd)
        service.require(isinstance(release, dict) and positive_integer(release.get("id"))
                        and release.get("draft") is (not publish) and release.get("prerelease") is False
                        and release.get("body") == body,
                        "Recovered release state differs from the requested draft or publication")
        record = {"schema": 1, **identity, **proof, "recovery_run_id": int(os.environ["GITHUB_RUN_ID"]),
                  "recovery_workflow_revision": os.environ["GITHUB_SHA"], "release_id": release["id"],
                  "publish": bool(publish), "draft": result["draft"], "native_verification": native,
                  "prerelease": False, "checksum_manifest_sha256": service.sha256(final / "SHA256SUMS"),
                  "body_sha256": hashlib.sha256(body.encode()).hexdigest(),
                  "asset_checksums": service.verify_final(final, identity), "download_verification_succeeded": True}
        encoded = json.dumps(record, indent=2) + "\n"
        service.require(len(encoded.encode()) <= 65536, "Public upload recovery record exceeds its limit")
        record_path = Path(record_path)
        record_path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with record_path.open("x", encoding="utf-8", newline="") as output:
            output.write(encoded)
        record_path.chmod(0o600)
    return record

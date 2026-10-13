#!/usr/bin/env python3
"""Validate exact CI artifacts, promote macOS packages and prepare verified draft releases."""
import argparse
import json
import os
from pathlib import Path
import platform
import signal
import sys

from release_service import (GitHub, ReleaseError, ROOT, assemble_final, checksum_manifest,
                             cleanup_credentials, run, require, fetch_artifacts, promote_macos, resolve_release, resolve_development, signing_credentials,
                             verify_inputs, upload_draft)


def interrupted(_signal, _frame):
    raise ReleaseError("Release was cancelled; publication is blocked")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("fetch", "validate", "promote", "upload", "cleanup"))
    parser.add_argument("--repository", default="batuhanoktem/c64-keyboard-bridge")
    parser.add_argument("--tag")
    parser.add_argument("--development-revision", help="Full untagged commit for local signed-only development; never publishable")
    parser.add_argument("--ci-run")
    parser.add_argument("--inputs", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--identity", type=Path)
    parser.add_argument("--credentials-dir", type=Path)
    parser.add_argument("--notarize-profile", help="Existing local Keychain profile; never an Apple password")
    parser.add_argument("--signed-only", action="store_true", help="Local development only; blocked from public release")
    parser.add_argument("--publish", action="store_true", help="Explicitly publish only after full upload/download verification")
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    if args.command == "cleanup":
        if not args.credentials_dir:
            parser.error("cleanup requires --credentials-dir")
        cleanup_credentials(args.credentials_dir)
        print("Temporary signing credentials removed")
        return
    if not args.ci_run or not args.inputs:
        parser.error("release operations require --ci-run and --inputs")
    github = GitHub(args.repository)
    if args.development_revision:
        if args.command not in {"fetch", "validate", "promote"} or not args.signed_only or args.tag or args.publish:
            parser.error("--development-revision is limited to local signed-only promotion")
        identity = resolve_development(args.development_revision, args.ci_run, github)
    else:
        if not args.tag:
            parser.error("release operations require an existing --tag")
        identity = resolve_release(args.tag, args.ci_run, github)
    if args.command == "fetch":
        fetch_artifacts(github, args.ci_run, args.inputs)
    if args.command in {"fetch", "validate"}:
        identity["source_sha256"] = verify_inputs(args.inputs, identity)
        if args.identity:
            args.identity.parent.mkdir(parents=True, exist_ok=True)
            args.identity.write_text(json.dumps(identity, indent=2) + "\n")
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                output.write("revision=" + identity["revision"] + "\n")
        print("Exact tag, successful five-platform CI, source and all artifact hashes verified")
    elif args.command == "promote":
        if platform.system() != "Darwin" or not args.output:
            parser.error("promotion requires a Mac and --output")
        identity["source_sha256"] = verify_inputs(args.inputs, identity)
        if os.environ.get("GITHUB_ACTIONS"):
            if args.notarize_profile or args.signed_only or not args.credentials_dir:
                parser.error("protected CI promotion requires temporary credentials and notarization")
            with signing_credentials(args.credentials_dir) as credentials:
                promote_macos(args.inputs, args.output, identity, credentials)
        else:
            if not args.notarize_profile and not args.signed_only:
                parser.error("local promotion requires --notarize-profile or explicit --signed-only")
            promote_macos(args.inputs, args.output, identity, profile=args.notarize_profile,
                          signed_only=args.signed_only)
        receipt_path = args.output / "promotion.metadata.json"
        receipt = json.loads(receipt_path.read_text())
        receipt["credential_cleanup"] = "succeeded"
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
        checksum_manifest(args.output)
        print("Mac artifacts promoted; credentials cleaned; no release was published")
    elif args.command == "upload":
        if not args.output:
            parser.error("upload requires --output containing promoted Mac artifacts")
        # Revalidate the original CI source and retained platform assets in the credential-free job.
        identity["source_sha256"] = verify_inputs(args.inputs, identity)
        final = args.output.parent / "final-release-assets"
        assemble_final(args.inputs, args.output, final, identity)
        result = upload_draft(final, identity, github, args.publish)
        print("Published verified release" if not result["draft"] else "Verified release prepared as a draft")


if __name__ == "__main__":
    try:
        main()
    except ReleaseError as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        # Never print raw exceptions: subprocess, Apple and file errors may contain secrets or paths.
        print("Release operation failed or was cancelled. No automatic publication retry; inspect the documented gates.", file=sys.stderr)
        sys.exit(1)

#!/usr/bin/env python3
"""Upload approved local files to protected GitHub environment secrets without logging values."""
import argparse
import base64
import getpass
import os
from pathlib import Path
import stat
import sys

from release_service import FINGERPRINT, TEAM, GitHub, ReleaseError, require, run


def private_file(path, maximum):
    path = Path(path).expanduser()
    require(not path.is_symlink() and path.is_file(), "Credential file must be a regular private file")
    info = path.stat()
    require(info.st_size <= maximum and info.st_size > 0, "Credential file is empty or oversized")
    if os.name != "nt":
        require(info.st_uid == os.getuid() and not stat.S_IMODE(info.st_mode) & 0o077,
                "Credential file must be owned by this user with permissions 0600")
    return path.read_bytes()


def provision(repository, environment, certificate, password, api_key, key_id, issuer):
    require(environment == "macos-release", "Only the protected macos-release environment is supported")
    github = GitHub(repository)
    protection = github.api("environments/macos-release")
    rules = protection.get("protection_rules", [])
    review = next((rule for rule in rules if rule.get("type") == "required_reviewers"), {})
    require(any(item.get("reviewer", {}).get("login") == "batuhanoktem" for item in review.get("reviewers", []))
            and review.get("prevent_self_review") is False and protection.get("can_admins_bypass") is False,
            "Expected reviewer protection is absent; no secrets were uploaded")
    policy = protection.get("deployment_branch_policy", {})
    require(policy.get("custom_branch_policies") is True, "Environment must restrict permitted refs")
    refs = github.api("environments/macos-release/deployment-branch-policies")["branch_policies"]
    require(len(refs) == 1 and refs[0].get("name") == "main" and refs[0].get("type") == "branch",
            "Environment must allow only the main branch")
    # Values are piped to gh, whose encryption happens locally; no value appears in argv or output.
    secrets = {"MACOS_CERTIFICATE_P12_BASE64": base64.b64encode(certificate),
               "MACOS_CERTIFICATE_PASSWORD": password.encode(),
               "APPLE_NOTARY_KEY_P8_BASE64": base64.b64encode(api_key)}
    require(all(len(value) <= 48 * 1024 for value in secrets.values()),
            "A credential exceeds GitHub's 48KB secret limit; no secrets were uploaded")
    require(len(password.encode()) <= 2048 and not any(char in password for char in ("\r", "\n", "\0")),
            "Certificate password exceeds the supported private importer limits")
    for name, value in secrets.items():
        run("gh", "secret", "set", name, "--env", environment, "--repo", repository, input=value)
    for name, value in {"APPLE_TEAM_ID": TEAM, "MACOS_CERTIFICATE_FINGERPRINT": FINGERPRINT,
                        "APPLE_NOTARY_KEY_ID": key_id, "APPLE_NOTARY_ISSUER_ID": issuer}.items():
        run("gh", "variable", "set", name, "--env", environment, "--repo", repository, input=value.encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="batuhanoktem/c64-keyboard-bridge")
    parser.add_argument("--apply", action="store_true", help="Explicitly provision the protected environment")
    args = parser.parse_args()
    require(args.apply, "Use --apply only after reviewing environment protections and exported credentials")
    certificate = private_file(input("Path to password-protected Developer ID .p12: "), 1024**2)
    password = getpass.getpass("Certificate export password (hidden): ")
    require(bool(password) and "\n" not in password and "\r" not in password,
            "Certificate password must be nonempty and on one line")
    api = private_file(input("Path to dedicated Team API .p8: "), 65536)
    require(b"-----BEGIN PRIVATE KEY-----" in api, "Notarization private-key format is incorrect")
    key_id = input("Team API key ID: ").strip()
    issuer = input("Team API issuer UUID: ").strip()
    import re
    import uuid
    require(re.fullmatch(r"[A-Za-z0-9]{10,}", key_id) is not None, "API key ID is invalid")
    try:
        uuid.UUID(issuer)
    except ValueError:
        raise ReleaseError("API issuer ID is invalid") from None
    provision(args.repository, "macos-release", certificate, password, api, key_id, issuer)
    print("Protected environment secrets uploaded; no credential values displayed")


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt):
        print("Credential provisioning failed or was cancelled; details suppressed. No fallback file was written.", file=sys.stderr)
        sys.exit(1)

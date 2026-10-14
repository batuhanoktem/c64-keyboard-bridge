#!/usr/bin/env python3
"""Recover a verified upload without importing keys, signing or rebuilding packages."""
import argparse
from pathlib import Path
import signal
import sys

from release_recovery import recover_upload
from release_service import GitHub, ReleaseError


def interrupted(_signal, _frame):
    raise ReleaseError("Release upload recovery was cancelled; no automatic publication retry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="batuhanoktem/c64-keyboard-bridge")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--ci-run", required=True)
    parser.add_argument("--promotion-run", required=True)
    parser.add_argument("--record", required=True, type=Path)
    parser.add_argument("--publish", action="store_true", help="Explicitly publish only after final download verification")
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    record = recover_upload(GitHub(args.repository), args.tag, args.ci_run, args.promotion_run, args.record, args.publish)
    print("Published recovered verified release" if record["publish"] else "Recovered verified release remains a draft")


if __name__ == "__main__":
    try:
        main()
    except ReleaseError as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("Release upload recovery failed; raw errors were suppressed and publication is blocked", file=sys.stderr)
        sys.exit(1)

#!/usr/bin/env python3
"""Regenerate the locked registry inventory in THIRD_PARTY_NOTICES.md from Cargo manifests."""
from pathlib import Path
import subprocess
import json

ROOT = Path(__file__).resolve().parent.parent
document = ROOT / 'THIRD_PARTY_NOTICES.md'
metadata = json.loads(subprocess.run(['cargo', 'metadata', '--locked', '--format-version', '1'],
                                    cwd=ROOT, check=True, capture_output=True, text=True).stdout)
packages = sorted((package for package in metadata['packages'] if package['source'] is not None),
                  key=lambda package: (package['name'], package['version']))
table = '| Package | Locked version | Declared license expression | Repository |\n| --- | --- | --- | --- |\n'
for package in packages:
    repository = package.get('repository')
    link = f'[upstream]({repository})' if repository else 'See vendored manifest'
    license_name = package.get('license') or 'See vendored license-file'
    table += f"| {package['name']} | {package['version']} | `{license_name}` | {link} |\n"
text = document.read_text()
start = text.index('| Package | Locked version |')
end = text.index('\nThe release source archive', start)
document.write_text(text[:start] + table.rstrip() + '\n' + text[end:])
print(f'Recorded {len(packages)} locked registry packages')

#!/usr/bin/env python3
"""Retain omitted upstream licenses at published crate commits, with origin and content hashes."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import re
import subprocess
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
DESTINATION = ROOT / 'third-party/notices'
CANDIDATES = ('LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-ZLIB',
              'LICENSE-MIT.txt', 'LICENSE-APACHE.txt', 'LICENSE.txt', 'LICENSE.md',
              'COPYING', 'NOTICE', 'NOTICE.txt')


def is_notice(path):
    lower = path.name.lower()
    return lower.startswith(('license', 'licence', 'copying', 'notice', 'copyright', 'authors')) or 'license' in lower or 'licence' in lower


def supplement(package):
    root = Path(package['manifest_path']).parent
    if any(path.is_file() and is_notice(path) for path in root.rglob('*')):
        return None
    vcs = json.loads((root / '.cargo_vcs_info.json').read_text())
    revision = vcs['git']['sha1']
    if re.fullmatch('[a-f0-9]{40}', revision) is None:
        raise ValueError('Published crate has an invalid source revision')
    parsed = urllib.parse.urlparse(package['repository'])
    parts = parsed.path.strip('/').removesuffix('.git').split('/')
    if parsed.hostname != 'github.com' or len(parts) < 2 or any(re.fullmatch('[A-Za-z0-9_.-]+', part) is None for part in parts[:2]):
        raise ValueError(f"Missing notices require a reviewed upstream source: {package['name']}")
    repository = '/'.join(parts[:2])
    destination = DESTINATION / f"{package['name']}-{package['version']}"
    notices = []
    for name in CANDIDATES:
        url = f'https://raw.githubusercontent.com/{repository}/{revision}/{name}'
        try:
            request = urllib.request.Request(url, headers={'User-Agent': 'C64KeyboardBridge-notice-audit'})
            with urllib.request.urlopen(request, timeout=30) as response:
                data = response.read(128 * 1024 + 1)
            if len(data) > 128 * 1024:
                raise ValueError('Upstream notice exceeds the review bound')
        except urllib.error.HTTPError as error:
            if error.code == 404:
                continue
            raise
        destination.mkdir(parents=True, exist_ok=True)
        (destination / name).write_bytes(data)
        notices.append({'file': name, 'url': url, 'sha256': hashlib.sha256(data).hexdigest()})
    if not notices and package['name'] == 'dispatch' and package['version'] == '0.2.0':
        # This published MIT crate supplies no license/copyright text even in its exact upstream tree.
        # Preserve the author's declaration and README; do not fabricate a year or ownership notice.
        destination.mkdir(parents=True, exist_ok=True)
        for name in ('Cargo.toml.orig', 'README.md'):
            data = (root / name).read_bytes()
            (destination / name).write_bytes(data)
            notices.append({'file': name, 'url': f'https://github.com/{repository}/tree/{revision}',
                            'sha256': hashlib.sha256(data).hexdigest(), 'source': 'unchanged-published-crate'})
    if not notices:
        raise ValueError(f"No root notices found at the published {package['name']} source revision; manual review required")
    record = {'package': package['name'], 'version': package['version'], 'revision': revision, 'notices': notices}
    if package['name'] == 'dispatch' and package['version'] == '0.2.0':
        record['notice_omission'] = 'MIT declared in upstream manifest; no standalone license or copyright text supplied'
    return record


def main():
    metadata = json.loads(subprocess.run(['cargo', 'metadata', '--locked', '--format-version', '1'],
                                       cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    packages = [package for package in metadata['packages'] if package['source'] is not None]
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
        origins = [origin for origin in executor.map(supplement, packages) if origin is not None]
    DESTINATION.mkdir(parents=True, exist_ok=True)
    (DESTINATION / 'origins.json').write_text(json.dumps({'schema': 1, 'packages': sorted(origins, key=lambda item: (item['package'], item['version']))}, indent=2) + '\n')
    print(f'Retained exact-commit upstream notices for {len(origins)} crates')


if __name__ == '__main__':
    main()

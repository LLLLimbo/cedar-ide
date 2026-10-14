#!/usr/bin/env python3
"""Create privacy-reviewed source snapshots from immutable tested Git tags.

No network writes, credentials, unreviewed binaries or working-tree changes are included.
The resulting stage directories are inputs to an explicitly authorized publisher.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess

STAGES = ('phase1-0.1.0', 'phase2-0.2.0', 'phase3-0.3.0')
REQUIRED_FIXTURE = 'crates/language/tests/evidence/jdtls-1.61.0-completion-resolve.jsonl'
FONT_ROOT = 'crates/app/assets/fonts/'
# These exact, unmodified OFL faces are the only permitted binary source assets.
# Neither a filename extension nor a manifest-provided hash grants approval.
APPROVED_FONTS = {
    FONT_ROOT + 'Inter-Regular.ttf': (
        '40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82',
        'Inter-LICENSE.txt',
        '262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a'),
    FONT_ROOT + 'JetBrainsMono-Regular.ttf': (
        'a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f',
        'JetBrainsMono-OFL.txt',
        '30f0c136e3c88e422d0791acd97238870f9054a9729bc34cf2ff0d4ed8cac4ad'),
}
SENSITIVE_PATTERNS = {
    'private key': rb'-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----',
    'GitHub token': rb'(?:gh[pousr]_[A-Za-z0-9_]{30,}|github_pat_[A-Za-z0-9_]{30,})',
    'AWS access key': rb'AKIA[A-Z0-9]{16}',
    'cloud workspace path': rb'/workspace/(?:scratch/[a-f0-9]{8,}|shared)/',
}


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args])


def omitted(path):
    return path != REQUIRED_FIXTURE and ('/evidence/' in path or (path.startswith('docs/') and not path.endswith('.md')))


def verified_font_assets(root, items):
    """Validate the complete font bundle using only blobs in the tagged tree."""
    blobs = {path: (mode, oid) for path, mode, oid in items}
    if not APPROVED_FONTS.keys() & blobs.keys():
        return {}  # Historical checkpoints contain no embedded font bundle.

    def required(path):
        if path not in blobs or blobs[path][0] != '100644' or omitted(path):
            raise RuntimeError(f'Blocked export: missing or invalid font bundle file {path}')
        return git(root, 'cat-file', 'blob', blobs[path][1])

    manifest = json.loads(required(FONT_ROOT + 'manifest.json').decode('utf-8'))
    assets = manifest.get('assets') if isinstance(manifest, dict) else None
    if (not isinstance(manifest, dict) or manifest.get('schema_version') != 1
            or not isinstance(assets, list) or len(assets) != len(APPROVED_FONTS)):
        raise RuntimeError('Blocked export: invalid font manifest')
    records = {}
    names = {PurePosixPath(path).name for path in APPROVED_FONTS}
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get('file'), str):
            raise RuntimeError('Blocked export: invalid font manifest entry')
        name = asset['file']
        if name not in names or name in records:
            raise RuntimeError('Blocked export: unexpected or duplicate font manifest entry')
        records[name] = asset

    required(FONT_ROOT + 'README.md').decode('utf-8')
    notices = required('THIRD_PARTY_NOTICES.md').decode('utf-8')
    # collect_licenses.py removes trailing whitespace when embedding licenses.
    normalized_notices = '\n'.join(line.rstrip() for line in notices.splitlines())
    fonts = {}
    for path, (sha256, license_file, license_sha256) in APPROVED_FONTS.items():
        data = required(path)
        asset = records[PurePosixPath(path).name]
        expected = {'sha256': sha256, 'size_bytes': len(data), 'license': 'OFL-1.1',
                    'license_file': license_file, 'license_sha256': license_sha256}
        if (hashlib.sha256(data).hexdigest() != sha256
                or any(asset.get(key) != value for key, value in expected.items())
                or asset.get('modified') is not False):
            raise RuntimeError(f'Blocked export: unapproved font bytes or manifest mismatch in {path}')
        license_data = required(FONT_ROOT + license_file)
        license_text = license_data.decode('utf-8')
        if hashlib.sha256(license_data).hexdigest() != license_sha256:
            raise RuntimeError(f'Blocked export: unapproved font license in {license_file}')
        normalized_license = '\n'.join(line.rstrip() for line in license_text.splitlines())
        if normalized_license not in normalized_notices or sha256 not in notices:
            raise RuntimeError(f'Blocked export: incomplete font notices for {path}')
        fonts[path] = data
    return fonts


def expected_tree_sha(files):
    root = {}
    for item in files:
        components = item['path'].split('/')
        tree = root
        for component in components[:-1]:
            tree = tree.setdefault(component, {})
        assert components[-1] not in tree
        tree[components[-1]] = (item['mode'], item['sha'])

    def digest(tree):
        entries = []
        for name, node in tree.items():
            if isinstance(node, dict):
                mode, sha, key = '40000', digest(node), name.encode() + b'/'
            else:
                mode, sha = node
                key = name.encode()
            entries.append((key, mode.encode() + b' ' + name.encode() + b'\0' + bytes.fromhex(sha)))
        data = b''.join(value for _, value in sorted(entries, key=lambda entry: entry[0]))
        return hashlib.sha1(f'tree {len(data)}\0'.encode() + data).hexdigest()

    return digest(root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--tags', nargs='+', default=STAGES)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    destination = args.destination.resolve()
    if destination.exists():
        parser.error('Destination must be new; never overwrite a previously reviewed export')
    destination.mkdir(parents=True)
    manifest = {'policy': 'source-only; generated raw evidence omitted; source tags immutable', 'stages': []}
    for tag in args.tags:
        commit = git(root, 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}').decode().strip()
        items = []
        for item in git(root, 'ls-tree', '-rz', '--full-tree', commit).split(b'\0'):
            if not item:
                continue
            meta, name = item.split(b'\t', 1)
            mode, kind, oid = meta.decode().split()
            path = name.decode()
            assert kind == 'blob' and mode in ('100644', '100755'), path
            assert not PurePosixPath(path).is_absolute() and '..' not in PurePosixPath(path).parts
            items.append((path, mode, oid))
        fonts = verified_font_assets(root, items)
        excluded = {path for path, _, _ in items if omitted(path)}
        stage = destination / tag
        stage.mkdir()
        exported = []
        for path, mode, oid in items:
            if path in excluded or path == 'PUBLICATION.md':
                # A recovered public checkout already contains this generated
                # file. Regenerate it exactly once for the new checkpoint.
                continue
            if path in fonts:
                data = fonts[path]  # Preserve approved TTF bytes, never decode/re-encode.
            else:
                data = git(root, 'cat-file', 'blob', oid)
                text = data.decode('utf-8')  # Fail closed for any unexpected binary.
                if '\0' in text:
                    raise RuntimeError(f'Blocked export: unexpected binary in {path}')
                if path == REQUIRED_FIXTURE:
                    # This complete response is a compile-time deterministic fixture.
                    # Retain it, not unrelated startup logs and temporary source URIs.
                    records = [json.loads(line) for line in text.splitlines()]
                    records = [record for record in records if record.get('kind') == 'completion_resolve']
                    assert len(records) == 1
                    text = json.dumps(records[0], ensure_ascii=False, separators=(',', ':')) + '\n'
                # Source code contains no environment-specific path; these replacements
                # apply only to historical prose such as measured JVM command examples.
                if path.endswith('.md'):
                    text = re.sub(r'/workspace/scratch/[a-f0-9]{8,}/', '/path/to/', text)
                    text = text.replace('/workspace/' + 'shared/', '/path/to/shared/')
                if path.endswith('.md'):
                    note_link = ('../' * len(PurePosixPath(path).parts[:-1])) + 'PUBLICATION.md'
                    mentioned = any(raw in text or Path(raw).name in text for raw in excluded)
                    # Retain descriptions of measurements, without dangling hyperlinks.
                    def clean_link(match):
                        label, target = match.group(1), match.group(2)
                        if any(target == raw or target.endswith('/' + Path(raw).name)
                               or target == Path(raw).name for raw in excluded):
                            return '[' + label + '](' + note_link + '#verification-evidence)'
                        return match.group(0)
                    text = re.sub(r'\[([^\]\n]+)\]\(([^)\n]+)\)', clean_link, text)
                    if mentioned and "> Public-source note:" not in text:
                        lines = text.splitlines(keepends=True)
                        lines.insert(1, '\n> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See ['
                                     'verification evidence](' + note_link + '#verification-evidence).\n')
                        text = ''.join(lines)
                if path == 'README.md':
                    footer = 'Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).'
                    if footer not in text:
                        text += '\n' + footer + '\n'
                data = text.encode()
            for description, pattern in SENSITIVE_PATTERNS.items():
                if re.search(pattern, data):
                    raise RuntimeError(f'Blocked export: {description} in {path}')
            out = stage / path
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(data)
            out.chmod(0o755 if mode == '100755' else 0o644)
            exported.append({'path': path, 'mode': mode, 'type': 'blob', 'bytes': len(data),
                             'sha': hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest(),
                             'sha256': hashlib.sha256(data).hexdigest()})
            if path in fonts:
                # GitHub blob transport encoding; the staged file remains raw TTF.
                exported[-1]['encoding'] = 'base64'
        publication = f'''# Public source checkpoint

This is the source-only public export of tested development stage `{tag}`.
It is an independent Rust IDE project, not a complete IntelliJ IDEA replacement.
Public commits preserve the phase-by-phase development sequence, but their hashes
differ from the private build checkpoints because generated evidence is omitted.

## Verification evidence

Source, deterministic tests, test fixtures, reproducible verification scripts,
Cargo.lock, CI configuration, and upstream license notices are included.
The only permitted binary source assets are two exact, hash-pinned OFL font faces
(when present), accompanied by their readable manifest and complete license notices.
The one complete JDT completion response needed by a deterministic compile-time
test remains as a data fixture, without unrelated startup logs or temporary URIs.
Raw screenshots, process logs and other JSON/JSONL measurement payloads from the cloud
test machine are deliberately omitted. References to their filenames in the
historical reports describe the original tests; they are not downloadable files
in this public repository. No new benchmark or platform guarantee is implied.
Run the documented test commands to produce fresh evidence on your own machine.

No prebuilt executables, JDK, Java/Kotlin language-server distributions, debug
adapters, credentials, private user projects or tool caches are included.
The 343 locked upstream dependencies keep their respective licenses; bundled
license texts may contain their upstream authors' public copyright information.
Java/Kotlin servers still need separate installation and any applicable license.
Native Windows/macOS GUI, authenticated SSH interoperability, full refactoring, integrated
debugging and IntelliJ plugin compatibility are not claimed by this checkpoint.
'''
        data = publication.encode()
        (stage / 'PUBLICATION.md').write_bytes(data)
        exported.append({'path': 'PUBLICATION.md', 'mode': '100644', 'type': 'blob', 'bytes': len(data),
                         'sha': hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest(),
                         'sha256': hashlib.sha256(data).hexdigest()})
        manifest['stages'].append({'tag': tag, 'private_source_commit': commit,
                                   'directory': str(stage), 'expected_tree_sha': expected_tree_sha(exported),
                                   'files': sorted(exported, key=lambda item: item['path']),
                                   'omitted_files': sorted(excluded)})
    target = destination / 'publish-manifest.json'
    target.write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({'manifest': str(target), 'stages': [
        {'tag': stage['tag'], 'files': len(stage['files']), 'bytes': sum(file['bytes'] for file in stage['files']),
         'omitted_files': len(stage['omitted_files'])} for stage in manifest['stages']]}, indent=2))


if __name__ == '__main__':
    main()

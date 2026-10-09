#!/usr/bin/env python3
"""Prepare only the pinned CI Maven cache; never execute Maven or a JAR.

This is separate from offline import. HTTPS reads use ordinary certificate
verification, no credentials, no redirects, exact sizes and hashes. The caller's
CI process deadline remains the enclosing wall-clock boundary.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import ssl
import stat
import time
import urllib.parse
import urllib.request
import zipfile

ORIGIN = 'https://repo.maven.apache.org/maven2/'
MANIFEST_SHA256 = '2afceba6a8f6b648a1dbf48cc356b931233cc57bc82b52d02fefdd58e5e876ac'
FILE_COUNT = 83
TOTAL_BYTES = 4_065_288
MAX_FILE_BYTES = 1024 * 1024
MAX_TOTAL_BYTES = 5 * 1024 * 1024
MAX_REQUESTS = 83
PREPARATION_SECONDS = 240


class CacheError(Exception):
    """Fixed public-safe failure category; never contains remote response text."""


def require(condition, category):
    if not condition:
        raise CacheError(category)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def validate_manifest(value):
    require(isinstance(value, dict) and value.get('schema_version') == 1,
            'manifest_schema')
    require(value.get('origin') == ORIGIN and value.get('expected_files') == FILE_COUNT
            and value.get('expected_bytes') == TOTAL_BYTES, 'manifest_scope')
    entries = value.get('entries')
    require(isinstance(entries, list) and len(entries) == FILE_COUNT, 'manifest_count')
    paths = set()
    total = 0
    for entry in entries:
        require(isinstance(entry, dict), 'manifest_entry')
        path = entry.get('path')
        require(isinstance(path, str) and re.fullmatch(r'[A-Za-z0-9._/-]+', path),
                'manifest_path')
        relative = PurePosixPath(path)
        require(not relative.is_absolute() and relative.as_posix() == path
                and all(part not in ('', '.', '..') for part in relative.parts),
                'manifest_path')
        require(path not in paths and relative.suffix in ('.jar', '.pom'), 'manifest_path')
        paths.add(path)
        gav = entry.get('gav')
        require(isinstance(gav, str) and len(gav.split(':')) == 3, 'manifest_coordinate')
        group, artifact, version = gav.split(':')
        expected = f'{group.replace(".", "/")}/{artifact}/{version}/{artifact}-{version}{relative.suffix}'
        require(path == expected, 'manifest_coordinate')
        size = entry.get('bytes')
        require(type(size) is int and 0 < size <= MAX_FILE_BYTES, 'manifest_size')
        total += size
        require(isinstance(entry.get('sha256'), str)
                and re.fullmatch('[0-9a-f]{64}', entry['sha256']), 'manifest_hash')
        published = entry.get('published_digest')
        require(isinstance(published, dict) and published.get('algorithm') in ('sha1', 'sha512'),
                'published_digest')
        width = 40 if published['algorithm'] == 'sha1' else 128
        require(isinstance(published.get('value'), str)
                and re.fullmatch(f'[0-9a-f]{{{width}}}', published['value']), 'published_digest')
    require(total == TOTAL_BYTES and total <= MAX_TOTAL_BYTES, 'manifest_total')
    return entries


def load_manifest():
    path = Path(__file__).with_name('maven_cache_manifest.json')
    data = path.read_bytes()
    require(len(data) <= 256 * 1024 and sha256(data) == MANIFEST_SHA256, 'manifest_identity')
    return validate_manifest(json.loads(data))


def ordinary(path, directory):
    metadata = path.lstat()
    require(not stat.S_ISLNK(metadata.st_mode)
            and not (getattr(metadata, 'st_file_attributes', 0) & 0x400), 'reparse_path')
    require(stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode),
            'path_type')
    return metadata


def verify_bytes(data, entry):
    require(len(data) == entry['bytes'], 'artifact_size')
    require(sha256(data) == entry['sha256'], 'artifact_sha256')
    published = entry['published_digest']
    require(hashlib.new(published['algorithm'], data).hexdigest() == published['value'],
            'artifact_published_digest')
    if entry['path'].endswith('.jar'):
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            members = archive.infolist()
            require(len(members) <= 10000
                    and sum(member.file_size for member in members) <= 32 * 1024 * 1024,
                    'jar_expansion_limit')


def verify_cache(root, entries):
    ordinary(root, True)
    expected = {entry['path']: entry for entry in entries}
    found = set()
    pending = [root]
    count = 0
    while pending:
        for path in pending.pop().iterdir():
            count += 1
            require(count <= 1024, 'cache_entry_limit')
            metadata = path.lstat()  # Fresh handle/path metadata on Windows, not DirEntry.stat.
            require(not stat.S_ISLNK(metadata.st_mode)
                    and not (getattr(metadata, 'st_file_attributes', 0) & 0x400), 'reparse_path')
            if stat.S_ISDIR(metadata.st_mode):
                pending.append(path)
            else:
                ordinary(path, False)
                relative = path.relative_to(root).as_posix()
                require(relative in expected, 'cache_extra_file')
                entry = expected[relative]
                require(metadata.st_size == entry['bytes'] <= MAX_FILE_BYTES, 'artifact_size')
                with path.open('rb') as stream:
                    data = stream.read(MAX_FILE_BYTES + 1)
                verify_bytes(data, entry)
                found.add(relative)
    require(found == set(expected), 'cache_missing_file')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise CacheError('redirect_rejected')


def fetch_one(entry, opener, deadline):
    require(time.monotonic() < deadline, 'preparation_deadline')
    url = ORIGIN + entry['path']
    request = urllib.request.Request(url, headers={
        'User-Agent': 'Cedar-CI-Maven-cache/1.0',
        'Accept': 'application/octet-stream', 'Accept-Encoding': 'identity'})
    try:
        with opener.open(request, timeout=min(15, max(0.1, deadline - time.monotonic()))) as response:
            require(response.status == 200 and response.geturl() == url, 'response_origin_or_status')
            require(response.headers.get('Content-Encoding', 'identity').lower() == 'identity',
                    'content_encoding')
            length = response.headers.get('Content-Length')
            if length is not None:
                require(int(length) == entry['bytes'], 'content_length')
            result = bytearray()
            while len(result) <= entry['bytes']:
                require(time.monotonic() < deadline, 'preparation_deadline')
                chunk = response.read1(min(64 * 1024, entry['bytes'] + 1 - len(result)))
                if not chunk:
                    break
                result.extend(chunk)
            data = bytes(result)
    except CacheError:
        raise
    except Exception:
        raise CacheError('network_request_failed') from None
    require(time.monotonic() < deadline, 'preparation_deadline')
    verify_bytes(data, entry)
    return data


def prepare(root, entries, opener):
    require(not root.exists() and not root.is_symlink(), 'destination_exists')
    for ancestor in [*reversed(root.parent.parents), root.parent]:
        ordinary(ancestor, True)
    root.mkdir()
    deadline = time.monotonic() + PREPARATION_SECONDS
    transferred = 0
    for index, entry in enumerate(entries):
        require(index < MAX_REQUESTS, 'request_limit')
        data = fetch_one(entry, opener, deadline)
        transferred += len(data)
        require(transferred <= MAX_TOTAL_BYTES, 'transfer_limit')
        destination = root.joinpath(*PurePosixPath(entry['path']).parts)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open('xb') as stream:
            stream.write(data)
    verify_cache(root, entries)
    return transferred


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--destination', type=Path)
    mode.add_argument('--verify-existing', type=Path)
    args = parser.parse_args()
    report = {'kind': 'cedar_ci_maven_cache', 'manifest_sha256': MANIFEST_SHA256,
              'status': 'failed', 'signatures_verified': False,
              'execution_performed': False, 'import_performed': False}
    try:
        entries = load_manifest()
        if args.verify_existing is not None:
            verify_cache(args.verify_existing.absolute(), entries)
            requests = 0
        else:
            for name, value in os.environ.items():
                if name.lower().endswith('_proxy'):
                    proxy = urllib.parse.urlsplit(value)
                    require(proxy.username is None and proxy.password is None,
                            'credential_bearing_proxy_rejected')
            opener = urllib.request.build_opener(NoRedirect(), urllib.request.HTTPSHandler(
                context=ssl.create_default_context()))
            prepare(args.destination.absolute(), entries, opener)
            requests = len(entries)
        report.update(status='complete', artifact_files=len(entries), artifact_bytes=TOTAL_BYTES,
                      network_requests=requests, exact_inventory_verified=True)
    except CacheError as error:
        report['failure'] = str(error)
    except Exception:
        report['failure'] = 'local_preparation_failed'
    print(json.dumps(report, sort_keys=True))
    return 0 if report['status'] == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())

import copy
import hashlib
import io
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
import time
import unittest
from unittest import mock

import prepare_maven_cache as cache


DATA = b'<project/>'


def entry():
    return {'path': 'org/example/test/1/test-1.pom', 'gav': 'org.example:test:1',
            'bytes': len(DATA), 'sha256': hashlib.sha256(DATA).hexdigest(),
            'published_digest': {'algorithm': 'sha1', 'value': hashlib.sha1(DATA).hexdigest()}}


class Response(io.BytesIO):
    def __init__(self, data=DATA, *, status=200, url=None, headers=None):
        super().__init__(data)
        self.status = status
        self.url = url or cache.ORIGIN + entry()['path']
        self.headers = headers or {}

    def geturl(self):
        return self.url


class CacheTests(unittest.TestCase):
    def test_frozen_manifest_has_exact_public_inventory_and_no_local_inputs(self):
        manifest = Path(cache.__file__).with_name('maven_cache_manifest.json').read_bytes()
        self.assertEqual(hashlib.sha256(manifest).hexdigest(), cache.MANIFEST_SHA256)
        entries = cache.load_manifest()
        self.assertEqual(len(entries), 83)
        self.assertEqual(sum(item['bytes'] for item in entries), 4_065_288)
        self.assertEqual(max(item['bytes'] for item in entries), 713_862)
        self.assertTrue(all(not item['path'].startswith('/') for item in entries))

    @unittest.skipUnless(shutil.which('git'), 'requires Git checkout regression')
    def test_frozen_manifest_checkout_preserves_hash_with_autocrlf_enabled(self):
        repository = Path(cache.__file__).resolve().parent.parent
        manifest = (repository / 'scripts/maven_cache_manifest.json').read_bytes()
        attributes = (repository / '.gitattributes').read_bytes()
        with tempfile.TemporaryDirectory(prefix='cedar-manifest-checkout-') as temporary:
            root = Path(temporary)
            environment = {key: value for key, value in os.environ.items()
                           if not key.upper().startswith('GIT_')}
            environment['GIT_CONFIG_NOSYSTEM'] = '1'
            environment['GIT_CONFIG_GLOBAL'] = os.devnull

            def git(*arguments):
                subprocess.run([shutil.which('git'), '-c', 'core.autocrlf=true',
                                '-c', 'core.safecrlf=false', *arguments], cwd=root,
                               env=environment, check=True, capture_output=True, timeout=10)

            git('init', '--quiet')
            (root / 'scripts').mkdir()
            target = root / 'scripts/maven_cache_manifest.json'
            target.write_bytes(manifest)
            # A control proves the checkout conversion is exercised on this OS.
            control = root / 'unprotected.txt'
            control.write_bytes(b'first\nsecond\n')
            (root / '.gitattributes').write_bytes(attributes)
            git('add', '.gitattributes', 'scripts/maven_cache_manifest.json', 'unprotected.txt')
            target.unlink()
            control.unlink()
            git('checkout-index', '--all')
            self.assertEqual(control.read_bytes(), b'first\r\nsecond\r\n')
            self.assertEqual(target.read_bytes(), manifest)
            self.assertEqual(hashlib.sha256(target.read_bytes()).hexdigest(),
                             cache.MANIFEST_SHA256)

    def test_manifest_rejects_path_coordinate_size_and_hash_changes(self):
        import json
        original = json.loads(Path(cache.__file__).with_name('maven_cache_manifest.json').read_text())
        for key, value in [('path', '../escape.jar'), ('path', 'https://other.invalid/file.jar'),
                           ('gav', 'wrong:coordinate:1'), ('bytes', True),
                           ('bytes', cache.MAX_FILE_BYTES + 1), ('sha256', 'bad')]:
            with self.subTest(key=key, value=value):
                changed = copy.deepcopy(original)
                changed['entries'][0][key] = value
                with self.assertRaises(cache.CacheError):
                    cache.validate_manifest(changed)
        duplicate = copy.deepcopy(original)
        duplicate['entries'][1] = duplicate['entries'][0]
        with self.assertRaises(cache.CacheError):
            cache.validate_manifest(duplicate)

    def test_successful_download_is_exact_and_has_no_auth_header(self):
        opener = mock.Mock()
        opener.open.return_value = Response(headers={'Content-Length': str(len(DATA))})
        self.assertEqual(cache.fetch_one(entry(), opener, time.monotonic() + 10), DATA)
        request = opener.open.call_args.args[0]
        self.assertEqual(request.full_url, cache.ORIGIN + entry()['path'])
        self.assertIsNone(request.data)
        self.assertIsNone(request.get_header('Authorization'))
        self.assertIsNone(request.get_header('Cookie'))

    def test_redirect_is_rejected_without_returning_a_new_request(self):
        with self.assertRaisesRegex(cache.CacheError, 'redirect_rejected'):
            cache.NoRedirect().redirect_request(None, None, 302, '', {}, 'https://other.invalid/')

    def test_response_identity_encoding_size_and_hash_fail_closed(self):
        for response in [Response(status=403), Response(url='https://other.invalid/'),
                         Response(headers={'Content-Encoding': 'gzip'}),
                         Response(headers={'Content-Length': '999'}), Response(data=b'short'),
                         Response(data=DATA + b'!'), Response(data=b'x' * len(DATA))]:
            with self.subTest(response=response):
                opener = mock.Mock()
                opener.open.return_value = response
                with self.assertRaises(cache.CacheError):
                    cache.fetch_one(entry(), opener, time.monotonic() + 10)

    def test_deadline_prevents_request_and_network_exception_stays_inert(self):
        opener = mock.Mock()
        with self.assertRaisesRegex(cache.CacheError, 'preparation_deadline'):
            cache.fetch_one(entry(), opener, time.monotonic() - 1)
        opener.open.assert_not_called()
        opener.open.side_effect = RuntimeError('private remote response sentinel')
        with self.assertRaisesRegex(cache.CacheError, '^network_request_failed$'):
            cache.fetch_one(entry(), opener, time.monotonic() + 10)

    def test_fresh_unicode_cache_is_exact_and_existing_directory_never_overwritten(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'cache 雪'
            opener = mock.Mock()
            opener.open.return_value = Response()
            self.assertEqual(cache.prepare(root, [entry()], opener), len(DATA))
            cache.verify_cache(root, [entry()])
            with self.assertRaisesRegex(cache.CacheError, 'destination_exists'):
                cache.prepare(root, [entry()], opener)
            self.assertEqual(opener.open.call_count, 1)

    def test_cache_extra_origin_marker_and_missing_or_changed_bytes_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / entry()['path']
            target.parent.mkdir(parents=True)
            target.write_bytes(DATA)
            cache.verify_cache(root, [entry()])
            marker = target.parent / '_remote.repositories'
            marker.write_bytes(b'not allowed')
            with self.assertRaisesRegex(cache.CacheError, 'cache_extra_file'):
                cache.verify_cache(root, [entry()])
            marker.unlink()
            target.write_bytes(b'x' * len(DATA))
            with self.assertRaisesRegex(cache.CacheError, 'artifact_sha256'):
                cache.verify_cache(root, [entry()])
            target.unlink()
            with self.assertRaisesRegex(cache.CacheError, 'cache_missing_file'):
                cache.verify_cache(root, [entry()])

    def test_reparse_metadata_rejected_without_following_it(self):
        path = mock.Mock()
        path.lstat.return_value = mock.Mock(st_mode=0o040755, st_file_attributes=0x400)
        with self.assertRaisesRegex(cache.CacheError, 'reparse_path'):
            cache.ordinary(path, True)

    def test_published_digest_is_checked_independently_of_local_sha256(self):
        changed = entry()
        changed['published_digest']['value'] = '0' * 40
        with self.assertRaisesRegex(cache.CacheError, 'artifact_published_digest'):
            cache.verify_bytes(DATA, changed)


if __name__ == '__main__':
    unittest.main()

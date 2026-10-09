#!/usr/bin/env python3
"""Actual-agent capability/trust checks on disposable local stdio fixtures.

No SSH, credentials, listeners, tool discovery or workspace commands are used.
"""
import json
from pathlib import Path
import re
import sys
import tempfile

from agent_harness import Agent


BASE = {'list', 'read', 'write', 'search'}
TASKS = {'run_start', 'run_poll', 'run_cancel'}
LANGUAGE = {'language_start', 'language_open', 'language_change',
            'language_close', 'language_events', 'language_stop'}
WINDOWS_JAVA = {'language_start_java', 'language_start_java_begin',
                'language_start_java_poll', 'language_start_java_cancel',
                'language_open', 'language_change', 'language_close',
                'language_events', 'language_stop'}
WINDOWS_MAVEN = {'language_start_java_maven_begin', 'language_maven_model'}
WINDOWS_IMPLEMENTATIONS = {'language_java_implementations'}


def validate(hello):
    assert hello['type'] == 'hello' and hello['protocol'] == 4, hello
    info = hello['agent']
    assert info['schema'] == 1
    assert isinstance(info['version'], str) and 1 <= len(info['version']) <= 64
    assert all(32 <= ord(c) <= 126 for c in info['version'])
    for field in ('os', 'arch'):
        assert re.fullmatch(r'[a-z0-9._-]{1,32}', info[field])
    capabilities = info['capabilities']
    assert isinstance(capabilities, list) and len(capabilities) <= 32
    assert len(set(capabilities)) == len(capabilities)
    assert all(re.fullmatch(r'[a-z0-9._-]{1,64}', name) for name in capabilities)
    assert BASE <= set(capabilities)
    return info


def validate_platform_capabilities(info):
    capabilities = set(info['capabilities'])
    if info['os'] in ('linux', 'macos'):
        assert TASKS | {'run', 'git_status', 'git_changes', 'git_diff'} <= capabilities
    elif info['os'] == 'windows':
        # The executable is the normal isolated agent, never a fixture host.
        assert TASKS | {'git_changes', 'git_diff'} <= capabilities
        assert not {'run', 'git_status'} & capabilities
    else:
        assert not (TASKS | {'run', 'git_status'}) & capabilities
    if info['os'] == 'windows':
        assert WINDOWS_JAVA | WINDOWS_MAVEN | WINDOWS_IMPLEMENTATIONS <= capabilities
        assert 'language_start' not in capabilities
    else:
        assert LANGUAGE <= capabilities
        assert not WINDOWS_IMPLEMENTATIONS & capabilities


def main():
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='cedar-capabilities-') as directory:
        root = Path(directory)
        untrusted = Agent(binary, root, allow_run=False)
        try:
            hello = untrusted.call('hello')
            info = validate(hello)
            assert hello['root'] == str(root.resolve())
            assert untrusted.call('hello') == hello
            assert untrusted.call('list', path='')['entries'] == []
            # Support claims do not grant permission, even via direct protocol.
            for operation, fields in [
                ('git_status', {}),
                ('git_changes', {'git_executable': ''}),
                ('git_diff', {'git_executable': '', 'path': 'not-opened', 'kind': 'unstaged'}),
                ('run', {'program': 'cedar-no-such-tool', 'args': [], 'timeout_secs': 1}),
                ('run_start', {'program': 'cedar-no-such-tool', 'args': [], 'timeout_secs': 1}),
                ('run_poll', {'task_id': 1}),
                ('run_cancel', {'task_id': 1}),
                ('language_start', {'program': 'cedar-no-such-tool', 'args': []}),
                ('language_events', {}),
                ('language_workspace_symbols', {'query': 'NeverLaunched'}),
                ('language_java_implementations', {'path': 'NeverLaunched.java', 'version': 1, 'line': 0, 'character': 0}),
                ('language_stop', {}),
            ]:
                assert untrusted.call(operation, ok=False, **fields)['code'] == 'run_disabled'
            assert untrusted.call('list', path='')['entries'] == []
        finally:
            untrusted.close()
        # Only Hello is requested here; the explicit fixture flag starts no tool.
        trusted = Agent(binary, root, allow_run=True)
        try:
            assert validate(trusted.call('hello')) == info
        finally:
            trusted.close()
        capabilities = set(info['capabilities'])
        validate_platform_capabilities(info)
        assert list(root.iterdir()) == []
        print('PASS: actual agent bounded metadata/platform advertisement/trust independence/no tool startup')
        print(json.dumps({'reported_version': info['version'], 'reported_os': info['os'],
                          'reported_arch': info['arch'], 'capability_count': len(capabilities)}))


if __name__ == '__main__':
    main()

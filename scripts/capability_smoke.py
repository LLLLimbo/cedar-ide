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
LANGUAGE_QUERIES = {'language_query', 'language_format', 'language_references',
                    'language_document_symbols', 'language_workspace_symbols',
                    'language_resolve_uri', 'language_resolve_completion'}
TYPED_JAVA = {'language_start_java', 'language_start_java_begin',
              'language_start_java_poll', 'language_start_java_cancel',
              'java_diagnostics_refresh', 'language_organize_java_imports',
              'language_java_implementations'}
WINDOWS_MAVEN = {'language_start_java_maven_begin', 'language_maven_model',
                 'language_maven_dependencies'}
LINUX_MAVEN_GROUPS = ['java_maven_dependencies_v1', 'java_maven_leaf_v1']


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
    groups = info.get('capability_groups', [])
    assert isinstance(groups, list) and len(groups) <= 2
    assert all(isinstance(name, str) and re.fullmatch(r'[a-z0-9._-]{1,64}', name)
               for name in groups)
    assert len(set(groups)) == len(groups)
    assert sum(len(name) for name in groups) <= 128
    assert len(json.dumps(groups, separators=(',', ':')).encode('ascii')) <= 135
    return info


def validate_platform_capabilities(info):
    # Linux 0.39 keeps its complete 31-name flat inventory and advertises Maven
    # only through these two exact groups. Other shipping hosts still omit it.
    if info['os'] == 'linux':
        assert info.get('capability_groups') == LINUX_MAVEN_GROUPS
    else:
        assert 'capability_groups' not in info
    capabilities = set(info['capabilities'])
    # This script connects only to the normal isolated agent. Require the whole
    # platform set, including absence of capabilities assigned to another host.
    expected = BASE | LANGUAGE_QUERIES
    if info['os'] in ('linux', 'macos'):
        expected |= TASKS | LANGUAGE | {'run', 'git_status', 'git_changes', 'git_diff'}
        if info['os'] == 'linux':
            expected |= TYPED_JAVA
    elif info['os'] == 'windows':
        expected |= TASKS | {'git_changes', 'git_diff'}
        expected |= (LANGUAGE - {'language_start'}) | TYPED_JAVA | WINDOWS_MAVEN
    else:
        expected |= LANGUAGE
    assert capabilities == expected, (info['os'], capabilities ^ expected)


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
                ('language_start_java', {'java_executable': '', 'distribution': '',
                                         'data_directory': ''}),
                ('language_start_java_begin', {'java_executable': '', 'distribution': '',
                                               'data_directory': ''}),
                ('language_start_java_maven_begin', {'java_executable': '', 'distribution': '',
                                                     'data_directory': '', 'local_repository': ''}),
                ('language_start_java_poll', {'startup_id': 1}),
                ('language_start_java_cancel', {'startup_id': 1}),
                ('language_events', {}),
                ('language_workspace_symbols', {'query': 'NeverLaunched'}),
                ('language_refresh_java_diagnostics', {'path': 'NeverLaunched.java', 'version': 1}),
                ('language_organize_java_imports', {'path': 'NeverLaunched.java', 'version': 1}),
                ('language_java_implementations', {'path': 'NeverLaunched.java', 'version': 1, 'line': 0, 'character': 0}),
                ('language_maven_model', {}),
                ('language_maven_dependencies', {'startup_id': 1, 'pom_sha256': 'a' * 64}),
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
                          'reported_arch': info['arch'], 'capability_count': len(capabilities),
                          'capability_group_count': len(info.get('capability_groups', []))}))


if __name__ == '__main__':
    main()

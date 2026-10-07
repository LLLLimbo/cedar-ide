#!/usr/bin/env python3
"""Real process chain: test client -> stdio agent -> deterministic Rust LSP peer.
The peer is a protocol fixture, not a real Java/Kotlin language implementation.
"""
import json, pathlib, subprocess, sys, tempfile

agent = str(pathlib.Path(sys.argv[1]).resolve())
server = str(pathlib.Path(sys.argv[2]).resolve())


def feature_operations(path="Hello.java", version=2):
    return [
        ("language_format", dict(path=path, version=version, tab_size=4, insert_spaces=True)),
        ("language_references", dict(path=path, line=0, character=0, include_declaration=True)),
        ("language_document_symbols", dict(path=path)),
    ]


with tempfile.TemporaryDirectory(prefix="cedar-lsp-bridge-") as tmp:
    root = pathlib.Path(tmp)
    original = "class Hello {}\n"
    (root / "Hello.java").write_text(original)
    audit = root / "audit.jsonl"
    p = subprocess.Popen([agent, "--root", tmp, "--allow-run"], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    counter = 0

    def call(op_type, ok=True, **kwargs):
        global counter
        counter += 1
        p.stdin.write(json.dumps({"id": counter, "op": {"type": op_type, **kwargs}}) + "\n")
        p.stdin.flush()
        answer = json.loads(p.stdout.readline())
        assert answer["id"] == counter
        result = answer["result"]
        assert ("Ok" in result) == ok, result
        return result.get("Ok", result.get("Err"))

    assert call("hello")["protocol"] == 4
    for operation, params in feature_operations():
        assert call(operation, ok=False, **params)["code"] == "language_not_running"
    started = call("language_start", program=server, args=["normal", str(audit)])
    assert started["value"]["started"]
    assert call("language_start", ok=False, program=server, args=[])["code"] == "language_running"
    for operation, params in feature_operations():
        assert call(operation, ok=False, **params)["code"] == "language_document_closed"
    call("language_open", path="Hello.java", language_id="java", version=1, text=original)
    call("language_change", path="Hello.java", version=2, text="class Hello { int count; }\n")
    assert call("language_change", ok=False, path="Hello.java", version=2, text="stale")["code"] == "language_error"
    completion = call("language_query", path="Hello.java", line=0, character=0, kind="completion")
    assert completion["value"]["items"][0]["label"] == "hello"
    resolved = call("language_resolve_completion", item=completion["value"]["items"][0])["value"]
    assert resolved["additionalTextEdits"][0]["newText"] == "import demo.Hello;\n"
    uri = (root / "Hello.java").as_uri()
    assert call("language_resolve_uri", uri=uri)["value"]["path"] == "Hello.java"
    assert call("language_resolve_uri", ok=False, uri="file:///etc/passwd")["code"] == "invalid_path"
    assert call("language_resolve_uri", ok=False, uri="https://example.org/file")["code"] == "unsupported_uri"
    assert call("language_query", path="Hello.java", line=0, character=0, kind="hover")["value"]["contents"]["value"] == "mock hover"
    assert call("language_query", path="Hello.java", line=0, character=0, kind="definition")["value"]["uri"] == uri

    # Both older drafts and not-yet-synchronized future drafts fail before LSP.
    for version in [1, 3, -1, 2147483647]:
        assert call("language_format", ok=False, path="Hello.java", version=version,
                    tab_size=4, insert_spaces=True)["code"] == "language_stale_version"
    for size in [0, 17, 4294967295]:
        assert call("language_format", ok=False, path="Hello.java", version=2,
                    tab_size=size, insert_spaces=True)["code"] == "language_error"
    formatted = call("language_format", path="Hello.java", version=2, tab_size=4, insert_spaces=True)
    assert formatted["value"][0]["newText"] == "// mock formatted\n"
    for include in [False, True]:
        references = call("language_references", path="Hello.java", line=0, character=0,
                          include_declaration=include)["value"]
        assert references[0]["uri"] == uri and "range" in references[0]
    for line, character in [(2147483648, 0), (0, 2147483648), (4294967295, 4294967295)]:
        assert call("language_references", ok=False, path="Hello.java", line=line,
                    character=character, include_declaration=True)["code"] == "language_error"
    symbols = call("language_document_symbols", path="Hello.java")["value"]
    assert symbols[0]["name"] == "Hello" and symbols[0]["children"][0]["name"] == "count"
    for path in ["../outside.java", "/absolute.java", "."]:
        for operation, params in feature_operations(path):
            assert call(operation, ok=False, **params)["code"] == "invalid_path"
    (root / "alias.java").symlink_to("Hello.java")
    for operation, params in feature_operations("alias.java"):
        assert call(operation, ok=False, **params)["code"] == "invalid_path"
    events = call("language_events")["value"]["events"]
    assert any(x["type"] == "diagnostics" for x in events)
    call("language_close", path="Hello.java")
    assert call("language_query", ok=False, path="Hello.java", line=0, character=0, kind="hover")["code"] == "language_document_closed"
    for operation, params in feature_operations():
        assert call(operation, ok=False, **params)["code"] == "language_document_closed"
    call("language_stop")
    for operation, params in feature_operations():
        assert call(operation, ok=False, **params)["code"] == "language_not_running"
    messages = [json.loads(line) for line in audit.read_text().splitlines()]
    formats = [m for m in messages if m.get("method") == "textDocument/formatting"]
    assert len(formats) == 1, "stale/invalid/closed format requests reached the server"
    assert formats[0]["params"] == {"textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": True}}
    references = [m for m in messages if m.get("method") == "textDocument/references"]
    assert len(references) == 2
    assert [m["params"]["context"]["includeDeclaration"] for m in references] == [False, True]
    assert len([m for m in messages if m.get("method") == "textDocument/documentSymbol"]) == 1
    assert not any(m.get("method") in ["workspace/executeCommand", "workspace/applyEdit", "textDocument/rename"] for m in messages)
    assert (root / "Hello.java").read_text() == original

    # Restarts reset open-document/version state. Unsupported static capabilities
    # are rejected locally rather than relying on errors from the peer.
    for mode in ["navigation-no-provider", "navigation-false-provider", "navigation-invalid-provider"]:
        unsupported_audit = root / (mode + ".jsonl")
        call("language_start", program=server, args=[mode, str(unsupported_audit)])
        for operation, params in feature_operations():
            assert call(operation, ok=False, **params)["code"] == "language_document_closed"
        call("language_open", path="Hello.java", language_id="java", version=2, text=original)
        for operation, params in feature_operations():
            result = call(operation, ok=False, **params)
            assert result["code"] == "language_error" and "unsupported" in result["message"].lower()
        call("language_stop")
        methods = [json.loads(line).get("method") for line in unsupported_audit.read_text().splitlines()]
        assert not any(method in methods for method in ["textDocument/formatting", "textDocument/references", "textDocument/documentSymbol"])
    call("language_start", program=server, args=[])
    call("language_open", path="Hello.java", language_id="java", version=42, text=original)
    assert call("language_format", ok=False, path="Hello.java", version=2,
                tab_size=4, insert_spaces=True)["code"] == "language_stale_version"
    call("language_format", path="Hello.java", version=42, tab_size=16, insert_spaces=False)
    call("language_stop")
    p.stdin.close()
    assert p.wait(timeout=3) == 0

    # Read-only language operations still require explicit workspace execution
    # trust, even when no server is running and a supplied path is invalid.
    requests = [{"id": i, "op": {"type": operation, **params}}
                for i, (operation, params) in enumerate(feature_operations("../outside.java"), 1)]
    untrusted = subprocess.run([agent, "--root", tmp], input="".join(json.dumps(r) + "\n" for r in requests),
                               text=True, capture_output=True, timeout=3, check=True)
    answers = [json.loads(line) for line in untrusted.stdout.splitlines()]
    assert len(answers) == 3 and all(a["result"]["Err"]["code"] == "run_disabled" for a in answers)
    assert (root / "Hello.java").read_text() == original
print("PASS: agent-side LSP lifecycle/completion/resolve/safe-navigation/format-version/references/symbols/capabilities/trust/disk-preservation")

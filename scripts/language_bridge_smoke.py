#!/usr/bin/env python3
"""Real process chain: test client -> stdio agent -> deterministic Rust LSP peer.
The peer is a protocol fixture, not a real Java/Kotlin language implementation.
"""
import json, pathlib, subprocess, sys, tempfile, time
agent=str(pathlib.Path(sys.argv[1]).resolve())
server=str(pathlib.Path(sys.argv[2]).resolve())
with tempfile.TemporaryDirectory(prefix="cedar-lsp-bridge-") as tmp:
    root=pathlib.Path(tmp); (root/"Hello.java").write_text("class Hello {}\n")
    p=subprocess.Popen([agent,"--root",tmp,"--allow-run"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    counter=0
    def call(op_type, ok=True, **kwargs):
        global counter
        counter+=1;p.stdin.write(json.dumps({"id":counter,"op":{"type":op_type,**kwargs}})+"\n");p.stdin.flush()
        answer=json.loads(p.stdout.readline());assert answer["id"]==counter
        result=answer["result"]
        assert ("Ok" in result)==ok, result
        return result.get("Ok",result.get("Err"))
    started=call("language_start",program=server,args=[])
    assert started["value"]["started"]
    assert call("language_start",ok=False,program=server,args=[])["code"]=="language_running"
    call("language_open",path="Hello.java",language_id="java",version=1,text="class Hello {}\n")
    call("language_change",path="Hello.java",version=2,text="class Hello { int count; }\n")
    assert call("language_change",ok=False,path="Hello.java",version=2,text="stale")["code"]=="language_error"
    completion=call("language_query",path="Hello.java",line=0,character=0,kind="completion")
    assert completion["value"]["items"][0]["label"]=="hello"
    assert call("language_query",path="Hello.java",line=0,character=0,kind="hover")["value"]["contents"]["value"]=="mock hover"
    assert call("language_query",path="Hello.java",line=0,character=0,kind="definition")["value"]["uri"].endswith("Hello.java")
    events=call("language_events")["value"]["events"]
    assert any(x["type"]=="diagnostics" for x in events)
    call("language_close",path="Hello.java")
    assert call("language_query",ok=False,path="Hello.java",line=0,character=0,kind="hover")["code"]=="language_document_closed"
    call("language_stop")
    # Restart in the same workspace proves resources/lifecycle reset.
    call("language_start",program=server,args=[]);call("language_stop")
    p.stdin.close();assert p.wait(timeout=3)==0
print("PASS: agent-side LSP start/open/change/stale-version/completion/definition/hover/diagnostics/close/stop/restart")

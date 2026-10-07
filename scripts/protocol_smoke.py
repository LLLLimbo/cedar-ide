#!/usr/bin/env python3
"""Black-box stdio smoke tests; creates no SSH keys and never contacts a server."""
import json, pathlib, subprocess, sys, tempfile, time, hashlib
agent = str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix="cedar-smoke-") as tmp:
    root=pathlib.Path(tmp); (root/"src").mkdir()
    p=subprocess.Popen([agent,"--root",tmp],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    counter=0
    def call(kind, **kwargs):
        global counter
        counter+=1
        p.stdin.write(json.dumps({"id":counter,"op":{"type":kind,**kwargs}})+"\n");p.stdin.flush()
        result=json.loads(p.stdout.readline());assert result["id"]==counter
        return result["result"]
    assert call("hello")["Ok"]["protocol"]==1
    content="class Hello { String greeting = \"你好\"; }\n"
    rev=call("write",path="src/Hello.java",text=content,expected_revision=None)["Ok"]["revision"]
    assert rev==hashlib.sha256(content.encode()).hexdigest()
    assert call("read",path="src/Hello.java")["Ok"]["text"]==content
    assert call("list",path="src")["Ok"]["entries"][0]["name"]=="Hello.java"
    assert call("search",query="你好",limit=10)["Ok"]["matches"][0]["line"]==1
    (root/"src/Hello.java").write_text("external edit\n")
    assert call("write",path="src/Hello.java",text="lost update",expected_revision=rev)["Err"]["code"]=="conflict"
    assert (root/"src/Hello.java").read_text()=="external edit\n"
    assert call("read",path="../outside")["Err"]["code"]=="invalid_path"
    assert call("run",program="echo",args=["no"],timeout_secs=1)["Err"]["code"]=="run_disabled"
    p.stdin.close();assert p.wait(timeout=3)==0
    # A malformed frame terminates cleanly without a stdout diagnostics leak.
    q=subprocess.run([agent,"--root",tmp],input="not-json\n",text=True,capture_output=True,timeout=3)
    assert q.returncode!=0 and not q.stdout and "protocol" in q.stderr
print("PASS: stdio hello/list/read/write/search/conflict/traversal/run trust/EOF/malformed frame")

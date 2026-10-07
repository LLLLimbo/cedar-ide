# Stdio workspace agent

Build with `cargo build -p cedar-agent --release`, then run:

```
cedar-agent --root /absolute/workspace/path
```

The agent accepts bounded newline-delimited JSON requests on stdin and responds
sequentially on stdout, preserving request IDs. All help/diagnostics go to stderr.
Clean EOF exits successfully. Malformed, incomplete, and oversized frames fail
closed and terminate the stream; ordinary operation errors return protocol errors
and allow the next request.

`--allow-run` explicitly enables arbitrary command execution with the current
account's permissions, including Git status because repository-configured filters
can execute code. It is not a sandbox. The default supports filesystem and search
requests while refusing both commands and Git status. Only Linux/macOS agents
support command execution and Git status. Other local agents, including Windows,
refuse them even with `--allow-run` until safe process containment is implemented.
A Windows frontend can use these features through a Linux/macOS agent over SSH.

SSH can transport this stream using normal OpenSSH authentication and host-key
verification. The agent itself opens no listening socket, stores no credentials,
and does not connect to remote servers. Install the binary on the workspace host
before requesting remote use.

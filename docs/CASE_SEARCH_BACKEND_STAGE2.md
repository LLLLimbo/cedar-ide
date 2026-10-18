# Case-search v1: bounded backend checkpoint

This checkpoint implements the independently negotiated backend operation from
`CASE_SEARCH_CONTRACT_STAGE1.md`. A normal agent advertises `case_search:{"version":1}`
alongside the unchanged `scoped_search:{"version":1}`. Both true and false modes
are implemented for programmatic clients. Application admission remains closed:
there is no Match case control or new UI route in this checkpoint. Protocol 4,
AgentInfo schema 1, package version 0.79.0, capability inventory and client gates
are unchanged. A future descriptor version still does not authorize v1.

## Literal semantics and original text

The matcher is built once per request from `regex::escape(query)`. It searches
UTF-8 lines and retains only the first original-text byte span per matching line.
Checked mode is exact literal matching; unchecked mode uses Unicode default
simple folding. Regex syntax, flags, captures, whole-word mode, normalization,
accent removal, locale/Turkic tailoring and full-fold expansion are not exposed.
Examples include K/k/K, S/s/ſ, Σ/σ/ς and ß/ẞ, but not ß/SS, I/İ/ı, or equivalence
between composed and decomposed accents. Basename masks remain case-sensitive.

The exact locked implementation is regex 1.13.1, regex-automata 0.4.18,
regex-syntax 0.8.11 and aho-corasick 1.1.5, reusing memchr 2.8.3. Only std,
unicode-case, perf-literal and perf-dfa are enabled on regex; its default features
are disabled. The dependency is introduced only in cedar-workspace.

The v1 folding table is Unicode 16.0.0, generated from status C+S equivalences,
excluding F and T. Its exact upstream source is
`regex-syntax-0.8.11/src/unicode_tables/case_folding_simple.rs`, 67,576 bytes,
SHA-256 `7622c7f7f03ac0dc2f2bcd51c81a217d64de0cc912f62f1add5f676603a02456`.
Do not silently change the table or these semantics under v1. Dependency updates
need renewed semantic, feature and notice review. This source-file size is not
linked size or a memory measurement.

The offset-taking preview helper crops original UTF-8 text, preserving the
2,048-byte window and optional ellipses. Query and match byte lengths may differ.
Long matches may be partly cropped; snippets do not prove current-disk freshness
or a match column. The legacy wrapper retains its exact query lookup and output.

## Resource and error policy

The request is validated before matcher construction and before filesystem
access. The query bound is 4,096 bytes; checked multiplication establishes an
8,192-byte escaped-pattern bound before allocation, and the actual escaped
length is also checked. The parser nesting limit is 8. The approximate size cap
is 8 MiB per NFA, and the lazy-DFA cache cap is 2 MiB per lazy-DFA engine. One
matcher can contain multiple NFAs and caches; these limits are not an aggregate
matcher, heap or RSS guarantee. Parser, pattern, literal extraction, scratch,
file, traversal, response and other allocations also exist.

Compilation exceeding the configured NFA limit returns ordinary application
error `case_search_matcher_limit`; other unexpected compilation failures return
`case_search_matcher_error`. Errors are fixed bounded messages without the query.
They do not trigger another operation, imply no matches, or fail an otherwise
healthy connection. A valid-sized query is not promised to compile within every
resource cap. Literal optimizations can avoid NFA construction; the unit-test
negative uses a verified complex folded literal and a test-only low NFA cap.
There is no runtime/environment override for shipping matcher limits.

A single five-second cooperative elapsed budget starts before compilation. It
is checked before/after compilation, at existing traversal/read/line boundaries,
and immediately after every case matcher call, including a final nonmatching
line. Successful compilation followed by observed expiry returns an empty
incomplete time_limit result with zero traversal/read counters. Build errors
precede the post-build elapsed observation. Compilation, one matcher call,
filesystem calls and bounded finalization are not forcibly preempted; this is
not a hard five-second completion or cancellation guarantee. The unchanged
client transport deadline is separate.

SearchCase shares the scoped walker: scope-first traversal; masks-before-open;
fixed exclusions; binary, invalid UTF-8 and oversize handling; symlink/reparse
checks; actual-read accounting; and the existing namespace-race limitations.
Bounds remain 20,000 entries, depth 64 below scope, 4,096 pending directories,
512 KiB pending path text, 4,096-byte paths, 1 MiB/file, 32 MiB actual read bytes,
1,000 effective rows and 512 KiB compact result encoding. The exact serialized
case boolean overhead is reserved before response-prefix cropping. Limits and
incomplete reasons are independent. Results are sorted/deduplicated as before;
a truncated subset is not promised globally best. There is no pagination.

## Notices and verification boundaries

The exact locked graph has four new packages. Nine new upstream notice files
are bundled, including the nested regex-syntax Unicode notice at
`third-party-licenses/regex-syntax-0.8.11/LICENSE-UNICODE`. The collector requires
its reviewed package identity, fixed canonical source path, 2,847-byte length
and SHA-256 `74db5baf44a41b1000312c673544b3374e4198af5605c7f9080a402cec42cfa3`.
It fails on missing, redirected or altered evidence and does not recursively
collect arbitrary nested files. Existing bundle notice allowlists are unchanged.
Exact new notice bytes are preserved across checkout line-ending conversions.

Focused unit tests exercise folding/literals/original offsets, maximum queries,
checked parity, scope/mask/open instrumentation, independent budgets, encoded
boolean overhead and compile/match deadline boundaries. Existing scoped tests
continue to cover the shared walker and read-error/growth cases. A separate
normal-agent programmatic suite uses the production client and stdio agent:

    cargo build -p cedar-agent --locked
    CEDAR_AGENT_BIN=/absolute/path/to/cedar-agent cargo test -p cedar-client \
        --test case_search_roundtrip --locked -- --ignored

Those process tests are explicitly ignored by ordinary all-targets execution and
must be run separately. They do not implement or claim the Stage 4 GUI ledger,
native UI, real SSH, Windows/macOS journeys, benchmarks, memory measurements or
release acceptance. Exact-source CI and package/native evidence remain separate
from local tests. Frozen old-reader sources and historical legacy receipts are
unchanged.

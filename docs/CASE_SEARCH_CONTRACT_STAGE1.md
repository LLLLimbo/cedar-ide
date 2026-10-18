# Case-search v1: inert contract checkpoint

This checkpoint adds protocol and client admission only. The workspace advertises
no `case_search` descriptor and returns `unsupported_operation` immediately for
`SearchCase`, without directory traversal or content opens. The application
validates a present descriptor but rejects every attempted `SearchCase`. Neither
Project Text surface exposes a case option. Case-insensitive searching is not
implemented by this checkpoint.

Protocol 4, AgentInfo schema 1, the 32 direct capabilities and two capability
groups, file discovery, scoped-search v1, package version 0.79.0, legacy default
routes, and existing source dependencies remain unchanged.

## Frozen wire contract

`case_search` is an optional Hello member. Absence/null means unavailable. A
present descriptor is exactly an object containing a required positive u32
`version`; only version 1 enables the client operation. Well-formed future
versions are valid, unsupported metadata. Malformed descriptors fail Hello.
The client retains the first validated Hello for the connection, with no probe,
retry, fallback, flat-capability inference or later-Hello upgrade/downgrade.

`search_case` carries exactly five required request members: `query`, `scope`,
`masks`, `limit`, `case_sensitive`. The boolean is never defaulted or coerced.
The request reuses unchanged scoped-v1 query/scope/mask/limit validation. Mask
matching remains case-sensitive. The exact requested limit is echoed even when
its effective result cap is 1,000. The case flag does not authorize an operation
without the independent descriptor, and scoped-search support is not a
prerequisite for a conforming case-search peer.

`case_matches` carries exactly `request`, `matches`, `incomplete`, `reasons`,
`examined_entries`, `read_bytes`. New strict rows contain exactly `path`, `line`,
`text`; legacy SearchMatch decoding stays unchanged. New descriptor, request,
result and row objects reject positional arrays, unknown/missing/duplicate
members, and wrong types, including escaped duplicate key spellings.

Response admission checks every echoed request component, exact path/scope,
case-sensitive masks, sorting/uniqueness, bounds and incomplete reasons. The
512 KiB compact result budget includes the case flag and JSON escaping. Scoped
row/counter/reason validation is reused after bounding the new result encoding.
A cropped snippet need not contain the full query. This does not prove match
correctness, current-disk freshness, a match column or Unicode behavior.

Semantically malformed/wrong-payload replies fail the process connection with
`protocol_error`; framing/strict decoder failures retain the existing
`transport_read` classification and also fail the connection. Ordinary remote
application errors do not disconnect or trigger fallback. Connection-wide
cancellation includes this read-only operation and preserves the existing
30-second default transport deadline. No query-edit cancellation is introduced.

## Compatibility and evidence boundaries

The compatibility integration test compiles the exact four protocol modules from
commit `705457c6ac67cc6e7be62490e17b5d2277af0d08`, tree
`569ae835d76e908e1d6876d58366a3f1046dde2a`. The frozen reader executes its own
frame reader/writer, AgentInfo validation and scoped-v1 request/result types.
Current production Hello serialization is read by that old implementation;
scoped operations round-trip both ways; a deliberately supplied new operation
is rejected. The existing accepted_061 fixture is unchanged.

Client process fixtures prove local unsupported/invalid gates, first-Hello
retention, strict replies, cancellation/deadline behavior and ordinary-error
survival. Workspace tests prove unadvertised immediate rejection, including
Linux kernel zero directory/content-open/access observations. Application tests
prove v1/future metadata cannot enable a partial route. These are contract and
inert-admission tests, not completed matching, native GUI or real-SSH evidence.

## Changes by purpose

Semantic production changes are limited to protocol types/variants, client
negotiation and validation, workspace unadvertised dispatch rejection, and app
startup/admission barriers. Required typed Hello constructors across app/client/
protocol tests add only `case_search: None`. Test registrations and the fixture
`.gitattributes` entry are mechanical. Existing scoped source, legacy acceptance
scripts and dependency locks are preserved. The checkpoint receipt separately
lists all changed files and identifies mechanical-only edits.

Backend matcher/dependency/Unicode-table verification, bounded traversal,
case-option UI ownership and exact end-to-end/native acceptance remain later
stages. No matcher library, UI control, advertised operation, release tag or
version bump belongs to this inert checkpoint.

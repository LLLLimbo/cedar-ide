# Verification report · bounded capability-group compatibility / 0.38.0

The [0.37 Linux Java checkpoint](TEST_REPORT_PHASE37_LINUX_JAVA.md) is fully verified. This checkpoint adds only optional metadata-reader compatibility; shipping agents retain their exact flat capability inventories and omit the new field. Linux Maven remains disabled.

## Contract

Protocol 4, AgentInfo schema 1, and the 32-name flat limit remain unchanged. At most two unique 64-byte ASCII group identifiers are accepted, with 128 content bytes and 135 compact-array bytes maximum. Exact versioned groups map only to the existing Maven core pair and optional dependency operation. Unknown well-formed identifiers are inert; malformed groups fail metadata validation. Direct/group overlap is idempotent and never flattens the inventory.

Structural claims do not bypass trust, lifecycle, provider, session, startup/POM identity or backend platform checks. Unsupported profiles remove direct and grouped Maven claims. No discovery roundtrip, command, cache preparation, Linux Maven activation or new runtime tool is introduced.

## Required verification

Frozen 0.37 typed-reader deserialization must retain all previous Linux flat support and ignore groups; new readers must preserve old Windows direct claims and metadata-free file-only behavior. Controlled peers must prove one immutable Hello, no extra requests for refused operation families, ordinary reads after refusal and connection isolation. Frontend tests retain trust/Ready/session/POM guards. Current shipping Hello must omit groups on all supported configurations, and both packages retain exact inventories.

Local formatting and strict host/MSVC Clippy passed. The complete Rust aggregate passed 1,399 tests across 45 suites, with 48 explicit opt-ins ignored. This includes 11 protocol group tests, seven new frontend tests, controlled-pipe client compatibility cases and shipping-profile omission/filter tests. Targeted Python suites passed: six capability, two export, 30 Linux package, 35 Windows package with one platform skip, and 28 Linux Java driver tests. No real Maven execution is implied by synthetic peer responses.

The rebuilt default-feature Linux agent also passed actual stdio capability/trust checks: 31 flat names, no group field, no tool startup and unchanged support with trust toggled. Local release verification covered that agent; exact-source dual CI and both complete package builds are pending. Existing Java, Maven, ownership, file-protection and distribution gates remain required. No resource, native Windows GUI or SSH interoperability claim is added.

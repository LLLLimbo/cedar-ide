# Frozen pre-case-search protocol reader

These four Rust modules are byte-for-byte copies of public commit
`705457c6ac67cc6e7be62490e17b5d2277af0d08` (package 0.79.0), tree
`569ae835d76e908e1d6876d58366a3f1046dde2a`. The complete baseline tree was
independently recomputed before copying. PROVENANCE.json records byte lengths,
SHA-256 and Git blob identities.

The case_search_compatibility integration test compiles the unchanged modules,
including their own read_frame, write_frame, AgentInfo::validate, scoped v1
request/result validation, Serde enums and original unit tests. Its crate-root
aliases resolve the unchanged modules' original crate-relative imports.
Current production serialization is passed through this actual old reader.
The existing accepted_061 fixture and its source remain unchanged.

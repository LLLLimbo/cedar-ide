# Verified recovery of the phase-5 source checkpoint

The development environment was replaced immediately after the audited phase-5
source objects had been uploaded. The prior checkout, local build cache, native
screenshots and private raw evidence were not available in the new environment.
No missing source was recreated from memory.

The publisher recovered the complete source tree from the prior phase-4 tree
and four uploaded change trees. Two newly named report copies reused existing
blob contents. The resulting **654-file** tree exactly matched the root tree
SHA recorded before the environment change:

`4f605092976d5a681191246a35da2c69f26a58bb`

Only after that exact match was established was phase 5 appended to the public
history as
[`119d5c30ddba51cbfa9f04d5ae6e281103a3803f`](https://github.com/LLLLimbo/cedar-ide/commit/119d5c30ddba51cbfa9f04d5ae6e281103a3803f).
A fresh anonymous clone/fetch in the replacement environment confirmed the same
commit and tree. There was no force push, credential transfer or reconstructed
source content.

[The exact-commit CI run](https://github.com/LLLLimbo/cedar-ide/actions/runs/37643452204)
passed on both Ubuntu and Windows. Windows exercised the new ordinary-file
held/concurrent-reader, restrictive sharing, read-only and temporary-attribute
regressions, plus transport fault tests, a release build and separate-process
integration. This does not establish Windows native GUI acceptance.

The source was also rebuilt in the new Linux environment using the official
Rust 1.99.0 toolchain and locked dependencies. The full verification script
passed again: **419 Rust tests** (413 ordinary plus six explicitly selected
integration tests), four Python agent chains, formatting and strict Clippy.
Nine tests were marked ignored in the ordinary aggregate; six were then run
explicitly. The remaining three opt-ins were not counted as passes.

The earlier native/JDT evidence remains described in the contemporaneous
phase-5 report, but its excluded raw files cannot be reproduced merely by
restoring public source. A later rebuild is not the same binary artifact and
must not inherit an earlier binary hash or native acceptance claim. Subsequent
native evidence and measurements require a new run.

Future checkpoints continue through reviewed, ordinary commits and exact-tree
verification. Public source and CI are durable records; a local checkout or
in-progress archive alone is not a backup guarantee.

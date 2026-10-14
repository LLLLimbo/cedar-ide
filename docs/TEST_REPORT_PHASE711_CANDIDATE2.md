# 0.71.1 candidate 2 — canonical license-test root

## Candidate 1 outcome

[CI 38516905479](https://github.com/LLLLimbo/cedar-ide/actions/runs/38516905479)
ran public commit `1c28e9e175ab7261e489e02279952a7f4457ed85`.
Linux succeeded: 50 Rust suites, 2,235 passed, zero failed, 71 ignored, and all
four strict normal-agent process proofs. Windows passed the public-export tests
but failed the next license-collection test before its content assertions.
The expected mock cwd used `RUNNER~1`, while the collector correctly resolved
the same directory to `runneradmin`. The dependent JDT job was skipped.
Five artifact metadata entries existed; their payloads were not downloaded or
accepted as a complete three-package release. Candidate 1 and its evidence remain
immutable. This second candidate keeps the unreleased version **0.71.1**.

## Exact oracle and regression coverage

Only `scripts/test_collect_licenses.py` changes outside reports. The fixture
saves `expected_root = root.resolve()` independently from the collector, but
retains the original temporary-directory spelling when invoking the script.
The mock still requires exactly one call with the original Cargo argument list
and exactly that canonical cwd. Collector production code is unchanged.

A second case invokes the actual collector through an existing owned `route/..`
entry path. With the old cwd oracle this case fails while the ordinary case
passes. With the corrected oracle both cases pass. This is a portable test of
canonical-directory semantics; it is not a claim that Linux reproduced Windows
8.3 names. The next Windows CI run must exercise its actual temporary paths.
No symlink privilege, system setting or path-normalization shortcut is used.

Both cases perform two actual regenerations. Existing complete OFL text,
Surefire fixture notice, dependency entry/count and copied LICENSE assertions
remain. New checks also require identical notice bytes on regeneration,
unchanged font manifest/license bytes, and exact copied dependency-license bytes.
No TTF, exporter allowlist, license hash or line-ending rule changes.

## Verification and explicitly reused evidence

- New Python run: 30 suites, 821 tests run, **811 passed, ten skipped**, zero
  failed suites. Collector: two passed, zero skipped. Public export: 15 passed.
- The old-oracle negative case is preserved with one pass and one failure.
- Of 899 non-Markdown source inputs, 898 are byte/mode identical to candidate 1;
  the only difference is the license-collector test. All Rust production/test
  code, fixtures, embedded assets, Cargo manifests/lock/toolchain files and
  retained-proof runtime/validator scripts are unchanged.
- Candidate 1 local Rust, Clippy, formatting, real CJK and four pinned proof
  receipts are reused only through this explicit cross-commit input-equivalence
  audit. Their original checkout paths, executable hashes and source binding
  remain intact. They are **not** relabelled as candidate 2 executions.
- Candidate 1's successful Linux CI is retained with its actual source commit.
  Candidate 2 has not yet passed its own CI, package verification or native GUI.

No Rust rebuild, Rust test, process proof or GUI was run for this test-only
candidate. It adds no UI feature, visual parity claim or search-dialog redesign.

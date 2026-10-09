# Verification report · Bundle guide integrity / 0.28.1

This narrow checkpoint includes the Java implementation-navigation guide in the
Windows development bundle and verifies local links in its two entry guides.
There are no Rust product-source or execution-policy changes.

## Prior native result and packaging defect

[0.28 implementation verification](TEST_REPORT_PHASE28_IMPLEMENTATIONS.md)
records the successful native implementation-location acceptance. Exact public
source `f6be7c9500ecd32a2a2ee21b672b0328f954179e` passed both OS jobs in
[CI 37940287794](https://github.com/LLLLimbo/cedar-ide/actions/runs/37940287794).
The Windows JDT receipt reported two type locations, one method location and an
empty negative query, with all 24 required witnesses true in 16,729 ms.

Independent ZIP inspection then found that the quickstart linked
`JAVA_IMPLEMENTATIONS.md`, but the explicit packaging inventory omitted it.
Existing payload hashes were valid; that did not establish a complete usable
package. The original ZIP remains an incomplete distribution checkpoint.

## Repair and regression scope

- The guide is mandatory in the source, manifest and ZIP inventories.
- The quickstart and implementation guide's supported simple inline local links
  must resolve to exact payload names, including the manifest when referenced.
  External URLs are not fetched; remote content and fragment anchors are not
  validated. This is a bounded entry-guide check, not a general Markdown parser.
- Verification runs before bundle output is created and before extraction.
- Regressions cover the actual guides, missing or corrupt guide bytes, invalid
  UTF-8, manifest-consistent broken local links, case/path/query confusion,
  fragments, external URLs, malformed large inputs and rejection without output.

## Current verification

Independent packaging review has no remaining findings. The packaging Python
suite ran 36 tests: 35 passed and one platform-specific case was skipped.
Strict host and Windows MSVC-target Clippy pass. The all-target host aggregate
completed 40 suites: 1,100 passed, zero failed and 29 explicit opt-ins ignored.
All-feature and default shipping release builds pass. Exact-source native
CI/package verification remains pending.
The new ZIP must contain the guide and pass all payload/provenance checks before
this correction is accepted. All existing Java, Maven, ownership and trust-off
bundle gates remain required.

The latest fully verified distribution remains 0.27 until that verification.
Windows native GUI, authenticated SSH and full IDEA equivalence remain separate
unmet validation or feature boundaries. The known JDT diagnostic-publication
limitation and explicit recovery distinctions remain unchanged.

## Exact 0.28.1 outcome

Public `7f556e3cd8fd79f887db73936de14bced2911d81` passed [both OS CI jobs](https://github.com/LLLLimbo/cedar-ide/actions/runs/37944324249). Independent verification confirmed all 534 payload hashes plus the manifest, exact implementation-guide bytes and every entry-guide local target. The ZIP is 5,065,378 bytes, SHA-256 `efd66d258af2e7f4a36815af6d6fa406d7a8be770d74f3b84618f43902009d74`.

The implementation witness passed with two type results, one method result and zero negative results; all 24 required flags were true in 17,631 ms. Maven present/missing acceptance passed in 31,702 ms. Ordinary Java session 2 retained its failed spontaneous correction wait (60,045 ms) and then passed the workflow after exactly one supported refresh with an unversioned witness. This is recovery evidence, not an upstream fix or a spontaneous pass. The separate required idle workflow matched spontaneously in 40,044 ms with zero recovery. Native Windows GUI and authenticated SSH remain unverified.

# Cedar modification notice for egui 0.31.1

This source tree contains a modified copy of official egui 0.31.1, licensed under
MIT OR Apache-2.0. It is not an upstream egui release. Cedar's relative Cargo patch
selects this same-version dependency; no dependency version upgrade is implied.

Upstream: https://github.com/emilk/egui, tag 0.31.1.
Archive: https://static.crates.io/crates/egui/egui-0.31.1.crate
Archive SHA-256: 25dd34cec49ab55d85ebf70139cb1ccd29c977ef6b6ba4fe85489d6877ee9ef3.
Archive VCS commit: 1669e52a7ccfc3489c1b0999b9ed48894a0b3887, path crates/egui.

The complete 92-member registry archive was copied before modification. Exactly
three upstream members are changed: src/widgets/text_edit/builder.rs, mod.rs, and
output.rs. The small patch adds per-invocation, payload-free IME branch receipts;
native acceptance, text mutation, event filtering/order and cursor behavior remain
upstream's responsibility. Each changed file carries a modification notice.

The exact upstream delta is egui-observation.patch. vendor-upstream.json records
all original archive-member hashes and the three patched-member hashes. These
records identify the accepted 2026-10-15 observation patch and are included in the
source export. They do not claim OS-IME, GUI or cross-platform runtime validation.

LICENSE-MIT and LICENSE-APACHE are the unmodified official upstream texts. The
registry crate does not contain these workspace-root files, so they were retrieved
from both tag 0.31.1 and the archive VCS commit. license-provenance.json records the
URLs, lengths, hashes and byte equality of each pair. Copies in
third-party-licenses/egui-0.31.1 belong to this one logical dependency and accompany
Cedar binary bundles. No second egui version or additional license identity is
introduced. Other dependencies retain their original upstream notices.

# Windows recovery descriptor feasibility

This is a nonshipping, generated-fixture investigation. It does not enable a
new recovery policy, modify permissions, or establish that an existing user's
recovery data was exposed. [Current recovery limitations](RECOVERY.md) remain.

## Why inspect

The recovery crate's Windows permission check currently accepts ordinary files
without examining their owner or discretionary access control list (DACL).
Windows object identity comparison currently checks object kind only. Reparse
point checks already exist. A new temporary is checked before writing its
header and plaintext draft, but that Windows check does not establish ACL
privacy. These are documented assurance gaps, not evidence of an incident.

## Fixed nonshipping scope

One explicitly selected Windows integration test inspects two fresh generated
roots: one below the CI temporary directory and one fresh Local AppData child.
It creates only empty ordinary objects with normal inherited descriptors and
renames one empty temporary to a record-like name. It never calls `Store::write`
or writes draft/header content. No existing user files are inspected.

The driver admits one native invocation with a 60-second watchdog. Compilation
and exact-one test selection have separate bounded preparation budgets. A
timeout, inspection failure or cleanup failure is recorded as a failure; there
is no retry or permission repair. A successfully inspected descriptor that the
candidate policy rejects is a valid observation, not a forced acceptance.

Descriptor inspection uses actual open handles, a caller-sized
`GetKernelObjectSecurity` owner/DACL query,
effective token identity, file type/reparse/link information, volume plus
128-bit file identity, and filesystem ACL capability. Descriptor parsing is
bounded to 64 KiB and 128 ACEs. Oversize is rejected before buffer allocation;
a descriptor-growth race fails without a retry. Receipts use fixed categories, counts and
booleans, never raw SIDs, descriptors, account names or profile paths.

The 0.43 observation failed during the empty rename in both generated roots.
Eight completed descriptor observations rejected the candidate because their
owners differed from the effective token user. The receipt did not identify
those owners or independently report the DACL verdict. It also did not capture
the native rename error, so its historical cause is unconfirmed. Cleanup
completed and no recovery payload bytes were written.

The 0.43.1 diagnostic correction allows write sharing only on the held generated
destination-root handle, while still denying delete sharing. Empty file handles
still deny write sharing; ancestor handling is unchanged. Windows can reopen
the destination directory with write access during rename, which supplies a
source-supported reason for this narrowly scoped correction. This is a handle
sharing change in the test, not an ACL or production storage-policy change.
The corrected receipt separately classifies owners as effective user, System,
Administrators or other, preserves an independent DACL verdict, and uses fixed
rename error categories. The identity observation describes the reopened renamed
alias compared with the captured original file, independently of earlier chain
checks. It and source verification distinguish checks that did not run from
completed checks. A failed native invocation remains
failed even when its subsequent source verification succeeds.

There are no ACL setters, custom on-disk security descriptors, ownership or
privilege changes, account lookups, installs, remote operations or explicit network
requests in this probe. Roots must report a fixed local drive; this is not a
host-level network-isolation claim. The existing locked Windows API dependency is reused.

## Candidate policy, not production enforcement

The candidate requires an owner equal to the effective token's user SID and
understood ordinary allow entries restricted to that user, LocalSystem and
BUILTIN Administrators. Inherited and directory inherit-only grants are also
inspected. Ordinary deny entries never rescue a foreign allow. Missing, NULL,
empty, malformed or unsupported DACLs are rejected with distinct categories.
An empty DACL is not described as public: it grants no ordinary discretionary
access, but is outside this usable-policy subset.

Memory-only tests cover foreign owners and grants, broad groups, inheritance,
deny/allow order, symbolic trustees, unsupported ACE forms and malformed bounds.
A fake payload sink verifies that rejection or inspection error occurs before
any metadata or body bytes are admitted. These tests do not alter disk ACLs.

This is deliberately stricter than a full effective-access calculation. An
access check for one token cannot establish that every other possible principal
lacks access. File identity detects substitutions at observation points; it
does not prevent later ancestor renames or concurrent descriptor changes.
Protection against hostile same-account or administrator actors is not claimed.

## Completed observation and routine CI

The single corrected 0.43.1 observation is recorded in the
[exact-source report](TEST_REPORT_PHASE431_DESCRIPTOR.md). Both roots completed
inspection and cleanup, while the strict owner policy rejected every object.
This did not enable production enforcement. Starting with 0.44, ordinary CI
keeps the memory-only parser and fixed driver tests but does not invoke the
native descriptor investigation again. Its ignored native test and driver remain
available for a separately reviewed investigation; no automatic retry or ACL
repair is introduced.

## Decision after evidence

Native results inform a separate compatibility and production-policy review.
Any future enforcement must validate root, lock, existing record and every
still-empty temporary through its own handle before reading or writing recovery
plaintext. Unsupported storage must visibly pause recovery while preserving
editor text and existing records. It must not silently chmod, migrate, delete
records or fall back to another plaintext directory. No such behavior is
introduced by this checkpoint.

## Official API references

- [Caller-sized GetKernelObjectSecurity](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getkernelobjectsecurity)
- [Token information classes](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_information_class)
- [DACL retrieval and NULL/empty distinction](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptordacl)
- [ACE inheritance rules](https://learn.microsoft.com/en-us/windows/win32/secauthz/ace-inheritance-rules)
- [File security and access rights](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights)
- [128-bit file identity](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)
- [Rename target-directory access](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)
- [CreateFile sharing rules](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)

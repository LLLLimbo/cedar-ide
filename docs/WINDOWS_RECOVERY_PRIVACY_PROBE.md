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

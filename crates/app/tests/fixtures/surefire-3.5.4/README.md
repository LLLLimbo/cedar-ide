# Apache Maven Surefire report fixture

`enclosed-error.xml` is an unchanged 869-byte upstream report-parser regression
fixture from Apache Maven Surefire 3.5.4, commit
`88513d8b8dfef3c00794e2ae5976cb1d4368ca3d`:

https://github.com/apache/maven-surefire/blob/88513d8b8dfef3c00794e2ae5976cb1d4368ca3d/maven-surefire-report-plugin/src/test/resources/unit/surefire-report-enclosed-trimStackTrace/surefire-reports/TEST-surefire.MyTest-enclosed-trimStackTrace.xml

Git blob: `d5e68a2129a9257a5f8241195121e8ec27296ff2`.
SHA-256: `3fd206e8ee8dc198a7e0adc960ec99a6a44f80ffeab025f4851d414feb21af2e`.
The bytes have no trailing newline and are protected from checkout line conversion.
This establishes compatibility with an upstream report fixture; it is not evidence
that Cedar executed Surefire, JUnit or a Maven goal. Its public runtime-property
entry is intentionally discarded by Cedar's parser. Test names and stack text are
upstream fixture data, not user reports.

Licensed under Apache License 2.0; a copy is retained in
[the license directory](../../../../../third-party-licenses/apache-maven-surefire-3.5.4/LICENSE).
The pinned repository has no root NOTICE file. No upstream binary is bundled.

# Formatting, references and outline fixture

A synthetic two-file Eclipse Java project, with no Maven/Gradle build scripts.
A Java 21 JDK and separately installed JDT LS are needed. Source text contains
Chinese; the frontend can load a suitable existing system font on demand.

The opt-in `scripts/jdt_navigation_smoke.py` copies this fixture to an isolated
temporary directory. It opens deliberately unformatted **unsaved** Greeter text
and an unsaved Main with two greeting calls, then checks actual formatting,
references, outline, version checks and confined navigation. It must leave both
source files on disk unchanged. JDT may write its own project metadata/build
outputs in that temporary workspace; those are not source-save operations.

Use copies for native manual QA too. Language features require explicitly
trusting this synthetic workspace and starting the installed server. No server
or JDK is bundled. This fixture does not establish large-project compatibility.

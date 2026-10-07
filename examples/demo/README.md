# Cedar demo workspace

Open this directory in Cedar. Navigate to `src`, open both files, edit, save,
search for `message`, then change the same file externally to exercise a safe
save conflict. No project task runs automatically.

Java: enable trusted command execution, enter executable `javac`, arguments
`["-d", "out", "src/Main.java"]`, then execute `java` with
`["-cp", "out", "demo.Main"]`. A JDK must be installed on the workspace machine.
Kotlin compilation requires an externally installed Kotlin compiler.

# Java language-service demonstration

This synthetic Eclipse Java project intentionally starts with two semantic issues:
`GregorianCalendar` has no import and a String is assigned to an int.

With a separately installed JDT LS server and JDK21:

1. Connect to this directory with trusted tool execution enabled
2. Start the JDT LS stdio server in the Language panel, with language ID `java`
3. Open `src/Main.java`; auto-sync should produce real diagnostics
4. Place the caret after `Grego` inside `GregorianCalendar`, press Ctrl+Space,
   choose `GregorianCalendar - java.util`, and apply its completion
5. The import and primary completion edit should be one undoable draft operation
6. Ctrl+Z should undo both; Ctrl+Shift+Z should restore both
7. Change `"oops"` to `42`; that type diagnostic should disappear after sync
8. F12 on the `greeting` reference should jump to its declaration

No build or server is started automatically. The editor does not execute the
server's optional completion callback or silently save completion changes.

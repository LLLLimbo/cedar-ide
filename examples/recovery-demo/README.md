# Recovery demonstration

This synthetic workspace is used to verify private local draft recovery.
Open note.txt, edit without saving, wait for the exact recovery acknowledgement,
then terminate the test editor process unexpectedly. On restart, explicitly
review and restore the recovery. It must remain an unsaved draft; disk must not
be overwritten. If note.txt changed externally, saving the restored old revision
must produce a conflict.

Use a separate CEDAR_RECOVERY_DIR for destructive test runs. Only terminate a
process you started for this test; do not target other users' editors or data.

For asynchronous command testing, explicitly trust this synthetic workspace and
run executable `sh` with literal JSON arguments `["run-demo.sh"]` and a 300-second
timeout. It prints once then sleeps for 180 seconds. While it runs, open/edit/save
note.txt, then press Cancel. Confirm a terminal Cancelled state before reconnecting.
The script has no network access or dependencies beyond the POSIX shell and sleep.

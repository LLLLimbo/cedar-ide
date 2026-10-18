use super::*;
use cedar_protocol::CaseSearchRequest;

fn request(case_sensitive: bool) -> Operation {
    Operation::SearchCase {
        request: CaseSearchRequest {
            query: "needle".into(),
            scope: String::new(),
            masks: vec![],
            limit: 1000,
            case_sensitive,
        },
    }
}

#[test]
fn case_search_is_unadvertised_and_unsupported_even_without_a_root() {
    let root = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    let hello = workspace.handle(Operation::Hello).unwrap();
    assert!(
        matches!(hello, Payload::Hello {case_search:None, scoped_search:Some(d), ..} if d.supported())
    );
    let wire = serde_json::to_value(hello).unwrap();
    assert!(wire.get("case_search").is_none());
    root.close().unwrap();
    for trust in [false, true] {
        workspace.set_allow_run(trust);
        for case_sensitive in [false, true] {
            assert_eq!(
                workspace.handle(request(case_sensitive)).unwrap_err().code,
                "unsupported_operation"
            );
        }
        let mut invalid = request(false);
        let Operation::SearchCase { request } = &mut invalid else {
            unreachable!()
        };
        request.query.clear();
        request.scope = "../outside".into();
        request.limit = 0;
        assert_eq!(
            workspace.handle(invalid).unwrap_err().code,
            "unsupported_operation"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn case_search_stage1_produces_zero_directory_or_content_open_access_events() {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/sentinel.rs"), "needle\nNEEDLE\n").unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    // Linux's kernel reports actual directory/content opens and accesses. Set
    // watches only after fixture/workspace construction, then make no test I/O
    // until both unsupported requests have returned.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    assert!(fd >= 0);
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    for path in [
        root.path().to_path_buf(),
        root.path().join("src"),
        root.path().join("src/sentinel.rs"),
    ] {
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let watch = unsafe {
            libc::inotify_add_watch(
                fd.as_raw_fd(),
                path.as_ptr(),
                libc::IN_OPEN | libc::IN_ACCESS,
            )
        };
        assert!(watch >= 0);
    }
    for case_sensitive in [false, true] {
        assert_eq!(
            workspace.handle(request(case_sensitive)).unwrap_err().code,
            "unsupported_operation"
        );
    }
    let mut events = [0u8; 4096];
    let count = unsafe { libc::read(fd.as_raw_fd(), events.as_mut_ptr().cast(), events.len()) };
    assert_eq!(count, -1, "unexpected filesystem activity: {count} bytes");
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::EAGAIN)
    );
}

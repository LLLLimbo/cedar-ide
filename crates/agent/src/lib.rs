//! Sequential stdio service. Protocol frames are the only stdout output.
use cedar_protocol::{read_frame, write_frame, Request, Response};
use cedar_workspace::Workspace;
use std::io::{self, BufRead, Write};

pub fn serve<R: BufRead, W: Write>(
    workspace: &mut Workspace,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    while let Some(request) = read_frame::<_, Request>(reader)? {
        let response = Response {
            id: request.id,
            result: workspace.handle(request.op),
        };
        write_frame(writer, &response)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cedar_protocol::Operation;

    struct BrokenOutput;
    impl Write for BrokenOutput {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test peer closed stdout",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_acknowledgement_can_follow_a_commit_but_stops_queued_mutations() {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(root.path()).unwrap();
        let mut input = Vec::new();
        for (id, path) in [(1, "first.txt"), (2, "must-not-run.txt")] {
            write_frame(
                &mut input,
                &Request {
                    id,
                    op: Operation::Write {
                        path: path.into(),
                        text: "committed before acknowledgement".into(),
                        expected_revision: None,
                    },
                },
            )
            .unwrap();
        }
        let error = serve(&mut workspace, &mut &input[..], &mut BrokenOutput).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(
            std::fs::read_to_string(root.path().join("first.txt")).unwrap(),
            "committed before acknowledgement"
        );
        assert!(!root.path().join("must-not-run.txt").exists());
    }

    #[test]
    fn ordinary_request_error_is_answered_and_next_request_keeps_its_id() {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(root.path()).unwrap();
        let mut input = Vec::new();
        for request in [
            Request {
                id: 20,
                op: Operation::Read {
                    path: "missing.txt".into(),
                },
            },
            Request {
                id: 21,
                op: Operation::Hello,
            },
        ] {
            write_frame(&mut input, &request).unwrap();
        }
        let mut output = Vec::new();
        serve(&mut workspace, &mut &input[..], &mut output).unwrap();
        let mut reader = &output[..];
        let failure: Response = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(failure.id, 20);
        assert!(failure.result.is_err());
        let success: Response = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(success.id, 21);
        assert!(success.result.is_ok());
        assert!(read_frame::<_, Response>(&mut reader).unwrap().is_none());
    }
}

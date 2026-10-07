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

//! Wire framing.
//!
//! The protocol is asymmetric: requests go out as bare CBOR with no length
//! prefix, while responses arrive as an 8-byte big-endian length followed by
//! that many bytes of CBOR.

use std::io::{Read, Write};

use crate::error::{Error, Result};
use crate::protocol::{CommandWithArgs, Response};

/// The server reads each request into a fixed 1024-byte buffer with a single
/// `read`, so anything longer is truncated and the connection is dropped.
pub(crate) const MAX_REQUEST_BYTES: usize = 1024;

/// Upper bound on a response we will buffer, so a corrupt length cannot make us
/// allocate unbounded memory.
pub(crate) const MAX_RESPONSE_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn encode_request(command: &CommandWithArgs) -> Result<Vec<u8>> {
    let bytes = serde_cbor::to_vec(command).map_err(Error::Encode)?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Error::RequestTooLarge {
            size: bytes.len(),
            max: MAX_REQUEST_BYTES,
        });
    }
    Ok(bytes)
}

pub(crate) fn write_request<W: Write>(writer: &mut W, command: &CommandWithArgs) -> Result<()> {
    let bytes = encode_request(command)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn read_response<R: Read>(reader: &mut R) -> Result<Response> {
    let mut length = [0u8; 8];
    reader.read_exact(&mut length)?;
    let length = u64::from_be_bytes(length);

    if length == 0 {
        return Ok(Response::QuietOk);
    }
    if length > MAX_RESPONSE_BYTES {
        return Err(Error::ResponseTooLarge {
            size: length,
            max: MAX_RESPONSE_BYTES,
        });
    }

    let mut body = vec![0u8; length as usize];
    reader.read_exact(&mut body)?;
    serde_cbor::from_slice(&body).map_err(Error::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{DBCommand, QueryResult, SystemCommand};

    fn framed(response: &Response) -> Vec<u8> {
        let body = serde_cbor::to_vec(response).unwrap();
        let mut out = (body.len() as u64).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn requests_are_written_without_a_length_prefix() {
        let command = CommandWithArgs::db(DBCommand::ExecuteSQL, vec!["SELECT 1".into()]);
        let mut out = Vec::new();
        write_request(&mut out, &command).unwrap();
        assert_eq!(out, serde_cbor::to_vec(&command).unwrap());
    }

    #[test]
    fn oversized_requests_are_rejected_before_sending() {
        let command = CommandWithArgs::db(DBCommand::ExecuteSQL, vec!["x".repeat(2000)]);
        let mut out = Vec::new();
        let err = write_request(&mut out, &command).unwrap_err();
        assert!(matches!(err, Error::RequestTooLarge { max, .. } if max == MAX_REQUEST_BYTES));
        assert!(out.is_empty(), "nothing should reach the wire");
    }

    #[test]
    fn requests_at_the_limit_are_allowed() {
        // Grow the payload until the encoding is exactly at the cap.
        let mut sql = String::new();
        loop {
            let command = CommandWithArgs::db(DBCommand::ExecuteSQL, vec![sql.clone()]);
            let size = serde_cbor::to_vec(&command).unwrap().len();
            if size == MAX_REQUEST_BYTES {
                assert!(encode_request(&command).is_ok());
                return;
            }
            assert!(size < MAX_REQUEST_BYTES, "overshot the cap at {size}");
            sql.push('x');
        }
    }

    #[test]
    fn responses_are_read_from_a_length_prefixed_frame() {
        let response = Response::SystemMsg("Created database mydb".into());
        let mut stream = std::io::Cursor::new(framed(&response));
        assert_eq!(read_response(&mut stream).unwrap(), response);
    }

    #[test]
    fn select_responses_survive_a_frame_roundtrip() {
        let response = Response::QueryResult(QueryResult::Insert {
            inserted: 2,
            table_name: "users".into(),
        });
        let mut stream = std::io::Cursor::new(framed(&response));
        assert_eq!(read_response(&mut stream).unwrap(), response);
    }

    #[test]
    fn a_truncated_body_reports_a_closed_connection() {
        let response = Response::SystemMsg("hello".into());
        let mut bytes = framed(&response);
        bytes.truncate(bytes.len() - 2);
        let mut stream = std::io::Cursor::new(bytes);
        assert!(matches!(
            read_response(&mut stream).unwrap_err(),
            Error::ConnectionClosed
        ));
    }

    #[test]
    fn a_closed_stream_reports_a_closed_connection() {
        let mut stream = std::io::Cursor::new(Vec::new());
        assert!(matches!(
            read_response(&mut stream).unwrap_err(),
            Error::ConnectionClosed
        ));
    }

    #[test]
    fn an_empty_frame_reads_as_a_quiet_ok() {
        let mut stream = std::io::Cursor::new(0u64.to_be_bytes().to_vec());
        assert_eq!(read_response(&mut stream).unwrap(), Response::QuietOk);
    }

    #[test]
    fn an_absurd_length_is_refused_before_allocating() {
        let mut stream = std::io::Cursor::new(u64::MAX.to_be_bytes().to_vec());
        assert!(matches!(
            read_response(&mut stream).unwrap_err(),
            Error::ResponseTooLarge { .. }
        ));
    }

    #[test]
    fn system_commands_encode_with_their_arguments() {
        let command = CommandWithArgs::system(SystemCommand::Connect, vec!["mydb".into()]);
        let bytes = encode_request(&command).unwrap();
        let back: CommandWithArgs = serde_cbor::from_slice(&bytes).unwrap();
        assert_eq!(back, command);
    }
}

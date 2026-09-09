//! Regenerates the captured wire-format fixture from a live server.
//!
//! The fixture is real bytes from a real FairyDB server, so the unit tests can
//! check our types against the server's encoding without needing one running.
//! Re-run this after any change to FairyDB's `common` crate:
//!
//! ```text
//! FING_TEST_ADDR=127.0.0.1:3333 cargo test --test capture_fixture -- --ignored
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_cbor::Value as Cbor;

const FIXTURE: &str = "tests/fixtures/select_response.cbor";

fn addr() -> String {
    std::env::var("FING_TEST_ADDR").unwrap_or_else(|_| "127.0.0.1:3333".into())
}

/// `CommandWithArgs { command: Command::DB(DBCommand::ExecuteSQL), args }`, and
/// the `System` equivalent, built by hand so the capture does not depend on the
/// types it exists to verify.
fn command(kind: &str, variant: &str, args: Vec<&str>) -> Vec<u8> {
    let inner = Cbor::Map(
        [(Cbor::Text(kind.to_owned()), Cbor::Text(variant.to_owned()))]
            .into_iter()
            .collect(),
    );
    let request = Cbor::Map(
        [
            (Cbor::Text("command".to_owned()), inner),
            (
                Cbor::Text("args".to_owned()),
                Cbor::Array(args.into_iter().map(|a| Cbor::Text(a.to_owned())).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    serde_cbor::to_vec(&request).unwrap()
}

fn round_trip(stream: &mut TcpStream, request: &[u8]) -> Vec<u8> {
    stream.write_all(request).unwrap();
    stream.flush().unwrap();

    let mut length = [0u8; 8];
    stream.read_exact(&mut length).unwrap();
    let mut body = vec![0u8; u64::from_be_bytes(length) as usize];
    stream.read_exact(&mut body).unwrap();
    body
}

#[test]
#[ignore = "regenerates a fixture; needs a running FairyDB server"]
fn capture_a_select_response() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let database = format!("fing_fixture_{nanos}");
    let table = format!("fixture_{nanos}");

    let mut stream = TcpStream::connect(addr()).expect("connect to server");
    round_trip(&mut stream, &command("System", "Create", vec![&database]));
    round_trip(&mut stream, &command("System", "Connect", vec![&database]));

    let create = format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY, name VARCHAR(255))");
    round_trip(&mut stream, &command("DB", "ExecuteSQL", vec![&create]));
    for values in ["(1, 'alice')", "(2, 'Ziad')"] {
        let insert = format!("INSERT INTO {table} VALUES {values}");
        round_trip(&mut stream, &command("DB", "ExecuteSQL", vec![&insert]));
    }

    let select = format!("SELECT * FROM {table}");
    let body = round_trip(&mut stream, &command("DB", "ExecuteSQL", vec![&select]));

    assert!(!body.is_empty(), "server returned an empty response");
    std::fs::create_dir_all("tests/fixtures").unwrap();
    std::fs::write(FIXTURE, &body).unwrap();
    println!("wrote {} bytes to {FIXTURE}", body.len());
}

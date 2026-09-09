# fing

A Rust client library for [FairyDB](https://github.com/zelshahawy/fairydb), a SQL-compliant OLTP storage engine.

`fing` speaks FairyDB's TCP wire protocol directly, so it needs nothing from the FairyDB source tree at build time. Add it as a dependency and connect.

```toml
[dependencies]
fing = "0.1"
```

## Quickstart

Start a server (from your FairyDB checkout):

```bash
cargo run --bin server
```

Then:

```rust
use fing::Client;

fn main() -> Result<(), fing::Error> {
    let mut db = Client::connect("127.0.0.1:3333")?;
    db.create_database("mydb")?;
    db.use_database("mydb")?;

    db.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name VARCHAR(255))")?;
    db.execute("INSERT INTO users VALUES (1, 'alice')")?;
    db.execute("INSERT INTO users VALUES (2, 'Ziad')")?;

    let rows = db.query("SELECT * FROM users")?;
    println!("{rows}");

    for row in &rows {
        let id: i64 = row.get("id")?;
        let name: &str = row.get("name")?;
        println!("{id}: {name}");
    }

    Ok(())
}
```

`Rows` implements `Display`, so the `println!` above prints:

```text
 id | name
----+-------
  1 | alice
  2 | Ziad
(2 rows)
```

There's a runnable version in `examples/`:

```bash
cargo run --example basic 127.0.0.1:3333
```

## Reading values

`Row::get` converts through the `FromValue` trait. Integers widen freely and narrow when the value fits, which matters because FairyDB reports every integer column as `BIGINT` no matter how it was declared. Only `Option<T>` accepts SQL NULL, so an unexpected NULL is an error rather than a silent zero.

```rust
let id: i32 = row.get("id")?;                  // widening handled for you
let nickname: Option<&str> = row.get("nick")?; // may be NULL
let raw: &fing::Value = row.value("id")?;      // no conversion
let by_position: i64 = row.get(0usize)?;
```

Built-in conversions: all integer types, `f64` (including scaled `DECIMAL`), `bool`, `String`, `&str`, `Value`, and `Option<T>` of any of them. Enable the `chrono` feature for `chrono::NaiveDate` on `DATE` columns.

## API

| Call | What it does |
|---|---|
| `Client::connect(addr)` | Open a connection |
| `Client::connect_to(addr, db)` | Connect and select a database |
| `Client::builder()` | Timeouts, database, statement guard |
| `create_database` / `use_database` | `\r` and `\c` |
| `execute(sql)` | Run a statement, returning an `Outcome` |
| `query(sql)` | Run a query, returning `Rows` |
| `tables()` / `databases()` | `\dt` and `\l` |
| `import_csv(path, table)` | `\i` — path is resolved **server-side** |
| `commit()` | Commit and start a new transaction |
| `reset_server()` / `shutdown_server()` | `\reset` and `\shutdown` |

A connection is a session. FairyDB assigns a client id per TCP connection and binds the selected database to it, so every connection must select its own database before running SQL.

## FairyDB's limits

FairyDB is a teaching-oriented engine with some sharp edges. Where it can, this crate turns them into ordinary errors instead of hangs or dropped connections.

**Caught before anything is sent:**

- **Requests are capped at 1 KiB.** The server reads each request into a fixed 1024-byte buffer with a single `read`; anything larger is truncated and the connection is dropped. You get `Error::RequestTooLarge`, which in practice caps a statement at roughly 980 bytes of SQL.
- **Only `CREATE TABLE`, `INSERT` and queries are implemented.** `UPDATE`, `DELETE`, `DROP`, `ALTER`, `CREATE INDEX` and friends hit an `unimplemented!()` that panics the server's connection thread. You get `Error::Unsupported`. Disable with `set_statement_guard(false)` if the server grows support this crate doesn't know about.
- **Only the first statement in a string is executed.** The server parses a statement list but runs `ast.first()`. Multi-statement input gets `Error::MultipleStatements` rather than being silently half-applied.

**Not catchable client-side:**

- **One database per server, in practice.** Container ids restart at zero for each database while the storage manager is global, so the first table created in a *second* database collides with the first database's containers and fails with `Storage Error`. Reproducible with the stock `cli-fairy`.
- **Querying a table that doesn't exist panics the server thread** (`Conductor::run_sql` unwraps the translator's result). It arrives as `Error::ConnectionClosed`.
- **`import_csv` opens the path on the server**, not in your process, so it doesn't work against a remote server.

## Wire protocol

Recorded here because it's asymmetric and easy to get wrong:

| Direction | Framing |
|---|---|
| Client → server | Bare CBOR `CommandWithArgs`, no length prefix |
| Server → client | 8-byte big-endian length, then CBOR `Response` |

The types in `src/protocol.rs`, `src/schema.rs` and `src/value.rs` mirror FairyDB's `common` crate. Variant names, field names and payload shapes are all part of the encoding. Two are easy to miss:

- `TableSchema` has a hand-written serde impl that encodes it as a **bare sequence of attributes**, not a struct with an `attributes` key.
- `Tuple.value_id` is `skip_serializing` on the server with no matching `default`, so it never appears on the wire and needs `#[serde(default)]` here to decode.

`serde_cbor` is pinned to the same version the server and `cli-fairy` use, so the encoding is byte-compatible by construction.

## Testing

Unit tests need nothing; they include a fake server over a real socket and a decode test against `tests/fixtures/select_response.cbor`, captured from a real server.

```bash
cargo test
```

Integration tests need a running server:

```bash
FING_TEST_ADDR=127.0.0.1:3333 cargo test -- --ignored
```

They share one database, because of the container id collision above. Regenerate the wire-format fixture after any change to FairyDB's `common` crate:

```bash
FING_TEST_ADDR=127.0.0.1:3333 cargo test --test capture_fixture -- --ignored
```

## License

MIT OR Apache-2.0

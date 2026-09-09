//! `fing` is a client library for [FairyDB], a SQL-compliant OLTP storage engine.
//!
//! It speaks FairyDB's TCP wire protocol directly, so it needs nothing from the
//! FairyDB source tree at build time — add it as a dependency and connect.
//!
//! ```no_run
//! let mut db = fing::Client::connect("127.0.0.1:3333")?;
//! db.create_database("mydb")?;
//! db.use_database("mydb")?;
//!
//! db.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name VARCHAR(255))")?;
//! db.execute("INSERT INTO users VALUES (1, 'alice')")?;
//!
//! for row in &db.query("SELECT * FROM users")? {
//!     let id: i64 = row.get("id")?;
//!     let name: &str = row.get("name")?;
//!     println!("{id}: {name}");
//! }
//! # Ok::<(), fing::Error>(())
//! ```
//!
//! # Reading values
//!
//! [`Row::get`] converts through [`FromValue`]. Integers widen freely and narrow
//! when the value fits, which matters because FairyDB reports every integer
//! column as `BIGINT` regardless of how it was declared. Only `Option<T>`
//! accepts SQL NULL.
//!
//! ```no_run
//! # let rows: fing::Rows = unimplemented!();
//! # let row = rows.get(0).unwrap();
//! let id: i32 = row.get("id")?;                  // widening handled for you
//! let nickname: Option<&str> = row.get("nick")?; // may be NULL
//! let raw: &fing::Value = row.value("id")?;      // no conversion
//! # Ok::<(), fing::Error>(())
//! ```
//!
//! [`Rows`] implements `Display`, so `println!("{rows}")` prints an aligned table.
//!
//! # A connection is a session
//!
//! FairyDB keys session state to the TCP connection: the server assigns a client
//! id per connection, and [`Client::use_database`] binds a database to that id.
//! A reconnect starts a fresh session, so every connection selects its own
//! database before running SQL.
//!
//! # FairyDB's limits
//!
//! FairyDB is a teaching-oriented engine with some sharp edges. Where possible
//! this crate turns them into ordinary errors instead of hangs or dropped
//! connections:
//!
//! - **Requests are capped at 1 KiB.** The server reads each request into a
//!   fixed 1024-byte buffer with a single `read`, so anything larger is
//!   truncated and the connection is dropped. See [`Error::RequestTooLarge`].
//! - **Only `CREATE TABLE`, `INSERT` and queries are implemented.** Anything
//!   else panics the server's connection thread. Such statements are rejected
//!   before they are sent; see [`Error::Unsupported`] and
//!   [`Client::set_statement_guard`].
//! - **Only the first statement in a string is executed.** Multi-statement input
//!   is rejected rather than silently half-applied; see
//!   [`Error::MultipleStatements`].
//!
//! Two more are the server's alone, and cannot be caught here:
//!
//! - **One database per server, in practice.** Container ids restart at zero for
//!   each database while the storage manager is global, so creating a table in a
//!   second database collides with the first one's containers and fails with
//!   `Storage Error`.
//! - **Querying a missing table panics the server thread**, which arrives here as
//!   [`Error::ConnectionClosed`].
//!
//! [FairyDB]: https://github.com/zelshahawy/fairydb

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod client;
mod codec;
mod convert;
mod error;
mod protocol;
mod rows;
mod schema;
mod sql;
mod value;

pub use client::{Client, ClientBuilder, Outcome};
pub use convert::FromValue;
pub use error::{ConversionError, Error, Result};
pub use protocol::PagingInfo;
pub use rows::{ColumnIndex, Row, RowIter, Rows};
pub use schema::{Attribute, Constraint, Schema};
pub use value::{DataType, Value};

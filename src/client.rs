//! The connection to a FairyDB server.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::codec;
use crate::error::{Error, Result};
use crate::protocol::{CommandWithArgs, DBCommand, QueryResult, Response, SystemCommand};
use crate::rows::Rows;
use crate::sql;

/// What a statement did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The server acknowledged with a message, as `CREATE TABLE` does.
    Message(String),
    /// Rows were inserted.
    Insert {
        /// Number of rows inserted.
        rows: usize,
        /// Table they went into.
        table: String,
    },
    /// The statement produced a result set.
    Rows(Rows),
}

impl Outcome {
    /// Rows inserted, or rows returned; zero for a bare acknowledgement.
    pub fn rows_affected(&self) -> usize {
        match self {
            Outcome::Message(_) => 0,
            Outcome::Insert { rows, .. } => *rows,
            Outcome::Rows(rows) => rows.len(),
        }
    }

    /// The server's message, if this was an acknowledgement.
    pub fn message(&self) -> Option<&str> {
        match self {
            Outcome::Message(message) => Some(message),
            _ => None,
        }
    }

    /// The result set, if there was one.
    pub fn rows(&self) -> Option<&Rows> {
        match self {
            Outcome::Rows(rows) => Some(rows),
            _ => None,
        }
    }

    /// Take the result set, or fail if the statement did not produce one.
    pub fn into_rows(self) -> Result<Rows> {
        match self {
            Outcome::Rows(rows) => Ok(rows),
            Outcome::Message(message) => Err(Error::UnexpectedResponse(format!(
                "expected a result set, server said: {message}"
            ))),
            Outcome::Insert { rows, table } => Err(Error::UnexpectedResponse(format!(
                "expected a result set, but {rows} rows were inserted into {table}"
            ))),
        }
    }
}

/// Options for opening a connection.
#[derive(Debug, Clone, Default)]
pub struct ClientBuilder {
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    database: Option<String>,
    statement_guard: Option<bool>,
}

impl ClientBuilder {
    /// Cap how long the TCP connect may take.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Cap how long a read may block.
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// Cap how long a write may block.
    pub fn write_timeout(mut self, timeout: Duration) -> Self {
        self.write_timeout = Some(timeout);
        self
    }

    /// Select this database once connected.
    pub fn database(mut self, name: impl Into<String>) -> Self {
        self.database = Some(name.into());
        self
    }

    /// Enable or disable the client-side statement guard. On by default; see
    /// [`Client::set_statement_guard`].
    pub fn statement_guard(mut self, enabled: bool) -> Self {
        self.statement_guard = Some(enabled);
        self
    }

    /// Connect using these options.
    pub fn connect<A: ToSocketAddrs>(self, addr: A) -> Result<Client> {
        let addr = resolve(addr)?;
        let stream = match self.connect_timeout {
            Some(timeout) => TcpStream::connect_timeout(&addr, timeout)?,
            None => TcpStream::connect(addr)?,
        };
        stream.set_nodelay(true)?;
        stream.set_read_timeout(self.read_timeout)?;
        stream.set_write_timeout(self.write_timeout)?;

        let mut client = Client {
            stream,
            addr,
            database: None,
            statement_guard: self.statement_guard.unwrap_or(true),
        };
        if let Some(database) = self.database {
            client.use_database(&database)?;
        }
        Ok(client)
    }
}

fn resolve<A: ToSocketAddrs>(addr: A) -> Result<SocketAddr> {
    addr.to_socket_addrs()?
        .next()
        .ok_or_else(|| Error::InvalidArgument("address resolved to nothing".into()))
}

/// A connection to a FairyDB server.
///
/// FairyDB keys session state to the connection, so the database selected with
/// [`Client::use_database`] applies to this `Client` alone.
#[derive(Debug)]
pub struct Client {
    stream: TcpStream,
    addr: SocketAddr,
    database: Option<String>,
    statement_guard: bool,
}

impl Client {
    /// Connect to a server.
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Client> {
        ClientBuilder::default().connect(addr)
    }

    /// Connect and select `database` in one step.
    pub fn connect_to<A: ToSocketAddrs>(addr: A, database: &str) -> Result<Client> {
        ClientBuilder::default().database(database).connect(addr)
    }

    /// Start building a connection with non-default options.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The server address.
    pub fn server_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The database selected on this connection.
    pub fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// Whether statements are checked before being sent.
    ///
    /// FairyDB panics its connection thread on statements it does not
    /// implement, so this is on by default. Turn it off if the server has grown
    /// support for statements this crate does not know about.
    pub fn set_statement_guard(&mut self, enabled: bool) {
        self.statement_guard = enabled;
    }

    /// Cap how long a read may block.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<()> {
        self.stream.set_read_timeout(timeout)?;
        Ok(())
    }

    /// Cap how long a write may block.
    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> Result<()> {
        self.stream.set_write_timeout(timeout)?;
        Ok(())
    }

    /// Create a database.
    pub fn create_database(&mut self, name: &str) -> Result<()> {
        let name = require_name(name, "database name")?;
        self.system(SystemCommand::Create, vec![name])?;
        Ok(())
    }

    /// Select the database this connection will use.
    pub fn use_database(&mut self, name: &str) -> Result<()> {
        let name = require_name(name, "database name")?;
        self.system(SystemCommand::Connect, vec![name.clone()])?;
        self.database = Some(name);
        Ok(())
    }

    /// List the databases on the server.
    pub fn databases(&mut self) -> Result<Vec<String>> {
        let response = self.system(SystemCommand::ShowDatabases, vec![])?;
        let message = expect_message(response)?;
        Ok(parse_list(&message, "Databases:"))
    }

    /// List the tables in the selected database.
    pub fn tables(&mut self) -> Result<Vec<String>> {
        let response = self.db(DBCommand::ShowTables, vec![])?;
        let message = expect_message(response)?;
        Ok(parse_list(&message, "Tables:"))
    }

    /// Run a statement.
    pub fn execute(&mut self, sql: &str) -> Result<Outcome> {
        if self.statement_guard {
            sql::inspect(sql)?;
        } else if sql.trim().is_empty() {
            return Err(Error::InvalidArgument("statement is empty".into()));
        }

        let response = self.db(DBCommand::ExecuteSQL, vec![sql.to_owned()])?;
        match response {
            Response::QueryResult(result) => Ok(into_outcome(result)),
            Response::SystemMsg(message) => Ok(Outcome::Message(message)),
            Response::Ok | Response::QuietOk => Ok(Outcome::Message(String::new())),
            other => Err(unexpected(&other)),
        }
    }

    /// Run a query and take its rows.
    pub fn query(&mut self, sql: &str) -> Result<Rows> {
        self.execute(sql)?.into_rows()
    }

    /// Import a CSV file into `table`, returning the number of rows read.
    ///
    /// The path is opened by the **server**, not this process, so it must be
    /// reachable from wherever the server is running.
    pub fn import_csv(&mut self, server_path: &str, table: &str) -> Result<usize> {
        let path = require_name(server_path, "path")?;
        let table = require_name(table, "table name")?;
        let response = self.db(DBCommand::Import, vec![path, table])?;
        let message = expect_message(response)?;
        parse_imported(&message)
    }

    /// Commit the current transaction and begin a new one.
    pub fn commit(&mut self) -> Result<()> {
        self.db(DBCommand::Commit, vec![])?;
        Ok(())
    }

    /// The server's own help text.
    pub fn help(&mut self) -> Result<String> {
        let response = self.system(SystemCommand::Help, vec![])?;
        expect_message(response)
    }

    /// Delete all databases and state on the server.
    pub fn reset_server(&mut self) -> Result<()> {
        self.system(SystemCommand::Reset, vec![])?;
        self.database = None;
        Ok(())
    }

    /// Ask the server to shut down cleanly.
    ///
    /// The server's accept loop only notices the shutdown flag when a
    /// connection arrives, so this opens one throwaway connection afterwards to
    /// wake it. Without that the server blocks in `accept` forever.
    pub fn shutdown_server(mut self) -> Result<()> {
        let response = self.round_trip(CommandWithArgs::system(SystemCommand::Shutdown, vec![]))?;
        match response {
            Response::Shutdown(_) | Response::QuietOk => {
                let _ = TcpStream::connect(self.addr);
                Ok(())
            }
            other => Err(unexpected(&other)),
        }
    }

    /// Close this connection, releasing the session on the server.
    pub fn close(mut self) -> Result<()> {
        self.system(SystemCommand::CloseConnection, vec![])?;
        Ok(())
    }

    fn system(&mut self, command: SystemCommand, args: Vec<String>) -> Result<Response> {
        self.round_trip(CommandWithArgs::system(command, args))
            .and_then(check)
    }

    fn db(&mut self, command: DBCommand, args: Vec<String>) -> Result<Response> {
        self.round_trip(CommandWithArgs::db(command, args))
            .and_then(check)
    }

    fn round_trip(&mut self, command: CommandWithArgs) -> Result<Response> {
        codec::write_request(&mut self.stream, &command)?;
        codec::read_response(&mut self.stream)
    }
}

/// Turn the error-carrying responses into `Err`.
fn check(response: Response) -> Result<Response> {
    match response {
        Response::SystemErr(message) if message.contains("Not connected to a database") => {
            Err(Error::NotConnected)
        }
        Response::SystemErr(message) => Err(Error::Server(message)),
        Response::QueryExecutionError(message) => Err(Error::Query(message)),
        Response::QuietErr => Err(Error::Server("server reported an error".into())),
        Response::Shutdown(false) => Err(Error::ServerShutdown),
        other => Ok(other),
    }
}

fn into_outcome(result: QueryResult) -> Outcome {
    match result {
        QueryResult::MessageOnly(message) => Outcome::Message(message),
        QueryResult::Insert {
            inserted,
            table_name,
        } => Outcome::Insert {
            rows: inserted,
            table: table_name,
        },
        QueryResult::Select {
            schema,
            result,
            paging_info,
        } => {
            let rows = result.into_iter().map(|tuple| tuple.field_vals).collect();
            Outcome::Rows(Rows::new(schema, rows, paging_info))
        }
    }
}

fn expect_message(response: Response) -> Result<String> {
    match response {
        Response::SystemMsg(message) => Ok(message),
        Response::QueryResult(QueryResult::MessageOnly(message)) => Ok(message),
        other => Err(unexpected(&other)),
    }
}

fn unexpected(response: &Response) -> Error {
    Error::UnexpectedResponse(format!("{response:?}"))
}

fn require_name(value: &str, what: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidArgument(format!("{what} is empty")));
    }
    Ok(trimmed.to_owned())
}

/// The server reports these lists as prose, so they have to be parsed back out.
fn parse_list(message: &str, prefix: &str) -> Vec<String> {
    message
        .strip_prefix(prefix)
        .unwrap_or(message)
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_imported(message: &str) -> Result<usize> {
    message
        .split_whitespace()
        .find_map(|word| word.parse::<usize>().ok())
        .ok_or_else(|| Error::UnexpectedResponse(format!("no row count in: {message}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_lists_are_parsed_from_prose() {
        assert_eq!(
            parse_list("Databases: a, b, c", "Databases:"),
            vec!["a", "b", "c"]
        );
        assert_eq!(parse_list("Tables: users", "Tables:"), vec!["users"]);
    }

    #[test]
    fn an_empty_list_parses_as_no_entries() {
        assert!(parse_list("Tables: ", "Tables:").is_empty());
        assert!(parse_list("Databases:", "Databases:").is_empty());
    }

    #[test]
    fn import_counts_are_parsed_from_prose() {
        assert_eq!(
            parse_imported("Imported 42 rows into table users").unwrap(),
            42
        );
        assert!(parse_imported("Imported rows into table users").is_err());
    }

    #[test]
    fn error_responses_become_errors() {
        assert!(matches!(
            check(Response::SystemErr("Not connected to a database".into())),
            Err(Error::NotConnected)
        ));
        assert!(matches!(
            check(Response::SystemErr("boom".into())),
            Err(Error::Server(_))
        ));
        assert!(matches!(
            check(Response::QueryExecutionError("bad sql".into())),
            Err(Error::Query(_))
        ));
        assert!(matches!(
            check(Response::Shutdown(false)),
            Err(Error::ServerShutdown)
        ));
        assert!(check(Response::Ok).is_ok());
    }

    #[test]
    fn names_must_not_be_blank() {
        assert!(require_name("  ", "database name").is_err());
        assert_eq!(require_name(" mydb ", "database name").unwrap(), "mydb");
    }

    mod against_a_fake_server {
        use super::*;
        use crate::protocol::Tuple;
        use crate::schema::{Attribute, Constraint, Schema};
        use crate::value::{DataType, Value};
        use std::io::{Read, Write};
        use std::net::TcpListener;

        /// Serve `responses` in order, one per request, then hang up.
        fn spawn(responses: Vec<Response>) -> SocketAddr {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                for response in responses {
                    let mut request = [0u8; 1024];
                    match stream.read(&mut request) {
                        Ok(0) | Err(_) => return,
                        Ok(_) => {}
                    }
                    let body = serde_cbor::to_vec(&response).unwrap();
                    let framed = (body.len() as u64).to_be_bytes();
                    if stream.write_all(&framed).is_err() || stream.write_all(&body).is_err() {
                        return;
                    }
                }
            });
            addr
        }

        fn select_response() -> Response {
            Response::QueryResult(QueryResult::Select {
                schema: Schema::new(vec![
                    Attribute {
                        name: "id".into(),
                        dtype: DataType::Int,
                        constraint: Constraint::PrimaryKey,
                    },
                    Attribute {
                        name: "name".into(),
                        dtype: DataType::String,
                        constraint: Constraint::None,
                    },
                ]),
                result: vec![Tuple {
                    tid: 0,
                    value_id: None,
                    field_vals: vec![Value::Int(1), Value::String("alice".into())],
                }],
                paging_info: None,
            })
        }

        #[test]
        fn selecting_a_database_records_it_on_the_connection() {
            let addr = spawn(vec![Response::SystemMsg(
                "Connected to database mydb".into(),
            )]);
            let mut client = Client::connect(addr).unwrap();
            assert_eq!(client.database(), None);
            client.use_database("mydb").unwrap();
            assert_eq!(client.database(), Some("mydb"));
        }

        #[test]
        fn queries_yield_typed_rows() {
            let addr = spawn(vec![select_response()]);
            let mut client = Client::connect(addr).unwrap();
            let rows = client.query("SELECT * FROM users").unwrap();

            assert_eq!(rows.len(), 1);
            let row = rows.get(0).unwrap();
            assert_eq!(row.get::<i64>("id").unwrap(), 1);
            assert_eq!(row.get::<&str>("name").unwrap(), "alice");
        }

        #[test]
        fn inserts_report_their_row_count() {
            let addr = spawn(vec![Response::QueryResult(QueryResult::Insert {
                inserted: 2,
                table_name: "users".into(),
            })]);
            let mut client = Client::connect(addr).unwrap();
            let outcome = client.execute("INSERT INTO users VALUES (1, 'a')").unwrap();
            assert_eq!(outcome.rows_affected(), 2);
        }

        #[test]
        fn query_execution_errors_surface_as_errors() {
            let addr = spawn(vec![Response::QueryExecutionError("no such table".into())]);
            let mut client = Client::connect(addr).unwrap();
            let err = client.query("SELECT * FROM nope").unwrap_err();
            assert!(matches!(err, Error::Query(message) if message == "no such table"));
        }

        #[test]
        fn running_sql_before_selecting_a_database_is_reported_clearly() {
            let addr = spawn(vec![Response::SystemErr(
                "Not connected to a database".into(),
            )]);
            let mut client = Client::connect(addr).unwrap();
            assert!(matches!(
                client.query("SELECT 1").unwrap_err(),
                Error::NotConnected
            ));
        }

        #[test]
        fn unsupported_statements_never_reach_the_wire() {
            // The server would panic on these, so no response is scripted: if the
            // client sent one it would block and fail the test instead.
            let addr = spawn(Vec::new());
            let mut client = Client::connect(addr).unwrap();
            assert!(matches!(
                client.execute("DELETE FROM users").unwrap_err(),
                Error::Unsupported(_)
            ));
            assert!(matches!(
                client.execute("SELECT 1; SELECT 2").unwrap_err(),
                Error::MultipleStatements
            ));
        }

        #[test]
        fn the_statement_guard_can_be_turned_off() {
            let addr = spawn(vec![Response::SystemMsg("ok".into())]);
            let mut client = Client::connect(addr).unwrap();
            client.set_statement_guard(false);
            assert!(client.execute("DELETE FROM users").is_ok());
        }

        #[test]
        fn oversized_statements_are_refused_locally() {
            let addr = spawn(Vec::new());
            let mut client = Client::connect(addr).unwrap();
            let sql = format!("SELECT * FROM t WHERE name = '{}'", "x".repeat(2000));
            assert!(matches!(
                client.execute(&sql).unwrap_err(),
                Error::RequestTooLarge { .. }
            ));
        }

        #[test]
        fn a_hung_up_server_reports_a_closed_connection() {
            let addr = spawn(Vec::new());
            let mut client = Client::connect(addr).unwrap();
            assert!(matches!(
                client.query("SELECT 1").unwrap_err(),
                Error::ConnectionClosed
            ));
        }

        #[test]
        fn table_listings_are_parsed() {
            let addr = spawn(vec![Response::QueryResult(QueryResult::MessageOnly(
                "Tables: users, orders".into(),
            ))]);
            let mut client = Client::connect(addr).unwrap();
            assert_eq!(client.tables().unwrap(), vec!["users", "orders"]);
        }
    }

    #[test]
    fn outcomes_report_affected_rows() {
        let insert = Outcome::Insert {
            rows: 3,
            table: "t".into(),
        };
        assert_eq!(insert.rows_affected(), 3);
        assert_eq!(Outcome::Message("ok".into()).rows_affected(), 0);
        assert!(Outcome::Message("ok".into()).into_rows().is_err());
    }
}

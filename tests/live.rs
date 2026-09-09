//! Tests against a running FairyDB server.
//!
//! Ignored by default, since they need a server. Start one and point the tests
//! at it:
//!
//! ```text
//! cargo run --bin server -- -o 127.0.0.1 -p 3333    # in the fairydb checkout
//! FING_TEST_ADDR=127.0.0.1:3333 cargo test -- --ignored
//! ```
//!
//! Every test shares one database. FairyDB's container ids restart at zero for
//! each database while the storage manager is global, so creating a table in a
//! second database collides with the first one's containers and fails with
//! "Storage Error". Tables are therefore named uniquely instead.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use fing::{Client, DataType, Error, Value};

fn addr() -> String {
    std::env::var("FING_TEST_ADDR").unwrap_or_else(|_| "127.0.0.1:3333".into())
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// One database for the whole test binary, created on first use.
fn shared_database() -> &'static str {
    static DATABASE: OnceLock<String> = OnceLock::new();
    DATABASE.get_or_init(|| {
        let name = format!("fing_test_{}", nanos());
        let mut client = Client::connect(addr()).expect("connect to server");
        client.create_database(&name).expect("create database");
        name
    })
}

fn connect() -> Client {
    Client::connect_to(addr(), shared_database()).expect("connect and select database")
}

fn unique_table(tag: &str) -> String {
    format!("{tag}_{}", nanos())
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn create_insert_and_select() {
    let mut db = connect();
    let table = unique_table("users");

    db.execute(&format!(
        "CREATE TABLE {table} (id INTEGER PRIMARY KEY, name VARCHAR(255))"
    ))
    .unwrap();

    let inserted = db
        .execute(&format!("INSERT INTO {table} VALUES (1, 'alice')"))
        .unwrap()
        .rows_affected();
    assert_eq!(inserted, 1);
    db.execute(&format!("INSERT INTO {table} VALUES (2, 'Ziad')"))
        .unwrap();

    let rows = db.query(&format!("SELECT * FROM {table}")).unwrap();
    assert_eq!(rows.len(), 2);

    let names: Vec<&str> = rows.iter().map(|row| row.get("name").unwrap()).collect();
    assert!(names.contains(&"alice"), "got {names:?}");
    assert!(names.contains(&"Ziad"), "got {names:?}");

    let ids: Vec<i64> = rows.iter().map(|row| row.get("id").unwrap()).collect();
    assert!(ids.contains(&1) && ids.contains(&2), "got {ids:?}");
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn schema_comes_back_with_column_types() {
    let mut db = connect();
    let table = unique_table("typed");

    db.execute(&format!(
        "CREATE TABLE {table} (a INT, b VARCHAR(20), PRIMARY KEY (a))"
    ))
    .unwrap();
    db.execute(&format!("INSERT INTO {table} VALUES (1, 'x')"))
        .unwrap();

    let rows = db.query(&format!("SELECT * FROM {table}")).unwrap();
    assert_eq!(rows.schema().names().collect::<Vec<_>>(), vec!["a", "b"]);

    // FairyDB widens INT to BIGINT and VARCHAR to its variable-length string
    // type, so the declared column type is not what comes back.
    assert_eq!(rows.columns()[0].dtype, DataType::BigInt);
    assert_eq!(rows.columns()[1].dtype, DataType::String);
    assert!(rows.columns()[0].is_primary_key());

    let row = rows.get(0).unwrap();
    assert!(matches!(row.value("a").unwrap(), Value::BigInt(1)));
    // Reading through FromValue does not depend on that widening.
    assert_eq!(row.get::<i32>("a").unwrap(), 1);
    assert_eq!(row.get::<&str>("b").unwrap(), "x");
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn projections_and_predicates_work() {
    let mut db = connect();
    let table = unique_table("nums");

    db.execute(&format!("CREATE TABLE {table} (n INTEGER PRIMARY KEY)"))
        .unwrap();
    for n in 1..=5 {
        db.execute(&format!("INSERT INTO {table} VALUES ({n})"))
            .unwrap();
    }

    let rows = db
        .query(&format!("SELECT n FROM {table} WHERE n > 3"))
        .unwrap();
    let mut got: Vec<i64> = rows.iter().map(|row| row.get("n").unwrap()).collect();
    got.sort_unstable();
    assert_eq!(got, vec![4, 5]);
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn tables_are_listed() {
    let mut db = connect();
    let table = unique_table("listed");

    db.execute(&format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY)"))
        .unwrap();
    assert!(db.tables().unwrap().contains(&table));
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn databases_are_listed() {
    let mut client = Client::connect(addr()).unwrap();
    let listed = client.databases().unwrap();
    assert!(listed.iter().any(|name| name == shared_database()));
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn an_empty_result_keeps_its_schema() {
    let mut db = connect();
    let table = unique_table("empty");

    db.execute(&format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY)"))
        .unwrap();

    let rows = db.query(&format!("SELECT * FROM {table}")).unwrap();
    assert!(rows.is_empty());
    assert_eq!(rows.schema().names().collect::<Vec<_>>(), vec!["id"]);
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn connect_to_selects_the_database_in_one_step() {
    let db = connect();
    assert_eq!(db.database(), Some(shared_database()));
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn sql_before_selecting_a_database_is_refused() {
    let mut client = Client::connect(addr()).unwrap();
    assert!(matches!(client.query("SELECT 1"), Err(Error::NotConnected)));
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn unsupported_statements_are_refused_without_reaching_the_server() {
    let mut db = connect();
    let table = unique_table("guarded");

    db.execute(&format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY)"))
        .unwrap();
    assert!(matches!(
        db.execute(&format!("DELETE FROM {table}")),
        Err(Error::Unsupported(_))
    ));

    // The guard stopped it locally, so the connection is untouched.
    db.execute(&format!("INSERT INTO {table} VALUES (1)"))
        .unwrap();
    assert_eq!(
        db.query(&format!("SELECT * FROM {table}")).unwrap().len(),
        1
    );
}

#[test]
#[ignore = "needs a running FairyDB server"]
fn oversized_statements_are_refused_without_reaching_the_server() {
    let mut db = connect();
    let sql = format!("SELECT * FROM t WHERE name = '{}'", "x".repeat(2000));
    assert!(matches!(
        db.execute(&sql),
        Err(Error::RequestTooLarge { .. })
    ));
}

/// Querying a table that does not exist panics the server's connection thread:
/// `Conductor::run_sql` unwraps the translator's result. Nothing the client can
/// prevent, but it must surface as an error rather than a hang.
#[test]
#[ignore = "needs a running FairyDB server"]
fn a_missing_table_closes_the_connection() {
    let mut db = connect();
    assert!(matches!(
        db.query("SELECT * FROM definitely_not_a_table"),
        Err(Error::ConnectionClosed) | Err(Error::Query(_))
    ));
}

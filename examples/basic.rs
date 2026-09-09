//! End-to-end example.
//!
//! Start a FairyDB server, then:
//!
//! ```text
//! cargo run --example basic              # defaults to 127.0.0.1:3333
//! cargo run --example basic 127.0.0.1:3399
//! ```

fn main() -> Result<(), fing::Error> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:3333".into());

    let mut db = fing::Client::connect(&addr)?;
    println!("connected to {}", db.server_addr());

    // FairyDB can only hold tables in one database per server, so reuse
    // whichever one already exists rather than adding another.
    let existing = db.databases()?;
    let database = existing.first().cloned().unwrap_or_else(|| "demo".into());
    if existing.is_empty() {
        db.create_database(&database)?;
    }
    db.use_database(&database)?;
    println!("using database `{database}`");

    if !db.tables()?.iter().any(|table| table == "users") {
        db.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name VARCHAR(255))")?;
        db.execute("INSERT INTO users VALUES (1, 'alice')")?;
        db.execute("INSERT INTO users VALUES (2, 'Ziad')")?;
        println!("created and populated `users`");
    }

    let rows = db.query("SELECT * FROM users")?;
    println!("\n{rows}\n");

    for row in &rows {
        let id: i64 = row.get("id")?;
        let name: &str = row.get("name")?;
        println!("{id}: {name}");
    }

    Ok(())
}

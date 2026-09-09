//! A guard over statements FairyDB cannot run.
//!
//! `Conductor::run_sql` handles `CREATE TABLE`, `INSERT` and queries, and falls
//! through to `unimplemented!()` for everything else, which panics the thread
//! serving the connection. It also executes only the first statement it parses.
//! Both are much friendlier caught here than observed as a dropped connection.

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatementKind {
    Query,
    Insert,
    CreateTable,
}

/// Classify a statement, rejecting anything the server would refuse or truncate.
pub(crate) fn inspect(sql: &str) -> Result<StatementKind> {
    let words = scan(sql)?;

    let Some(first) = words.first() else {
        return Err(Error::InvalidArgument("statement is empty".into()));
    };

    match first.as_str() {
        "SELECT" | "WITH" => Ok(StatementKind::Query),
        "INSERT" => Ok(StatementKind::Insert),
        "CREATE" => match words.get(1).map(String::as_str) {
            Some("TABLE") => Ok(StatementKind::CreateTable),
            Some(what) => Err(Error::Unsupported(format!("CREATE {what} statements"))),
            None => Err(Error::InvalidArgument("incomplete CREATE statement".into())),
        },
        other => Err(Error::Unsupported(format!("{other} statements"))),
    }
}

/// Walk `sql`, skipping comments and quoted text, collecting the leading
/// keywords and rejecting anything after a statement terminator.
fn scan(sql: &str) -> Result<Vec<String>> {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut words: Vec<String> = Vec::new();
    let mut terminated = false;

    macro_rules! significant {
        () => {
            if terminated {
                return Err(Error::MultipleStatements);
            }
        };
    }

    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b';' => {
                terminated = true;
                i += 1;
            }
            b'\'' => {
                significant!();
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'"' => {
                significant!();
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            b if b.is_ascii_alphanumeric() || b == b'_' => {
                significant!();
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80)
                {
                    i += 1;
                }
                if words.len() < 2 {
                    words.push(sql[start..i].to_ascii_uppercase());
                }
            }
            _ => {
                significant!();
                i += 1;
            }
        }
    }

    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_statements_are_classified() {
        assert_eq!(inspect("SELECT * FROM t").unwrap(), StatementKind::Query);
        assert_eq!(
            inspect("insert into t values (1)").unwrap(),
            StatementKind::Insert
        );
        assert_eq!(
            inspect("CREATE TABLE t (a INT)").unwrap(),
            StatementKind::CreateTable
        );
    }

    #[test]
    fn a_trailing_semicolon_is_fine() {
        assert_eq!(inspect("SELECT 1;").unwrap(), StatementKind::Query);
        assert_eq!(inspect("SELECT 1;   ").unwrap(), StatementKind::Query);
        assert_eq!(
            inspect("SELECT 1; -- done").unwrap(),
            StatementKind::Query
        );
    }

    #[test]
    fn statements_the_server_cannot_run_are_rejected() {
        for sql in [
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "DROP TABLE t",
            "ALTER TABLE t ADD b INT",
            "TRUNCATE TABLE t",
            "EXPLAIN SELECT 1",
        ] {
            assert!(
                matches!(inspect(sql), Err(Error::Unsupported(_))),
                "expected {sql} to be rejected"
            );
        }
    }

    #[test]
    fn only_create_table_is_supported() {
        assert!(matches!(
            inspect("CREATE INDEX i ON t (a)"),
            Err(Error::Unsupported(what)) if what == "CREATE INDEX statements"
        ));
        assert!(inspect("CREATE TABLE t (a INT)").is_ok());
    }

    #[test]
    fn multiple_statements_are_rejected() {
        assert!(matches!(
            inspect("SELECT 1; SELECT 2"),
            Err(Error::MultipleStatements)
        ));
        assert!(matches!(
            inspect("INSERT INTO t VALUES (1); INSERT INTO t VALUES (2);"),
            Err(Error::MultipleStatements)
        ));
    }

    #[test]
    fn semicolons_inside_strings_do_not_split_statements() {
        assert_eq!(
            inspect("INSERT INTO t VALUES ('a;b')").unwrap(),
            StatementKind::Insert
        );
        assert_eq!(
            inspect("SELECT * FROM t WHERE s = 'it''s; fine'").unwrap(),
            StatementKind::Query
        );
    }

    #[test]
    fn semicolons_inside_comments_do_not_split_statements() {
        assert_eq!(
            inspect("SELECT 1 -- ; not a statement").unwrap(),
            StatementKind::Query
        );
        assert_eq!(
            inspect("/* ; */ SELECT 1").unwrap(),
            StatementKind::Query
        );
    }

    #[test]
    fn leading_comments_are_skipped() {
        assert_eq!(
            inspect("-- a comment\nSELECT 1").unwrap(),
            StatementKind::Query
        );
        assert_eq!(
            inspect("/* block */ INSERT INTO t VALUES (1)").unwrap(),
            StatementKind::Insert
        );
    }

    #[test]
    fn parenthesized_queries_are_queries() {
        assert_eq!(inspect("(SELECT 1)").unwrap(), StatementKind::Query);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(matches!(inspect(""), Err(Error::InvalidArgument(_))));
        assert!(matches!(inspect("  \n -- hi"), Err(Error::InvalidArgument(_))));
        assert!(matches!(inspect(";"), Err(Error::InvalidArgument(_))));
    }

    #[test]
    fn quoted_identifiers_are_not_keywords() {
        assert_eq!(
            inspect("SELECT \"UPDATE\" FROM t").unwrap(),
            StatementKind::Query
        );
    }
}

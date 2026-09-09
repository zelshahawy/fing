//! The FairyDB wire types.
//!
//! These mirror `common::commands` and `common::query::query_result`. Type names
//! are free, but variant names, field names and payload shapes are all part of
//! the CBOR encoding and must match the server exactly.

use serde::{Deserialize, Serialize};

use crate::schema::Schema;
use crate::value::Value;

/// Commands that act on server state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SystemCommand {
    Create,
    Connect,
    Reset,
    Shutdown,
    CloseConnection,
    QuietMode,
    ShowDatabases,
    Test,
    Help,
}

/// Commands that act on the connected database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DBCommand {
    ExecuteSQL,
    RegisterQuery,
    RunQueryFull,
    RunQueryPartial,
    ConvertQuery,
    ShowTables,
    ShowQueries,
    Generate,
    Import,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Command {
    System(SystemCommand),
    DB(DBCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CommandWithArgs {
    pub command: Command,
    pub args: Vec<String>,
}

impl CommandWithArgs {
    pub(crate) fn system(command: SystemCommand, args: Vec<String>) -> Self {
        CommandWithArgs {
            command: Command::System(command),
            args,
        }
    }

    pub(crate) fn db(command: DBCommand, args: Vec<String>) -> Self {
        CommandWithArgs {
            command: Command::DB(command),
            args,
        }
    }
}

/// Where a stored value lives. Never populated on the wire, but part of the
/// `Tuple` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ValueId {
    pub container_id: u16,
    pub segment_id: Option<u8>,
    pub page_id: Option<u32>,
    pub slot_id: Option<u16>,
}

/// A result row as the server sends it.
///
/// `value_id` is `skip_serializing` on the server with no matching `default`, so
/// it never appears on the wire. `default` here is what makes decoding work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tuple {
    pub tid: u64,
    #[serde(default, skip_serializing)]
    pub value_id: Option<ValueId>,
    pub field_vals: Vec<Value>,
}

/// Pagination metadata attached to a result set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagingInfo {
    /// Zero-based index of the current page.
    pub current_page: u32,
    /// Total number of pages.
    pub total_pages: u32,
    /// Whether another page follows.
    pub has_next_page: bool,
    /// Rows per page.
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum QueryResult {
    MessageOnly(String),
    Select {
        schema: Schema,
        result: Vec<Tuple>,
        paging_info: Option<PagingInfo>,
    },
    Insert {
        inserted: usize,
        table_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Response {
    Ok,
    SystemMsg(String),
    SystemErr(String),
    QueryResult(QueryResult),
    QueryExecutionError(String),
    /// `true` when the shutdown was requested by a client.
    Shutdown(bool),
    QuietOk,
    QuietErr,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Attribute, Constraint};
    use crate::value::DataType;

    fn roundtrip<T>(value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let bytes = serde_cbor::to_vec(value).unwrap();
        serde_cbor::from_slice(&bytes).unwrap()
    }

    #[test]
    fn commands_roundtrip_through_cbor() {
        let cases = vec![
            CommandWithArgs::system(SystemCommand::Create, vec!["mydb".into()]),
            CommandWithArgs::system(SystemCommand::ShowDatabases, vec![]),
            CommandWithArgs::db(DBCommand::ExecuteSQL, vec!["SELECT 1".into()]),
            CommandWithArgs::db(DBCommand::Import, vec!["/tmp/x.csv".into(), "t".into()]),
        ];
        for case in cases {
            assert_eq!(roundtrip(&case), case);
        }
    }

    #[test]
    fn responses_roundtrip_through_cbor() {
        let cases = vec![
            Response::Ok,
            Response::SystemMsg("hi".into()),
            Response::SystemErr("bad".into()),
            Response::QueryExecutionError("nope".into()),
            Response::Shutdown(true),
            Response::QuietOk,
            Response::QuietErr,
        ];
        for case in cases {
            assert_eq!(roundtrip(&case), case);
        }
    }

    #[test]
    fn select_results_roundtrip_through_cbor() {
        let response = Response::QueryResult(QueryResult::Select {
            schema: Schema::new(vec![Attribute {
                name: "id".into(),
                dtype: DataType::Int,
                constraint: Constraint::PrimaryKey,
            }]),
            result: vec![Tuple {
                tid: 0,
                value_id: None,
                field_vals: vec![Value::Int(1)],
            }],
            paging_info: None,
        });
        assert_eq!(roundtrip(&response), response);
    }

    #[test]
    fn insert_results_roundtrip_through_cbor() {
        let response = Response::QueryResult(QueryResult::Insert {
            inserted: 3,
            table_name: "users".into(),
        });
        assert_eq!(roundtrip(&response), response);
    }

    #[test]
    fn tuple_decodes_without_a_value_id() {
        let tuple = Tuple {
            tid: 7,
            value_id: Some(ValueId {
                container_id: 1,
                segment_id: None,
                page_id: Some(2),
                slot_id: Some(3),
            }),
            field_vals: vec![Value::BigInt(1)],
        };
        // The server drops value_id on the way out; decoding must still work.
        assert_eq!(roundtrip(&tuple).value_id, None);
    }
}

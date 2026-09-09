//! Error types.

use std::fmt;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Anything that can go wrong talking to FairyDB.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Network or socket failure.
    Io(std::io::Error),
    /// A request could not be CBOR-encoded.
    Encode(serde_cbor::Error),
    /// A response could not be CBOR-decoded.
    Decode(serde_cbor::Error),
    /// The encoded request exceeds the server's fixed read buffer.
    RequestTooLarge {
        /// Encoded size of the request, in bytes.
        size: usize,
        /// Largest request the server can read.
        max: usize,
    },
    /// The server announced a response larger than this client will buffer.
    ResponseTooLarge {
        /// Length the server announced, in bytes.
        size: u64,
        /// Largest response this client will buffer.
        max: u64,
    },
    /// The server closed the connection, often because a command panicked its thread.
    ConnectionClosed,
    /// The server reported a system-level failure.
    Server(String),
    /// The server failed to execute a query.
    Query(String),
    /// A SQL command was issued before selecting a database.
    NotConnected,
    /// The statement is not implemented by FairyDB and would panic the server.
    Unsupported(String),
    /// Input held more than one statement; FairyDB would run only the first.
    MultipleStatements,
    /// A value could not be converted to the requested Rust type.
    Conversion {
        /// Column the value came from.
        column: String,
        /// Underlying conversion failure.
        source: ConversionError,
    },
    /// No column matched the requested name.
    UnknownColumn(String),
    /// A column index was past the end of the row.
    ColumnOutOfRange {
        /// Requested index.
        index: usize,
        /// Number of columns in the row.
        len: usize,
    },
    /// The server replied with something this request did not expect.
    UnexpectedResponse(String),
    /// An argument was rejected before reaching the server.
    InvalidArgument(String),
    /// The server is shutting down and will not serve further requests.
    ServerShutdown,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Encode(e) => write!(f, "failed to encode request: {e}"),
            Error::Decode(e) => write!(f, "failed to decode response: {e}"),
            Error::RequestTooLarge { size, max } => write!(
                f,
                "request is {size} bytes but the server reads at most {max}; \
                 split the statement into smaller ones"
            ),
            Error::ResponseTooLarge { size, max } => {
                write!(f, "server announced a {size} byte response, limit is {max}")
            }
            Error::ConnectionClosed => write!(
                f,
                "server closed the connection; it may have panicked handling the request"
            ),
            Error::Server(msg) => write!(f, "server error: {msg}"),
            Error::Query(msg) => write!(f, "query error: {msg}"),
            Error::NotConnected => write!(
                f,
                "not connected to a database; call `use_database` on this connection first"
            ),
            Error::Unsupported(what) => {
                write!(f, "FairyDB does not implement {what}")
            }
            Error::MultipleStatements => write!(
                f,
                "input holds more than one statement; FairyDB executes only the first, \
                 so send them one at a time"
            ),
            Error::Conversion { column, source } => write!(f, "column `{column}`: {source}"),
            Error::UnknownColumn(name) => write!(f, "no column named `{name}`"),
            Error::ColumnOutOfRange { index, len } => {
                write!(f, "column index {index} out of range for a row of {len}")
            }
            Error::UnexpectedResponse(what) => write!(f, "unexpected response: {what}"),
            Error::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
            Error::ServerShutdown => write!(f, "server is shutting down"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Encode(e) | Error::Decode(e) => Some(e),
            Error::Conversion { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset => {
                Error::ConnectionClosed
            }
            _ => Error::Io(e),
        }
    }
}

/// Why a [`Value`](crate::Value) did not fit the requested Rust type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionError {
    /// Rust type that was asked for.
    pub expected: &'static str,
    /// FairyDB type that was found.
    pub found: &'static str,
    /// Extra detail, when the types were compatible but the value was not.
    pub reason: Option<&'static str>,
}

impl ConversionError {
    pub(crate) fn new(expected: &'static str, found: &'static str) -> Self {
        ConversionError {
            expected,
            found,
            reason: None,
        }
    }

    pub(crate) fn with_reason(
        expected: &'static str,
        found: &'static str,
        reason: &'static str,
    ) -> Self {
        ConversionError {
            expected,
            found,
            reason: Some(reason),
        }
    }
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot read {} as {}", self.found, self.expected)?;
        if let Some(reason) = self.reason {
            write!(f, " ({reason})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConversionError {}

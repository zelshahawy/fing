//! `fing` is a client library for [FairyDB], a SQL-compliant OLTP storage engine.
//!
//! It speaks FairyDB's TCP wire protocol directly, so it needs nothing from the
//! FairyDB source tree at build time — add it as a dependency and connect.
//!
//! [FairyDB]: https://github.com/zelshahawy/fairydb

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;

pub use error::{ConversionError, Error, Result};

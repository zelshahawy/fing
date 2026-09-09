//! Query results.

use std::fmt;
use std::ops::Index;

use crate::convert::FromValue;
use crate::error::{Error, Result};
use crate::protocol::PagingInfo;
use crate::schema::Schema;
use crate::value::{DataType, Value};

/// Something that can name a column: a position, or a column name.
pub trait ColumnIndex {
    /// Resolve to a column position within `schema`.
    fn index_in(&self, schema: &Schema) -> Result<usize>;
}

impl ColumnIndex for usize {
    fn index_in(&self, schema: &Schema) -> Result<usize> {
        if *self < schema.len() {
            Ok(*self)
        } else {
            Err(Error::ColumnOutOfRange {
                index: *self,
                len: schema.len(),
            })
        }
    }
}

impl ColumnIndex for &str {
    fn index_in(&self, schema: &Schema) -> Result<usize> {
        schema
            .index_of(self)
            .ok_or_else(|| Error::UnknownColumn((*self).to_owned()))
    }
}

impl ColumnIndex for String {
    fn index_in(&self, schema: &Schema) -> Result<usize> {
        self.as_str().index_in(schema)
    }
}

impl ColumnIndex for &String {
    fn index_in(&self, schema: &Schema) -> Result<usize> {
        self.as_str().index_in(schema)
    }
}

/// The rows returned by a query, with their schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rows {
    schema: Schema,
    rows: Vec<Vec<Value>>,
    paging: Option<PagingInfo>,
}

impl Rows {
    pub(crate) fn new(schema: Schema, rows: Vec<Vec<Value>>, paging: Option<PagingInfo>) -> Self {
        Rows {
            schema,
            rows,
            paging,
        }
    }

    /// The result schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The result columns, in order.
    pub fn columns(&self) -> &[crate::schema::Attribute] {
        self.schema.columns()
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the result has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row at `index`.
    pub fn get(&self, index: usize) -> Option<Row<'_>> {
        self.rows.get(index).map(|values| Row {
            schema: &self.schema,
            values,
        })
    }

    /// Iterate over the rows.
    pub fn iter(&self) -> RowIter<'_> {
        RowIter {
            schema: &self.schema,
            inner: self.rows.iter(),
        }
    }

    /// Pagination metadata, when the server sent any.
    pub fn paging(&self) -> Option<&PagingInfo> {
        self.paging.as_ref()
    }

    /// Consume the result, yielding the raw values.
    pub fn into_values(self) -> Vec<Vec<Value>> {
        self.rows
    }
}

impl<'a> IntoIterator for &'a Rows {
    type Item = Row<'a>;
    type IntoIter = RowIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over [`Row`]s.
pub struct RowIter<'a> {
    schema: &'a Schema,
    inner: std::slice::Iter<'a, Vec<Value>>,
}

impl<'a> Iterator for RowIter<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|values| Row {
            schema: self.schema,
            values,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for RowIter<'_> {}

/// A single row, borrowed from its [`Rows`].
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    schema: &'a Schema,
    values: &'a [Value],
}

impl<'a> Row<'a> {
    /// Read a column as `T`.
    pub fn get<T: FromValue<'a>>(&self, column: impl ColumnIndex) -> Result<T> {
        let index = column.index_in(self.schema)?;
        let value = &self.values[index];
        T::from_value(value).map_err(|source| Error::Conversion {
            column: self
                .schema
                .column(index)
                .map_or_else(|| index.to_string(), |attribute| attribute.name.clone()),
            source,
        })
    }

    /// The raw value of a column.
    pub fn value(&self, column: impl ColumnIndex) -> Result<&'a Value> {
        let index = column.index_in(self.schema)?;
        Ok(&self.values[index])
    }

    /// All values in the row, in column order.
    pub fn values(&self) -> &'a [Value] {
        self.values
    }

    /// The schema this row belongs to.
    pub fn schema(&self) -> &'a Schema {
        self.schema
    }

    /// Number of columns.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the row has no columns.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Panics if the column does not exist; use [`Row::value`] to handle that.
impl Index<&str> for Row<'_> {
    type Output = Value;

    fn index(&self, name: &str) -> &Value {
        self.value(name).expect("no such column")
    }
}

/// Panics if the column does not exist; use [`Row::value`] to handle that.
impl Index<usize> for Row<'_> {
    type Output = Value;

    fn index(&self, index: usize) -> &Value {
        self.value(index).expect("column index out of range")
    }
}

fn is_right_aligned(dtype: &DataType) -> bool {
    matches!(
        dtype,
        DataType::BigInt | DataType::Int | DataType::SmallInt | DataType::Decimal(..)
    )
}

impl fmt::Display for Rows {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let columns = self.schema.columns();
        if columns.is_empty() {
            return write!(f, "({} rows)", self.len());
        }

        let cells: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| row.iter().map(Value::to_string).collect())
            .collect();

        let widths: Vec<usize> = columns
            .iter()
            .enumerate()
            .map(|(i, attribute)| {
                let widest = cells
                    .iter()
                    .filter_map(|row| row.get(i))
                    .map(|cell| cell.chars().count())
                    .max()
                    .unwrap_or(0);
                widest.max(attribute.name.chars().count())
            })
            .collect();

        let header = columns
            .iter()
            .zip(&widths)
            .map(|(attribute, width)| format!(" {:width$} ", attribute.name, width = width))
            .collect::<Vec<_>>()
            .join("|");
        writeln!(f, "{}", header.trim_end())?;

        let rule = widths
            .iter()
            .map(|width| "-".repeat(width + 2))
            .collect::<Vec<_>>()
            .join("+");
        writeln!(f, "{rule}")?;

        for row in &cells {
            let line = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let width = widths[i];
                    if columns.get(i).is_some_and(|a| is_right_aligned(&a.dtype)) {
                        format!(" {cell:>width$} ")
                    } else {
                        format!(" {cell:<width$} ")
                    }
                })
                .collect::<Vec<_>>()
                .join("|");
            writeln!(f, "{}", line.trim_end())?;
        }

        write!(
            f,
            "({} row{})",
            self.len(),
            if self.len() == 1 { "" } else { "s" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Attribute, Constraint};

    fn attribute(name: &str, dtype: DataType) -> Attribute {
        Attribute {
            name: name.into(),
            dtype,
            constraint: Constraint::None,
        }
    }

    fn sample() -> Rows {
        Rows::new(
            Schema::new(vec![
                attribute("id", DataType::Int),
                attribute("name", DataType::String),
            ]),
            vec![
                vec![Value::Int(1), Value::String("alice".into())],
                vec![Value::Int(2), Value::String("Ziad".into())],
            ],
            None,
        )
    }

    #[test]
    fn rows_are_iterable() {
        let rows = sample();
        assert_eq!(rows.len(), 2);
        let names: Vec<String> = rows.iter().map(|row| row.get("name").unwrap()).collect();
        assert_eq!(names, vec!["alice", "Ziad"]);
    }

    #[test]
    fn columns_read_by_name_and_position() {
        let rows = sample();
        let row = rows.get(0).unwrap();
        assert_eq!(row.get::<i64>("id").unwrap(), 1);
        assert_eq!(row.get::<i64>(0usize).unwrap(), 1);
        assert_eq!(row.get::<&str>("name").unwrap(), "alice");
    }

    #[test]
    fn indexing_yields_raw_values() {
        let rows = sample();
        let row = rows.get(1).unwrap();
        assert_eq!(row["name"], Value::String("Ziad".into()));
        assert_eq!(row[0], Value::Int(2));
    }

    #[test]
    fn unknown_columns_are_reported_by_name() {
        let rows = sample();
        let err = rows.get(0).unwrap().get::<i64>("nope").unwrap_err();
        assert!(matches!(err, Error::UnknownColumn(name) if name == "nope"));
    }

    #[test]
    fn out_of_range_columns_report_the_row_width() {
        let rows = sample();
        let err = rows.get(0).unwrap().get::<i64>(9usize).unwrap_err();
        assert!(matches!(err, Error::ColumnOutOfRange { index: 9, len: 2 }));
    }

    #[test]
    fn conversion_errors_name_the_column() {
        let rows = sample();
        let err = rows.get(0).unwrap().get::<i64>("name").unwrap_err();
        assert_eq!(err.to_string(), "column `name`: cannot read VARCHAR as i64");
    }

    #[test]
    fn display_renders_an_aligned_table() {
        let expected = concat!(
            " id | name\n",
            "----+-------\n",
            "  1 | alice\n",
            "  2 | Ziad\n",
            "(2 rows)",
        );
        assert_eq!(sample().to_string(), expected);
    }

    #[test]
    fn display_handles_an_empty_result() {
        let rows = Rows::new(
            Schema::new(vec![attribute("id", DataType::Int)]),
            Vec::new(),
            None,
        );
        assert_eq!(rows.to_string(), " id\n----\n(0 rows)");
    }

    #[test]
    fn display_uses_the_singular_for_one_row() {
        let rows = Rows::new(
            Schema::new(vec![attribute("id", DataType::Int)]),
            vec![vec![Value::Int(1)]],
            None,
        );
        assert!(rows.to_string().ends_with("(1 row)"));
    }
}

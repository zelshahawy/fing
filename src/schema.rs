//! Result and table schemas.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::value::DataType;

/// A constraint declared on a column.
///
/// Mirrors `common::Constraint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Constraint {
    /// No constraint.
    None,
    /// Part of the primary key.
    PrimaryKey,
    /// Unique.
    Unique,
    /// Not null.
    NotNull,
    /// Unique and not null.
    UniqueNotNull,
    /// Foreign key into the given container.
    ForeignKey(u16),
    /// Not-null foreign key into the given container.
    NotNullFKey(u16),
}

/// A named, typed column.
///
/// Mirrors `common::Attribute`; field names are part of the wire format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    /// Column name.
    pub name: String,
    /// Column type.
    pub dtype: DataType,
    /// Column constraint.
    pub constraint: Constraint,
}

impl Attribute {
    /// Whether this column is part of the primary key.
    pub fn is_primary_key(&self) -> bool {
        matches!(self.constraint, Constraint::PrimaryKey)
    }
}

/// An ordered list of columns describing a result set or table.
///
/// Mirrors `common::TableSchema`, which has a hand-written serde impl that
/// encodes it as a bare sequence of attributes rather than as a struct. The
/// impls below reproduce that exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    attributes: Vec<Attribute>,
}

impl Schema {
    /// Build a schema from its columns.
    pub fn new(attributes: Vec<Attribute>) -> Self {
        Schema { attributes }
    }

    /// The columns, in order.
    pub fn columns(&self) -> &[Attribute] {
        &self.attributes
    }

    /// Number of columns.
    pub fn len(&self) -> usize {
        self.attributes.len()
    }

    /// Whether the schema has no columns.
    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    /// The column at `index`.
    pub fn column(&self, index: usize) -> Option<&Attribute> {
        self.attributes.get(index)
    }

    /// Column names, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.attributes.iter().map(|a| a.name.as_str())
    }

    /// Position of the column called `name`.
    ///
    /// Matches on the full name first. Failing that, and only when the result is
    /// unambiguous, matches on the part after the last `.`, so a `users.id`
    /// column from a join can be found as `id`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        if let Some(i) = self.attributes.iter().position(|a| a.name == name) {
            return Some(i);
        }
        let mut found = None;
        for (i, attr) in self.attributes.iter().enumerate() {
            let unqualified = attr.name.rsplit('.').next().unwrap_or(&attr.name);
            if unqualified == name {
                if found.is_some() {
                    return None;
                }
                found = Some(i);
            }
        }
        found
    }
}

impl Serialize for Schema {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.attributes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Schema {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Schema, D::Error> {
        Vec::deserialize(deserializer).map(Schema::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, dtype: DataType) -> Attribute {
        Attribute {
            name: name.into(),
            dtype,
            constraint: Constraint::None,
        }
    }

    fn sample() -> Schema {
        Schema::new(vec![
            Attribute {
                name: "id".into(),
                dtype: DataType::Int,
                constraint: Constraint::PrimaryKey,
            },
            attr("name", DataType::String),
        ])
    }

    #[test]
    fn schema_encodes_as_a_bare_sequence_of_attributes() {
        let schema = sample();
        let as_schema = serde_cbor::to_vec(&schema).unwrap();
        let as_vec = serde_cbor::to_vec(&schema.columns()).unwrap();
        assert_eq!(as_schema, as_vec);
    }

    #[test]
    fn schema_roundtrips_through_cbor() {
        let schema = sample();
        let bytes = serde_cbor::to_vec(&schema).unwrap();
        let back: Schema = serde_cbor::from_slice(&bytes).unwrap();
        assert_eq!(back, schema);
    }

    #[test]
    fn constraints_roundtrip_through_cbor() {
        for case in [
            Constraint::None,
            Constraint::PrimaryKey,
            Constraint::Unique,
            Constraint::NotNull,
            Constraint::UniqueNotNull,
            Constraint::ForeignKey(3),
            Constraint::NotNullFKey(4),
        ] {
            let bytes = serde_cbor::to_vec(&case).unwrap();
            let back: Constraint = serde_cbor::from_slice(&bytes).unwrap();
            assert_eq!(back, case);
        }
    }

    #[test]
    fn columns_resolve_by_exact_name() {
        assert_eq!(sample().index_of("name"), Some(1));
        assert_eq!(sample().index_of("missing"), None);
    }

    #[test]
    fn qualified_columns_resolve_when_unambiguous() {
        let schema = Schema::new(vec![
            attr("users.id", DataType::Int),
            attr("orders.total", DataType::Int),
        ]);
        assert_eq!(schema.index_of("users.id"), Some(0));
        assert_eq!(schema.index_of("total"), Some(1));
    }

    #[test]
    fn ambiguous_qualified_columns_do_not_resolve() {
        let schema = Schema::new(vec![
            attr("users.id", DataType::Int),
            attr("orders.id", DataType::Int),
        ]);
        assert_eq!(schema.index_of("id"), None);
        assert_eq!(schema.index_of("orders.id"), Some(1));
    }
}

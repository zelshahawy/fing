//! Column values and SQL types.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A SQL type, as reported by the server in a schema.
///
/// Mirrors `common::DataType`; variant names and payloads are part of the wire
/// format and must not be renamed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    /// 64-bit signed integer.
    BigInt,
    /// 32-bit signed integer.
    Int,
    /// 16-bit signed integer.
    SmallInt,
    /// Fixed-width character data of the given length.
    Char(u8),
    /// Variable-length string.
    String,
    /// Fixed-point decimal with the given precision and scale.
    Decimal(u32, u32),
    /// Calendar date.
    Date,
    /// Boolean.
    Bool,
    /// The type of a NULL literal.
    Null,
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::BigInt => f.write_str("BIGINT"),
            DataType::Int => f.write_str("INT"),
            DataType::SmallInt => f.write_str("SMALLINT"),
            DataType::Char(n) => write!(f, "CHAR({n})"),
            DataType::String => f.write_str("VARCHAR"),
            DataType::Decimal(p, s) => write!(f, "DECIMAL({p},{s})"),
            DataType::Date => f.write_str("DATE"),
            DataType::Bool => f.write_str("BOOL"),
            DataType::Null => f.write_str("NULL"),
        }
    }
}

/// A single column value.
///
/// Mirrors `common::Field`; variant names and payloads are part of the wire
/// format and must not be renamed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Value {
    /// 64-bit signed integer.
    BigInt(i64),
    /// 32-bit signed integer.
    Int(i32),
    /// 16-bit signed integer.
    SmallInt(i16),
    /// Fixed-width character data, with its declared width.
    Char(u8, String),
    /// Variable-length string.
    String(String),
    /// Fixed-point decimal held as an unscaled integer plus a scale.
    Decimal(i64, u32),
    /// Days since 1970-01-01.
    Date(i64),
    /// Boolean.
    Bool(bool),
    /// SQL NULL.
    Null,
}

impl Value {
    /// Name of this value's type, for diagnostics.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::BigInt(_) => "BIGINT",
            Value::Int(_) => "INT",
            Value::SmallInt(_) => "SMALLINT",
            Value::Char(..) => "CHAR",
            Value::String(_) => "VARCHAR",
            Value::Decimal(..) => "DECIMAL",
            Value::Date(_) => "DATE",
            Value::Bool(_) => "BOOL",
            Value::Null => "NULL",
        }
    }

    /// The [`DataType`] corresponding to this value.
    pub fn data_type(&self) -> DataType {
        match self {
            Value::BigInt(_) => DataType::BigInt,
            Value::Int(_) => DataType::Int,
            Value::SmallInt(_) => DataType::SmallInt,
            Value::Char(n, _) => DataType::Char(*n),
            Value::String(_) => DataType::String,
            Value::Decimal(whole, scale) => DataType::Decimal(precision_of(*whole), *scale),
            Value::Date(_) => DataType::Date,
            Value::Bool(_) => DataType::Bool,
            Value::Null => DataType::Null,
        }
    }

    /// Whether this value is SQL NULL.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Whether this value is one of the numeric types.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Value::BigInt(_) | Value::Int(_) | Value::SmallInt(_) | Value::Decimal(..)
        )
    }

    /// This value as an `i64`, if it is an integer type.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::BigInt(i) => Some(*i),
            Value::Int(i) => Some(*i as i64),
            Value::SmallInt(i) => Some(*i as i64),
            _ => None,
        }
    }

    /// This value as an `f64`, if it is numeric. Decimals are scaled.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::BigInt(i) => Some(*i as f64),
            Value::Int(i) => Some(*i as f64),
            Value::SmallInt(i) => Some(*i as f64),
            Value::Decimal(whole, scale) => Some(*whole as f64 / 10f64.powi(*scale as i32)),
            _ => None,
        }
    }

    /// This value as a `&str`, if it is textual.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::Char(_, s) => Some(s),
            _ => None,
        }
    }

    /// This value as a `bool`, if it is boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// This value as a `(year, month, day)` triple, if it is a date.
    pub fn as_date_ymd(&self) -> Option<(i32, u32, u32)> {
        match self {
            Value::Date(days) => Some(civil_from_days(*days)),
            _ => None,
        }
    }
}

fn precision_of(whole: i64) -> u32 {
    let digits = whole.unsigned_abs().to_string().len() as u32;
    digits.max(1)
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::BigInt(i) => write!(f, "{i}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::SmallInt(i) => write!(f, "{i}"),
            Value::Char(_, s) | Value::String(s) => f.write_str(s),
            Value::Decimal(whole, scale) => f.write_str(&format_decimal(*whole, *scale)),
            Value::Date(days) => {
                let (y, m, d) = civil_from_days(*days);
                write!(f, "{y:04}-{m:02}-{d:02}")
            }
            Value::Bool(b) => write!(f, "{b}"),
            Value::Null => f.write_str("NULL"),
        }
    }
}

fn format_decimal(whole: i64, scale: u32) -> String {
    let digits = whole.unsigned_abs().to_string();
    let sign = if whole < 0 { "-" } else { "" };
    let scale = scale as usize;

    if scale == 0 {
        return format!("{sign}{digits}");
    }
    if digits.len() > scale {
        let split = digits.len() - scale;
        format!("{sign}{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("{sign}0.{digits:0>scale$}")
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: &Value) -> Value {
        let bytes = serde_cbor::to_vec(v).unwrap();
        serde_cbor::from_slice(&bytes).unwrap()
    }

    #[test]
    fn values_roundtrip_through_cbor() {
        let cases = vec![
            Value::BigInt(-9_000_000_000),
            Value::Int(-42),
            Value::SmallInt(7),
            Value::Char(5, "abc".into()),
            Value::String("hello".into()),
            Value::Decimal(-12345, 4),
            Value::Date(20_000),
            Value::Bool(true),
            Value::Null,
        ];
        for case in cases {
            assert_eq!(roundtrip(&case), case);
        }
    }

    #[test]
    fn data_types_roundtrip_through_cbor() {
        let cases = vec![
            DataType::BigInt,
            DataType::Int,
            DataType::SmallInt,
            DataType::Char(10),
            DataType::String,
            DataType::Decimal(10, 4),
            DataType::Date,
            DataType::Bool,
            DataType::Null,
        ];
        for case in cases {
            let bytes = serde_cbor::to_vec(&case).unwrap();
            let back: DataType = serde_cbor::from_slice(&bytes).unwrap();
            assert_eq!(back, case);
        }
    }

    #[test]
    fn unit_variants_encode_as_bare_strings() {
        assert_eq!(
            serde_cbor::to_vec(&Value::Null).unwrap(),
            serde_cbor::to_vec(&"Null").unwrap()
        );
    }

    #[test]
    fn decimals_format_with_scale() {
        assert_eq!(format_decimal(123456, 4), "12.3456");
        assert_eq!(format_decimal(5, 2), "0.05");
        assert_eq!(format_decimal(0, 2), "0.00");
        assert_eq!(format_decimal(150, 0), "150");
    }

    #[test]
    fn decimals_format_negatives() {
        assert_eq!(format_decimal(-123456, 4), "-12.3456");
        assert_eq!(format_decimal(-5, 2), "-0.05");
        assert_eq!(format_decimal(-1, 0), "-1");
    }

    #[test]
    fn dates_convert_from_epoch_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // 2024 is a leap year.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn dates_display_zero_padded() {
        assert_eq!(Value::Date(0).to_string(), "1970-01-01");
        assert_eq!(Value::Date(19_782).to_string(), "2024-02-29");
    }

    #[test]
    fn numeric_accessors() {
        assert_eq!(Value::SmallInt(3).as_i64(), Some(3));
        assert_eq!(Value::Decimal(12345, 2).as_f64(), Some(123.45));
        assert_eq!(Value::String("x".into()).as_i64(), None);
    }
}

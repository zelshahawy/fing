//! Converting [`Value`]s into Rust types.

use crate::error::ConversionError;
use crate::value::Value;

/// A Rust type a column value can be read into.
///
/// Integer types widen freely and narrow only when the value fits. `Option<T>`
/// accepts SQL NULL; the bare types reject it.
pub trait FromValue<'a>: Sized {
    /// Read `value`, or explain why it does not fit.
    fn from_value(value: &'a Value) -> Result<Self, ConversionError>;
}

fn out_of_range(expected: &'static str, value: &Value) -> ConversionError {
    ConversionError::with_reason(expected, value.type_name(), "value out of range")
}

macro_rules! int_from_value {
    ($ty:ty, $name:literal) => {
        impl<'a> FromValue<'a> for $ty {
            fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
                let wide = value
                    .as_i64()
                    .ok_or_else(|| ConversionError::new($name, value.type_name()))?;
                <$ty>::try_from(wide).map_err(|_| out_of_range($name, value))
            }
        }
    };
}

int_from_value!(i64, "i64");
int_from_value!(i32, "i32");
int_from_value!(i16, "i16");
int_from_value!(i8, "i8");
int_from_value!(u64, "u64");
int_from_value!(u32, "u32");
int_from_value!(u16, "u16");
int_from_value!(u8, "u8");
int_from_value!(usize, "usize");
int_from_value!(isize, "isize");

impl<'a> FromValue<'a> for f64 {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        value
            .as_f64()
            .ok_or_else(|| ConversionError::new("f64", value.type_name()))
    }
}

impl<'a> FromValue<'a> for bool {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        value
            .as_bool()
            .ok_or_else(|| ConversionError::new("bool", value.type_name()))
    }
}

impl<'a> FromValue<'a> for &'a str {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        value
            .as_str()
            .ok_or_else(|| ConversionError::new("&str", value.type_name()))
    }
}

impl<'a> FromValue<'a> for String {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| ConversionError::new("String", value.type_name()))
    }
}

impl<'a> FromValue<'a> for &'a Value {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        Ok(value)
    }
}

impl<'a> FromValue<'a> for Value {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        Ok(value.clone())
    }
}

impl<'a, T: FromValue<'a>> FromValue<'a> for Option<T> {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        match value {
            Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

#[cfg(feature = "chrono")]
impl<'a> FromValue<'a> for chrono::NaiveDate {
    fn from_value(value: &'a Value) -> Result<Self, ConversionError> {
        let (year, month, day) = value
            .as_date_ymd()
            .ok_or_else(|| ConversionError::new("chrono::NaiveDate", value.type_name()))?;
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| out_of_range("chrono::NaiveDate", value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a, T: FromValue<'a>>(value: &'a Value) -> Result<T, ConversionError> {
        T::from_value(value)
    }

    #[test]
    fn integers_widen() {
        assert_eq!(get::<i64>(&Value::SmallInt(7)).unwrap(), 7);
        assert_eq!(get::<i64>(&Value::Int(7)).unwrap(), 7);
        assert_eq!(get::<i64>(&Value::BigInt(7)).unwrap(), 7);
    }

    #[test]
    fn integers_narrow_when_they_fit() {
        assert_eq!(get::<i16>(&Value::BigInt(7)).unwrap(), 7);
        assert_eq!(get::<u8>(&Value::Int(200)).unwrap(), 200);
    }

    #[test]
    fn narrowing_out_of_range_is_an_error() {
        let err = get::<i16>(&Value::BigInt(100_000)).unwrap_err();
        assert_eq!(err.reason, Some("value out of range"));
        assert!(get::<u32>(&Value::Int(-1)).is_err());
    }

    #[test]
    fn decimals_read_as_floats_but_not_integers() {
        assert_eq!(get::<f64>(&Value::Decimal(12345, 2)).unwrap(), 123.45);
        assert!(get::<i64>(&Value::Decimal(12345, 2)).is_err());
    }

    #[test]
    fn strings_read_from_varchar_and_char() {
        assert_eq!(get::<&str>(&Value::String("a".into())).unwrap(), "a");
        assert_eq!(get::<String>(&Value::Char(1, "b".into())).unwrap(), "b");
    }

    #[test]
    fn type_mismatches_name_both_types() {
        let err = get::<i64>(&Value::String("a".into())).unwrap_err();
        assert_eq!(err.expected, "i64");
        assert_eq!(err.found, "VARCHAR");
        assert_eq!(err.to_string(), "cannot read VARCHAR as i64");
    }

    #[test]
    fn null_needs_an_option() {
        assert!(get::<i64>(&Value::Null).is_err());
        assert_eq!(get::<Option<i64>>(&Value::Null).unwrap(), None);
        assert_eq!(get::<Option<i64>>(&Value::Int(1)).unwrap(), Some(1));
    }

    #[test]
    fn values_pass_through() {
        let value = Value::Bool(true);
        assert_eq!(get::<Value>(&value).unwrap(), value);
        assert_eq!(get::<&Value>(&value).unwrap(), &value);
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn dates_read_as_chrono() {
        let date = get::<chrono::NaiveDate>(&Value::Date(19_782)).unwrap();
        assert_eq!(date.to_string(), "2024-02-29");
    }
}

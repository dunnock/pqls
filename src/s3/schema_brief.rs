use parquet::basic::{ConvertedType, LogicalType, TimeUnit, Type as PhysicalType};
use parquet::schema::types::Type;

/// Returns a one-line `(name: type, ...)` summary of the top-level schema columns.
///
/// Takes the root `MessageType` (e.g. from `file_metadata.schema()`).
pub fn schema_brief(schema: &Type) -> String {
    let fields = schema.get_fields();
    if fields.is_empty() {
        return "()".to_string();
    }
    let parts: Vec<String> = fields
        .iter()
        .map(|f| format!("{}: {}", f.name(), type_name(f)))
        .collect();
    format!("({})", parts.join(", "))
}

/// Maps a parquet `Type` node to a brief human-readable type name.
///
/// Nested groups are capped at depth 1: a `LIST` of structs renders as
/// `list<struct>`, not `list<struct{...}>`.
pub fn type_name(field: &Type) -> String {
    if field.is_group() {
        return group_type_name(field);
    }
    primitive_type_name(field)
}

fn group_type_name(field: &Type) -> String {
    let basic = field.get_basic_info();
    let lt = basic.logical_type();
    let ct = basic.converted_type();
    let fields = field.get_fields();

    // LIST
    if matches!(lt, Some(LogicalType::List)) || ct == ConvertedType::LIST {
        let item = fields
            .first()
            .map(|outer| {
                // Standard Parquet LIST encoding: LIST → repeated group "list" → optional "item"
                if outer.is_group() {
                    outer
                        .get_fields()
                        .first()
                        .map(|item| type_name(item))
                        .unwrap_or_else(|| "unknown".to_string())
                } else {
                    type_name(outer)
                }
            })
            .unwrap_or_else(|| "unknown".to_string());
        return format!("list<{item}>");
    }

    // MAP
    if matches!(lt, Some(LogicalType::Map))
        || ct == ConvertedType::MAP
        || ct == ConvertedType::MAP_KEY_VALUE
    {
        let (k, v) = fields
            .first()
            .filter(|f| f.is_group())
            .and_then(|kv| {
                let kv_fields = kv.get_fields();
                let k = kv_fields.first().map(|f| type_name(f));
                let v = kv_fields.get(1).map(|f| type_name(f));
                k.zip(v)
            })
            .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
        return format!("map<{k},{v}>");
    }

    "struct".to_string()
}

fn primitive_type_name(field: &Type) -> String {
    let physical = field.get_physical_type();
    let basic = field.get_basic_info();
    let lt = basic.logical_type();
    let ct = basic.converted_type();
    let precision = field.get_precision();
    let scale = field.get_scale();
    let type_len = match field {
        Type::PrimitiveType { type_length, .. } => *type_length,
        _ => -1,
    };

    match physical {
        PhysicalType::BOOLEAN => "bool".to_string(),

        PhysicalType::INT32 => match lt {
            Some(LogicalType::Date) => "date".to_string(),
            Some(LogicalType::Integer {
                bit_width: 8,
                is_signed: true,
            }) => "int8".to_string(),
            Some(LogicalType::Integer {
                bit_width: 16,
                is_signed: true,
            }) => "int16".to_string(),
            Some(LogicalType::Integer {
                bit_width: 32,
                is_signed: true,
            }) => "int32".to_string(),
            Some(LogicalType::Integer {
                bit_width: 8,
                is_signed: false,
            }) => "uint8".to_string(),
            Some(LogicalType::Integer {
                bit_width: 16,
                is_signed: false,
            }) => "uint16".to_string(),
            Some(LogicalType::Integer {
                bit_width: 32,
                is_signed: false,
            }) => "uint32".to_string(),
            Some(LogicalType::Decimal { .. }) => format!("decimal({precision},{scale})"),
            Some(LogicalType::Time {
                unit: TimeUnit::MILLIS(_),
                ..
            }) => "time[ms]".to_string(),
            _ => match ct {
                ConvertedType::DATE => "date".to_string(),
                ConvertedType::INT_8 => "int8".to_string(),
                ConvertedType::INT_16 => "int16".to_string(),
                ConvertedType::UINT_8 => "uint8".to_string(),
                ConvertedType::UINT_16 => "uint16".to_string(),
                ConvertedType::UINT_32 => "uint32".to_string(),
                ConvertedType::DECIMAL => format!("decimal({precision},{scale})"),
                ConvertedType::TIME_MILLIS => "time[ms]".to_string(),
                _ => "int32".to_string(),
            },
        },

        PhysicalType::INT64 => match lt {
            Some(LogicalType::Integer {
                bit_width: 64,
                is_signed: true,
            }) => "int64".to_string(),
            Some(LogicalType::Integer {
                bit_width: 64,
                is_signed: false,
            }) => "uint64".to_string(),
            Some(LogicalType::Timestamp {
                unit: TimeUnit::MILLIS(_),
                is_adjusted_to_u_t_c: true,
            }) => "timestamp[ms,UTC]".to_string(),
            Some(LogicalType::Timestamp {
                unit: TimeUnit::MICROS(_),
                is_adjusted_to_u_t_c: true,
            }) => "timestamp[us,UTC]".to_string(),
            Some(LogicalType::Timestamp {
                unit: TimeUnit::NANOS(_),
                is_adjusted_to_u_t_c: true,
            }) => "timestamp[ns,UTC]".to_string(),
            Some(LogicalType::Timestamp {
                unit: TimeUnit::MILLIS(_),
                is_adjusted_to_u_t_c: false,
            }) => "timestamp[ms]".to_string(),
            Some(LogicalType::Timestamp {
                unit: TimeUnit::MICROS(_),
                is_adjusted_to_u_t_c: false,
            }) => "timestamp[us]".to_string(),
            Some(LogicalType::Timestamp {
                unit: TimeUnit::NANOS(_),
                is_adjusted_to_u_t_c: false,
            }) => "timestamp[ns]".to_string(),
            Some(LogicalType::Time {
                unit: TimeUnit::MICROS(_),
                ..
            }) => "time[us]".to_string(),
            Some(LogicalType::Decimal { .. }) => format!("decimal({precision},{scale})"),
            _ => match ct {
                ConvertedType::UINT_64 => "uint64".to_string(),
                ConvertedType::DECIMAL => format!("decimal({precision},{scale})"),
                ConvertedType::TIMESTAMP_MILLIS => "timestamp[ms]".to_string(),
                ConvertedType::TIMESTAMP_MICROS => "timestamp[us]".to_string(),
                ConvertedType::TIME_MICROS => "time[us]".to_string(),
                _ => "int64".to_string(),
            },
        },

        PhysicalType::INT96 => "timestamp".to_string(),
        PhysicalType::FLOAT => "float32".to_string(),
        PhysicalType::DOUBLE => "float64".to_string(),

        PhysicalType::BYTE_ARRAY => match lt {
            Some(LogicalType::String) => "utf8".to_string(),
            Some(LogicalType::Json) => "json".to_string(),
            Some(LogicalType::Bson) => "bson".to_string(),
            Some(LogicalType::Enum) => "enum".to_string(),
            Some(LogicalType::Decimal { .. }) => format!("decimal({precision},{scale})"),
            _ => match ct {
                ConvertedType::UTF8 => "utf8".to_string(),
                ConvertedType::JSON => "json".to_string(),
                ConvertedType::BSON => "bson".to_string(),
                ConvertedType::ENUM => "enum".to_string(),
                ConvertedType::DECIMAL => format!("decimal({precision},{scale})"),
                _ => "bytes".to_string(),
            },
        },

        PhysicalType::FIXED_LEN_BYTE_ARRAY => match lt {
            Some(LogicalType::Uuid) => "uuid".to_string(),
            Some(LogicalType::Decimal { .. }) => format!("decimal({precision},{scale})"),
            Some(LogicalType::Float16) => "float16".to_string(),
            _ => match ct {
                ConvertedType::DECIMAL => format!("decimal({precision},{scale})"),
                _ => format!("bytes({type_len})"),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::basic::{LogicalType, Repetition, TimeUnit, Type as PhysicalType};
    use parquet::schema::types::Type;
    use std::sync::Arc;

    fn primitive(name: &str, physical: PhysicalType, logical: Option<LogicalType>) -> Arc<Type> {
        let mut builder = Type::primitive_type_builder(name, physical)
            .with_repetition(Repetition::OPTIONAL);
        if let Some(lt) = logical {
            builder = builder.with_logical_type(Some(lt));
        }
        Arc::new(builder.build().unwrap())
    }

    fn primitive_with_len(
        name: &str,
        physical: PhysicalType,
        logical: Option<LogicalType>,
        len: i32,
    ) -> Arc<Type> {
        let mut builder = Type::primitive_type_builder(name, physical)
            .with_repetition(Repetition::OPTIONAL)
            .with_length(len);
        if let Some(lt) = logical {
            builder = builder.with_logical_type(Some(lt));
        }
        Arc::new(builder.build().unwrap())
    }

    fn make_schema(fields: Vec<Arc<Type>>) -> Type {
        Type::group_type_builder("schema")
            .with_fields(fields)
            .build()
            .unwrap()
    }

    #[test]
    fn test_schema_brief_basic() {
        let schema = make_schema(vec![
            primitive("id", PhysicalType::INT64, None),
            primitive(
                "ts",
                PhysicalType::INT64,
                Some(LogicalType::Timestamp {
                    is_adjusted_to_u_t_c: true,
                    unit: TimeUnit::MILLIS(Default::default()),
                }),
            ),
            primitive("price", PhysicalType::DOUBLE, None),
            primitive("qty", PhysicalType::INT32, None),
            primitive("side", PhysicalType::BYTE_ARRAY, Some(LogicalType::String)),
        ]);

        let brief = schema_brief(&schema);
        assert_eq!(
            brief,
            "(id: int64, ts: timestamp[ms,UTC], price: float64, qty: int32, side: utf8)"
        );
    }

    #[test]
    fn test_type_name_bool() {
        let f = primitive("x", PhysicalType::BOOLEAN, None);
        assert_eq!(type_name(&f), "bool");
    }

    #[test]
    fn test_type_name_int32_date() {
        let f = primitive("d", PhysicalType::INT32, Some(LogicalType::Date));
        assert_eq!(type_name(&f), "date");
    }

    #[test]
    fn test_type_name_int32_signed_integers() {
        let f8 = primitive(
            "x",
            PhysicalType::INT32,
            Some(LogicalType::Integer {
                bit_width: 8,
                is_signed: true,
            }),
        );
        let f16 = primitive(
            "x",
            PhysicalType::INT32,
            Some(LogicalType::Integer {
                bit_width: 16,
                is_signed: true,
            }),
        );
        let f32_ = primitive(
            "x",
            PhysicalType::INT32,
            Some(LogicalType::Integer {
                bit_width: 32,
                is_signed: true,
            }),
        );
        assert_eq!(type_name(&f8), "int8");
        assert_eq!(type_name(&f16), "int16");
        assert_eq!(type_name(&f32_), "int32");
    }

    #[test]
    fn test_type_name_int32_unsigned_integers() {
        let f8 = primitive(
            "x",
            PhysicalType::INT32,
            Some(LogicalType::Integer {
                bit_width: 8,
                is_signed: false,
            }),
        );
        let f16 = primitive(
            "x",
            PhysicalType::INT32,
            Some(LogicalType::Integer {
                bit_width: 16,
                is_signed: false,
            }),
        );
        let f32_ = primitive(
            "x",
            PhysicalType::INT32,
            Some(LogicalType::Integer {
                bit_width: 32,
                is_signed: false,
            }),
        );
        assert_eq!(type_name(&f8), "uint8");
        assert_eq!(type_name(&f16), "uint16");
        assert_eq!(type_name(&f32_), "uint32");
    }

    #[test]
    fn test_type_name_int64_timestamps() {
        let ms_utc = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Timestamp {
                is_adjusted_to_u_t_c: true,
                unit: TimeUnit::MILLIS(Default::default()),
            }),
        );
        let us_utc = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Timestamp {
                is_adjusted_to_u_t_c: true,
                unit: TimeUnit::MICROS(Default::default()),
            }),
        );
        let ns_utc = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Timestamp {
                is_adjusted_to_u_t_c: true,
                unit: TimeUnit::NANOS(Default::default()),
            }),
        );
        let ms = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Timestamp {
                is_adjusted_to_u_t_c: false,
                unit: TimeUnit::MILLIS(Default::default()),
            }),
        );
        let us = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Timestamp {
                is_adjusted_to_u_t_c: false,
                unit: TimeUnit::MICROS(Default::default()),
            }),
        );
        let ns = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Timestamp {
                is_adjusted_to_u_t_c: false,
                unit: TimeUnit::NANOS(Default::default()),
            }),
        );
        assert_eq!(type_name(&ms_utc), "timestamp[ms,UTC]");
        assert_eq!(type_name(&us_utc), "timestamp[us,UTC]");
        assert_eq!(type_name(&ns_utc), "timestamp[ns,UTC]");
        assert_eq!(type_name(&ms), "timestamp[ms]");
        assert_eq!(type_name(&us), "timestamp[us]");
        assert_eq!(type_name(&ns), "timestamp[ns]");
    }

    #[test]
    fn test_type_name_float_double() {
        let f = primitive("f", PhysicalType::FLOAT, None);
        let d = primitive("d", PhysicalType::DOUBLE, None);
        assert_eq!(type_name(&f), "float32");
        assert_eq!(type_name(&d), "float64");
    }

    #[test]
    fn test_type_name_byte_array_utf8() {
        let f = primitive("s", PhysicalType::BYTE_ARRAY, Some(LogicalType::String));
        assert_eq!(type_name(&f), "utf8");
    }

    #[test]
    fn test_type_name_byte_array_raw() {
        let f = primitive("b", PhysicalType::BYTE_ARRAY, None);
        assert_eq!(type_name(&f), "bytes");
    }

    #[test]
    fn test_type_name_fixed_len_uuid() {
        let f = primitive_with_len(
            "u",
            PhysicalType::FIXED_LEN_BYTE_ARRAY,
            Some(LogicalType::Uuid),
            16,
        );
        assert_eq!(type_name(&f), "uuid");
    }

    #[test]
    fn test_type_name_fixed_len_bytes() {
        let f = primitive_with_len("b", PhysicalType::FIXED_LEN_BYTE_ARRAY, None, 12);
        assert_eq!(type_name(&f), "bytes(12)");
    }

    #[test]
    fn test_type_name_int96() {
        let f = primitive("t", PhysicalType::INT96, None);
        assert_eq!(type_name(&f), "timestamp");
    }

    #[test]
    fn test_type_name_int64_uint64() {
        let f = primitive(
            "x",
            PhysicalType::INT64,
            Some(LogicalType::Integer {
                bit_width: 64,
                is_signed: false,
            }),
        );
        assert_eq!(type_name(&f), "uint64");
    }

    #[test]
    fn test_type_name_time_millis() {
        let f = primitive(
            "t",
            PhysicalType::INT32,
            Some(LogicalType::Time {
                is_adjusted_to_u_t_c: false,
                unit: TimeUnit::MILLIS(Default::default()),
            }),
        );
        assert_eq!(type_name(&f), "time[ms]");
    }

    #[test]
    fn test_type_name_time_micros() {
        let f = primitive(
            "t",
            PhysicalType::INT64,
            Some(LogicalType::Time {
                is_adjusted_to_u_t_c: false,
                unit: TimeUnit::MICROS(Default::default()),
            }),
        );
        assert_eq!(type_name(&f), "time[us]");
    }

    #[test]
    fn test_schema_brief_empty() {
        let schema = make_schema(vec![]);
        assert_eq!(schema_brief(&schema), "()");
    }
}

//! Conversion between typed entity documents and SQLite values and rows.

use std::collections::BTreeMap;

use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Row,
};

use super::{EntityDocument, EntitySchema, PersistentEntity, SqlKey};
use crate::persistence::typed_store::{FieldType, StoredValue};

/// Converts a normalized primary key SQL value back into its stored form.
///
/// # Arguments
/// * `field` - Column name used in type-mismatch errors.
/// * `value` - Primary key cell value produced by [`SqlKey::from_sql_value`].
///
/// # Errors
/// Returns an error when the cell is neither text nor integer.
fn stored_from_key_sql(field: &str, value: SqlValue) -> anyhow::Result<StoredValue> {
    Ok(match value {
        SqlValue::Text(value) => StoredValue::String(value),
        SqlValue::Integer(value) => StoredValue::Integer(value),
        _ => anyhow::bail!("primary key column '{field}' holds an unexpected SQLite type"),
    })
}

/// Converts a stored value into its SQLite representation.
///
/// # Arguments
/// * `value` - Stored value to encode.
///
/// # Errors
/// Returns an error when a date-time does not fit the SQLite integer range or JSON serialization fails.
pub(super) fn to_sql_value(value: &StoredValue) -> anyhow::Result<SqlValue> {
    Ok(match value {
        StoredValue::String(value) => SqlValue::Text(value.clone()),
        StoredValue::Integer(value) => SqlValue::Integer(*value),
        StoredValue::Boolean(value) => SqlValue::Integer(i64::from(*value)),
        StoredValue::DateTime(value) => SqlValue::Integer(
            i64::try_from(*value)
                .map_err(|_| anyhow::anyhow!("date-time value is out of SQLite integer range"))?,
        ),
        StoredValue::Json(value) => SqlValue::Text(serde_json::to_string(value)?),
    })
}
/// Converts a SQLite cell into the stored value expected by a field type.
///
/// # Arguments
/// * `field` - Column name used in errors.
/// * `field_type` - Logical type expected for the column.
/// * `cell` - Raw SQLite cell to decode.
///
/// # Errors
/// Returns an error when the cell type or value does not match the expected field type.
fn stored_from_cell(
    field: &str,
    field_type: FieldType,
    cell: ValueRef<'_>,
) -> anyhow::Result<Option<StoredValue>> {
    let stored = match cell {
        ValueRef::Null => return Ok(None),
        ValueRef::Text(value) => match field_type {
            FieldType::String => StoredValue::String(std::str::from_utf8(value)?.to_string()),
            FieldType::Json => StoredValue::Json(serde_json::from_slice(value)?),
            _ => anyhow::bail!("sqlite column '{field}' holds text but expects {field_type:?}"),
        },
        ValueRef::Integer(value) => match field_type {
            FieldType::Integer => StoredValue::Integer(value),
            FieldType::Boolean => match value {
                0 => StoredValue::Boolean(false),
                1 => StoredValue::Boolean(true),
                _ => anyhow::bail!("sqlite boolean column '{field}' holds {value}"),
            },
            FieldType::DateTime => StoredValue::DateTime(i128::from(value)),
            _ => {
                anyhow::bail!("sqlite column '{field}' holds an integer but expects {field_type:?}")
            }
        },
        ValueRef::Real(value) => {
            anyhow::bail!("sqlite column '{field}' holds an unexpected real value ({value})")
        }
        ValueRef::Blob(_) => {
            anyhow::bail!("sqlite column '{field}' holds an unexpected blob value")
        }
    };
    Ok(Some(stored))
}
/// Reads one SQLite row into an entity document.
///
/// # Arguments
/// * `schema` - Entity schema defining field order and types.
/// * `row` - SQLite row positioned on the entity table.
///
/// # Errors
/// Returns an error when a row cell cannot be read, decoded, or matched to the primary key.
pub(super) fn read_row<E: PersistentEntity>(
    schema: &EntitySchema,
    row: &Row<'_>,
) -> anyhow::Result<EntityDocument>
where
    E::Key: SqlKey,
{
    let primary_key_fields = schema.primary_key_fields();
    let mut fields = BTreeMap::new();
    for (index, field) in schema.fields().iter().enumerate() {
        let cell = row.get_ref(index)?;
        let stored = if let Some(key_index) = primary_key_fields
            .iter()
            .position(|key| key.name() == field.name())
        {
            Some(stored_from_key_sql(
                field.name(),
                <E::Key as SqlKey>::from_sql_value(key_index, cell)?,
            )?)
        } else {
            stored_from_cell(field.name(), field.field_type(), cell)?
        };
        if let Some(stored) = stored {
            fields.insert(field.name().to_string(), stored);
        }
    }
    Ok(EntityDocument { fields })
}

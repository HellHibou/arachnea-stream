//! Composite text keys retaining explicit domain identity columns.

use arachnea_core::persistence::sqlite_store::SqlValue;
use arachnea_core::persistence::sqlite_store::ValueRef;
use arachnea_core::persistence::{EntityKey, FieldType, SqlKey};

macro_rules! key {
    ($name:ident, $length:expr) => {
        /// Ordered, unambiguous text components of a composite primary key.
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(
            /// Components in schema primary-key order.
            pub [String; $length],
        );
        impl EntityKey for $name {
            fn field_types() -> &'static [FieldType] {
                &[FieldType::String; $length]
            }
            fn storage_key(&self) -> String {
                serde_json::to_string(&self.0).expect("string array serialization")
            }
        }
        impl SqlKey for $name {
            fn physical_types() -> &'static [&'static str] {
                &["TEXT"; $length]
            }
            fn to_sql_values(&self) -> Vec<SqlValue> {
                self.0.iter().cloned().map(SqlValue::Text).collect()
            }
            fn from_sql_value(index: usize, cell: ValueRef<'_>) -> anyhow::Result<SqlValue> {
                anyhow::ensure!(index < $length, "composite key component out of range");
                <String as SqlKey>::from_sql_value(0, cell)
            }
        }
    };
}
key!(PairKey, 2);
key!(TripleKey, 3);

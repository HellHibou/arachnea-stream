//! Durable operation receipts in the common store, without service-table dependencies.

use anyhow::Result;
use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, PersistentEntity,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Persisted recovery status; transient failures retain their durable intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Accepted intent not yet executed.
    Pending,
    /// Execution has started and may need recovery after interruption.
    Running,
    /// A storage failure requires another attempt.
    RetryPending,
    /// All steps are complete or superseded by deletion.
    Completed,
}

/// Last completed boundary of a coordinated operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStep {
    /// Intent durably recorded before execution.
    Prepared,
    /// Common-domain changes applied.
    CommonApplied,
    /// Streaming-domain changes applied.
    StreamingApplied,
    /// Final common receipt recorded.
    Completed,
}

/// Durable, versioned coordination intent and replay receipt.
#[derive(Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Caller-supplied stable operation UUID.
    pub operation_id: Uuid,
    /// Target account and owner of profile operations.
    pub user_id: Uuid,
    /// Common profile UUID, absent for whole-account deletion.
    pub profile_id: Option<Uuid>,
    /// Version of the coordination payload.
    pub schema_version: i64,
    /// Stable application operation kind.
    pub kind: String,
    /// Durable recovery state.
    pub state: OperationState,
    /// Last confirmed local commit boundary.
    pub step: OperationStep,
    /// Normalized non-secret request used to reject ID reuse.
    pub request: Value,
    /// Versioned application intent; common store treats it as opaque.
    pub payload: Value,
    /// Confirmed result or deletion supersession receipt.
    pub result: Value,
    /// Last contextual recovery error, empty on success.
    pub last_error: String,
}

impl PersistentEntity for Operation {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.operation_id.to_string()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("operation_id"))
            .field(Field::string("user_id").indexed())
            .field(Field::string("profile_id").nullable().indexed())
            .field(Field::integer("schema_version"))
            .field(Field::string("kind"))
            .field(Field::string("state").indexed())
            .field(Field::string("step"))
            .field(Field::json("request"))
            .field(Field::json("payload"))
            .field(Field::json("result"))
            .field(Field::string("last_error"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("operation_id", self.operation_id.to_string())?;
        writer.string("user_id", self.user_id.to_string())?;
        if let Some(value) = self.profile_id {
            writer.string("profile_id", value.to_string())?;
        }
        writer.integer("schema_version", self.schema_version)?;
        writer.string("kind", self.kind.clone())?;
        writer.string("state", super::super::enum_string(&self.state)?)?;
        writer.string("step", super::super::enum_string(&self.step)?)?;
        writer.json("request", serde_json::to_value(&self.request)?)?;
        writer.json("payload", serde_json::to_value(&self.payload)?)?;
        writer.json("result", serde_json::to_value(&self.result)?)?;
        writer.string("last_error", self.last_error.clone())?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            operation_id: Uuid::parse_str(reader.string("operation_id")?)?,
            user_id: Uuid::parse_str(reader.string("user_id")?)?,
            profile_id: reader
                .optional_string("profile_id")?
                .map(Uuid::parse_str)
                .transpose()?,
            schema_version: reader.integer("schema_version")?,
            kind: reader.string("kind")?.to_string(),
            state: super::super::read_enum(reader.string("state")?)?,
            step: super::super::read_enum(reader.string("step")?)?,
            request: serde_json::from_value(reader.json("request")?.clone())?,
            payload: serde_json::from_value(reader.json("payload")?.clone())?,
            result: serde_json::from_value(reader.json("result")?.clone())?,
            last_error: reader.string("last_error")?.to_string(),
        })
    }
}

//! Collection revisions, deletion tombstones and atomic streaming mutation receipts.

use crate::profiles::keys::PairKey;
use anyhow::Result;
use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, PersistentEntity,
};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Profile-local collection version, retaining deleted record versions to prevent stale recreation.
#[derive(Clone)]
pub(crate) struct Collection {
    /// Authoritative common UUID.
    pub profile_id: Uuid,
    /// One of homes, sections or bookmarks.
    pub kind: String,
    /// Monotonic collection revision; an absent collection starts at one.
    pub revision: i64,
    /// Last deleted version keyed by exact serialized resource identity.
    pub deleted_revisions: BTreeMap<String, i64>,
}

impl PersistentEntity for Collection {
    type Key = PairKey;
    fn key(&self) -> PairKey {
        PairKey([self.profile_id.to_string(), self.kind.clone()])
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("profile_id"))
            .primary_key(Field::string("kind"))
            .field(Field::integer("revision"))
            .field(Field::json("deleted_revisions"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("profile_id", self.profile_id.to_string())?;
        writer.string("kind", self.kind.clone())?;
        writer.integer("revision", self.revision)?;
        writer.json(
            "deleted_revisions",
            serde_json::to_value(&self.deleted_revisions)?,
        )
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            profile_id: Uuid::parse_str(reader.string("profile_id")?)?,
            kind: reader.string("kind")?.into(),
            revision: reader.integer("revision")?,
            deleted_revisions: serde_json::from_value(reader.json("deleted_revisions")?.clone())?,
        })
    }
}

/// Completed mutation and result committed with its streaming data, never across stores.
#[derive(Clone)]
pub(crate) struct Mutation {
    /// Stable client retry UUID.
    pub operation_id: Uuid,
    /// Authoritative common profile UUID; ownership is rechecked before replay.
    pub profile_id: Uuid,
    /// Registered operation kind.
    pub kind: String,
    /// Complete normalized non-secret request including expected revisions.
    pub request: Value,
    /// Original confirmed result.
    pub result: Value,
}

impl PersistentEntity for Mutation {
    type Key = String;
    fn key(&self) -> String {
        self.operation_id.to_string()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("operation_id"))
            .field(Field::string("profile_id").indexed())
            .field(Field::string("kind"))
            .field(Field::json("request"))
            .field(Field::json("result"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("operation_id", self.operation_id.to_string())?;
        writer.string("profile_id", self.profile_id.to_string())?;
        writer.string("kind", self.kind.clone())?;
        writer.json("request", self.request.clone())?;
        writer.json("result", self.result.clone())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            operation_id: Uuid::parse_str(reader.string("operation_id")?)?,
            profile_id: Uuid::parse_str(reader.string("profile_id")?)?,
            kind: reader.string("kind")?.into(),
            request: reader.json("request")?.clone(),
            result: reader.json("result")?.clone(),
        })
    }
}

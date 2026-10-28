use super::*;
use std::time::{Duration, SystemTime};

pub(super) fn test_schema() -> EntitySchema {
    EntitySchema::new()
        .primary_key(Field::string("authority"))
        .field(Field::string("name").indexed())
        .field(Field::integer("port"))
        .field(Field::date_time("expires_at").expiration())
}

pub(super) fn config() -> PersistenceStoreConfig {
    PersistenceStoreConfig::new("typed-store-test", test_schema()).expect("valid config")
}

pub(super) fn mismatched_config() -> PersistenceStoreConfig {
    // Same store contract but a schema that does not match the entities.
    PersistenceStoreConfig::new(
        "typed-store-test",
        EntitySchema::new()
            .primary_key(Field::string("authority"))
            .field(Field::string("name"))
            .field(Field::string("port"))
            .field(Field::date_time("expires_at").expiration()),
    )
    .expect("valid config")
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TestEntity {
    pub(super) authority: String,
    pub(super) name: String,
    pub(super) port: i64,
    pub(super) expires_at: SystemTime,
}
impl PersistentEntity for TestEntity {
    type Key = String;
    fn key(&self) -> String {
        self.authority.clone()
    }
    fn schema() -> EntitySchema {
        test_schema()
    }
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
        writer.string("authority", self.authority.clone())?;
        writer.string("name", self.name.clone())?;
        writer.integer("port", self.port)?;
        writer.date_time("expires_at", self.expires_at)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            authority: reader.string("authority")?.to_string(),
            name: reader.string("name")?.to_string(),
            port: reader.integer("port")?,
            expires_at: reader.date_time("expires_at")?,
        })
    }
}

/// Same schema as [`TestEntity`] but the port stays optional: an entity
/// without a port fails validation because the field is required.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexibleEntity {
    pub(super) authority: String,
    pub(super) name: String,
    pub(super) port: Option<i64>,
    pub(super) expires_at: SystemTime,
}
impl PersistentEntity for FlexibleEntity {
    type Key = String;
    fn key(&self) -> String {
        self.authority.clone()
    }
    fn schema() -> EntitySchema {
        test_schema()
    }
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
        writer.string("authority", self.authority.clone())?;
        writer.string("name", self.name.clone())?;
        if let Some(port) = self.port {
            writer.integer("port", port)?;
        }
        writer.date_time("expires_at", self.expires_at)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            authority: reader.string("authority")?.to_string(),
            name: reader.string("name")?.to_string(),
            port: Some(reader.integer("port")?),
            expires_at: reader.date_time("expires_at")?,
        })
    }
}

pub(super) fn entity(authority: &str, name: &str, ttl_secs: u64) -> TestEntity {
    TestEntity {
        authority: authority.to_string(),
        name: name.to_string(),
        port: 8080,
        expires_at: SystemTime::now() + Duration::from_secs(ttl_secs),
    }
}

pub(super) fn flexible(authority: &str, name: &str, port: Option<i64>) -> FlexibleEntity {
    FlexibleEntity {
        authority: authority.to_string(),
        name: name.to_string(),
        port,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    }
}

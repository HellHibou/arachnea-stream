//! Persisted common account, session, profile and onboarding models.

use super::super::keys::PairKey;
use anyhow::Result;
use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, PersistentEntity,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Account lifecycle blocking authorization during deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    /// Account available for authentication.
    Active,
    /// Account inaccessible while cleanup is pending.
    Deleting,
}

/// Identity lifecycle, independent of onboarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileState {
    /// Identity reserved but not published.
    Creating,
    /// Identity available to its owner.
    Active,
    /// Identity blocked pending cleanup.
    Deleting,
}

/// Completion state for one service scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingState {
    /// Guide must be completed.
    Pending,
    /// Guide confirmed by the user.
    Completed,
}

/// Common settings; streaming data never enters this document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonPreferences {
    /// Supported UI locale, or browser language when absent.
    pub language: Option<String>,
    /// Common theme preset identity.
    pub theme: String,
}

/// Persisted public account; password hashes never belong in public DTOs.
#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    /// Stable account UUID.
    pub id: Uuid,
    /// Validated canonical login with global uniqueness.
    pub normalized_login: String,
    /// Argon2id hash, never a plaintext password.
    pub password_hash: String,
    /// Restricts the session until password replacement.
    pub must_change_password: bool,
    /// Account lifecycle state.
    pub state: AccountState,
    /// Creation time in epoch milliseconds.
    pub created_at: i64,
    /// Last mutation time in epoch milliseconds.
    pub updated_at: i64,
}

impl PersistentEntity for User {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.id.to_string()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("id"))
            .field(Field::string("normalized_login").indexed())
            .field(Field::string("password_hash"))
            .field(Field::boolean("must_change_password"))
            .field(Field::string("state").indexed())
            .field(Field::integer("created_at"))
            .field(Field::integer("updated_at"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("id", self.id.to_string())?;
        writer.string("normalized_login", self.normalized_login.clone())?;
        writer.string("password_hash", self.password_hash.clone())?;
        writer.boolean("must_change_password", self.must_change_password)?;
        writer.string("state", super::super::enum_string(&self.state)?)?;
        writer.integer("created_at", self.created_at)?;
        writer.integer("updated_at", self.updated_at)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            id: Uuid::parse_str(reader.string("id")?)?,
            normalized_login: reader.string("normalized_login")?.to_string(),
            password_hash: reader.string("password_hash")?.to_string(),
            must_change_password: reader.boolean("must_change_password")?,
            state: super::super::read_enum(reader.string("state")?)?,
            created_at: reader.integer("created_at")?,
            updated_at: reader.integer("updated_at")?,
        })
    }
}

/// Persistent public session metadata; no plaintext secret.
#[derive(Clone, Serialize, Deserialize)]
pub struct UserSession {
    /// Session record identity.
    pub id: Uuid,
    /// Owning account UUID.
    pub user_id: Uuid,
    /// Cryptographic digest of the session secret.
    pub session_digest: String,
    /// Idle expiry in epoch milliseconds, checked by session service.
    pub expires_at: i64,
    /// Last authenticated use in epoch milliseconds.
    pub last_activity_at: i64,
    /// Explicit revocation flag.
    pub revoked: bool,
}

impl PersistentEntity for UserSession {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.id.to_string()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("id"))
            .field(Field::string("user_id").indexed())
            .field(Field::string("session_digest").indexed())
            .field(Field::integer("expires_at"))
            .field(Field::integer("last_activity_at"))
            .field(Field::boolean("revoked"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("id", self.id.to_string())?;
        writer.string("user_id", self.user_id.to_string())?;
        writer.string("session_digest", self.session_digest.clone())?;
        writer.integer("expires_at", self.expires_at)?;
        writer.integer("last_activity_at", self.last_activity_at)?;
        writer.boolean("revoked", self.revoked)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            id: Uuid::parse_str(reader.string("id")?)?,
            user_id: Uuid::parse_str(reader.string("user_id")?)?,
            session_digest: reader.string("session_digest")?.to_string(),
            expires_at: reader.integer("expires_at")?,
            last_activity_at: reader.integer("last_activity_at")?,
            revoked: reader.boolean("revoked")?,
        })
    }
}

/// Common authoritative profile identity.
#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    /// UUID generated only by the common repository.
    pub id: Uuid,
    /// Owning account UUID.
    pub user_id: Uuid,
    /// Validated display name.
    pub name: String,
    /// Validated Unicode uniqueness key within the account.
    pub normalized_name: String,
    /// Embedded avatar catalogue identity.
    pub avatar_id: String,
    /// Non-negative navigation order.
    pub order: i64,
    /// Creation/authorization/deletion state.
    pub state: ProfileState,
    /// Identity revision, starting at one.
    pub revision: i64,
    /// Creation time in epoch milliseconds.
    pub created_at: i64,
    /// Last mutation time in epoch milliseconds.
    pub updated_at: i64,
}

impl PersistentEntity for Profile {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.id.to_string()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("id"))
            .field(Field::string("user_id").indexed())
            .field(Field::string("name"))
            .field(Field::string("normalized_name").indexed())
            .field(Field::string("avatar_id"))
            .field(Field::integer("order"))
            .field(Field::string("state").indexed())
            .field(Field::integer("revision"))
            .field(Field::integer("created_at"))
            .field(Field::integer("updated_at"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("id", self.id.to_string())?;
        writer.string("user_id", self.user_id.to_string())?;
        writer.string("name", self.name.clone())?;
        writer.string("normalized_name", self.normalized_name.clone())?;
        writer.string("avatar_id", self.avatar_id.clone())?;
        writer.integer("order", self.order)?;
        writer.string("state", super::super::enum_string(&self.state)?)?;
        writer.integer("revision", self.revision)?;
        writer.integer("created_at", self.created_at)?;
        writer.integer("updated_at", self.updated_at)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            id: Uuid::parse_str(reader.string("id")?)?,
            user_id: Uuid::parse_str(reader.string("user_id")?)?,
            name: reader.string("name")?.to_string(),
            normalized_name: reader.string("normalized_name")?.to_string(),
            avatar_id: reader.string("avatar_id")?.to_string(),
            order: reader.integer("order")?,
            state: super::super::read_enum(reader.string("state")?)?,
            revision: reader.integer("revision")?,
            created_at: reader.integer("created_at")?,
            updated_at: reader.integer("updated_at")?,
        })
    }
}

/// Versioned common preferences for one authoritative profile.
#[derive(Clone, Serialize, Deserialize)]
pub struct CommonSettings {
    /// Common profile UUID.
    pub profile_id: Uuid,
    /// Version of the typed document.
    pub schema_version: i64,
    /// Monotonic document revision.
    pub revision: i64,
    /// Common UI settings only.
    pub values: CommonPreferences,
}

impl PersistentEntity for CommonSettings {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.profile_id.to_string()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("profile_id"))
            .field(Field::integer("schema_version"))
            .field(Field::integer("revision"))
            .field(Field::json("values"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("profile_id", self.profile_id.to_string())?;
        writer.integer("schema_version", self.schema_version)?;
        writer.integer("revision", self.revision)?;
        writer.json("values", serde_json::to_value(&self.values)?)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            profile_id: Uuid::parse_str(reader.string("profile_id")?)?,
            schema_version: reader.integer("schema_version")?,
            revision: reader.integer("revision")?,
            values: serde_json::from_value(reader.json("values")?.clone())?,
        })
    }
}

/// Common or service-specific guide state, without service data.
#[derive(Clone, Serialize, Deserialize)]
pub struct Onboarding {
    /// Common profile UUID.
    pub profile_id: Uuid,
    /// Common or service namespace.
    pub scope: String,
    /// Guide state.
    pub state: OnboardingState,
    /// Monotonic guide revision.
    pub revision: i64,
}

impl PersistentEntity for Onboarding {
    type Key = PairKey;
    fn key(&self) -> Self::Key {
        PairKey([self.profile_id.to_string(), self.scope.clone()])
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("profile_id"))
            .primary_key(Field::string("scope"))
            .field(Field::string("state").indexed())
            .field(Field::integer("revision"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("profile_id", self.profile_id.to_string())?;
        writer.string("scope", self.scope.clone())?;
        writer.string("state", super::super::enum_string(&self.state)?)?;
        writer.integer("revision", self.revision)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            profile_id: Uuid::parse_str(reader.string("profile_id")?)?,
            scope: reader.string("scope")?.to_string(),
            state: super::super::read_enum(reader.string("state")?)?,
            revision: reader.integer("revision")?,
        })
    }
}

/// New identity request; caller validates names/avatar before submission.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProfile {
    /// Validated display name.
    pub name: String,
    /// Unicode uniqueness key computed by the profile service.
    pub normalized_name: String,
    /// Validated embedded avatar identity.
    pub avatar_id: String,
    /// Validated common defaults/preferences.
    pub common: CommonPreferences,
}

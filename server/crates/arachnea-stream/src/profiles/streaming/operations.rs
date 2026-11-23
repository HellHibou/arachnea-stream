//! Profile-authorized local streaming transactions and revision-checked CRUD and reorder operations.

use super::{models::*, state::Collection, validation, StreamingRepository};
use crate::profiles::{
    accounts::reject,
    by_id,
    keys::{PairKey, TripleKey},
};
use anyhow::{Context, Result};
use arachnea_core::persistence::SqliteTransaction;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

/// Strict common request shape; command-specific field presence is checked separately.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    profile_id: Uuid,
    expected_revision: Option<i64>,
    value: Option<Value>,
    identity: Option<Value>,
    ids: Option<Vec<String>>,
}

/// Returns an incremented JSON-safe revision without wrapping.
fn next(revision: i64) -> Result<i64> {
    anyhow::ensure!(
        (1..crate::PROFILE_MAX_REVISION).contains(&revision),
        "streaming revision exhausted"
    );
    Ok(revision + 1)
}
fn conflict() -> anyhow::Error {
    reject(
        "revision_conflict",
        409,
        "Streaming data changed; reload before applying this change.",
    )
}
fn invalid() -> anyhow::Error {
    reject("invalid_data", 422, "Invalid streaming resource request.")
}
fn expected(current: Option<i64>, expected: i64) -> Result<()> {
    if expected < 0 || current.unwrap_or(0) != expected {
        return Err(conflict());
    }
    Ok(())
}
fn exact(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|v| v.len() == keys.len() && keys.iter().all(|key| v.contains_key(*key)))
}

impl StreamingRepository {
    /// Executes a strict streaming operation after the coordinator has locked and authorized its profile.
    ///
    /// # Arguments
    /// * `tx` - Shared transaction holding the authorization checks.
    /// * `command` - Registered collection/read/write operation.
    /// * `payload` - Transport DTO; ownership never comes from the payload.
    ///
    /// # Errors
    /// Rejects invalid data and stale revisions; the caller rolls back on error.
    pub(crate) fn request(
        &self,
        tx: &SqliteTransaction<'_>,
        command: &str,
        mut payload: Value,
    ) -> Result<Value> {
        let action = command.strip_prefix("profiles/").ok_or_else(invalid)?;
        let kind = if action.ends_with("home") || action == "homes" || action == "reorder_homes" {
            "homes"
        } else if action.ends_with("section")
            || action == "sections"
            || action == "reorder_sections"
        {
            "sections"
        } else if action.ends_with("bookmark") || action == "bookmarks" {
            "bookmarks"
        } else {
            return Err(invalid());
        };
        let write = action != kind;
        let allowed = if !write {
            vec!["profile_id"]
        } else if action.starts_with("reorder_") {
            vec!["profile_id", "expected_revision", "ids"]
        } else if action.starts_with("save_") {
            vec!["profile_id", "expected_revision", "value"]
        } else {
            vec!["profile_id", "expected_revision", "identity"]
        };
        if !exact(&payload, &allowed) {
            return Err(invalid());
        }
        if kind == "bookmarks" {
            let object = payload.get_mut(if action.starts_with("save_") {
                "value"
            } else {
                "identity"
            });
            if let Some(object) = object {
                for key in ["source_id", "entry"] {
                    if let Some(s) = object.get_mut(key) {
                        *s = json!(s.as_str().ok_or_else(invalid)?.trim());
                    }
                }
            }
        }
        let request: Request = serde_json::from_value(payload.clone()).map_err(|_| invalid())?;
        let id = request.profile_id;
        if tx.get(&self.settings, &id.to_string())?.is_none() {
            return Err(reject(
                "invalid_data",
                422,
                "Streaming profile is not initialized.",
            ));
        }
        let key = PairKey([id.to_string(), kind.into()]);
        let mut collection = tx.get(&self.collections, &key)?.unwrap_or(Collection {
            profile_id: id,
            kind: kind.into(),
            revision: 1,
            deleted_revisions: Default::default(),
        });
        if !write {
            return self.list(tx, &collection);
        }
        let revision = request.expected_revision.ok_or_else(invalid)?;
        let result = if action.starts_with("reorder_") {
            if collection.revision != revision {
                return Err(conflict());
            }
            self.reorder(tx, &collection, request.ids.ok_or_else(invalid)?)?
        } else {
            self.mutate(
                tx,
                &mut collection,
                action,
                revision,
                request.value.or(request.identity).ok_or_else(invalid)?,
            )?
        };
        collection.revision = next(collection.revision)?;
        tx.put(&self.collections, &collection)?;
        if action.starts_with("reorder_") {
            self.list(tx, &collection)
        } else {
            Ok(result)
        }
    }

    /// Projects a complete collection in deterministic display order without internal metadata.
    fn list(&self, tx: &SqliteTransaction<'_>, collection: &Collection) -> Result<Value> {
        let items = match collection.kind.as_str() {
            "homes" => {
                let mut rows = tx.find(&self.homes, &by_id("profile_id", collection.profile_id))?;
                rows.sort_by_key(|r| (r.navigation_order, r.home_id));
                rows.iter().map(home_view).collect::<Vec<_>>()
            }
            "sections" => {
                let mut rows =
                    tx.find(&self.sections, &by_id("profile_id", collection.profile_id))?;
                rows.sort_by_key(|r| (r.order, r.section_key.clone()));
                rows.iter().map(section_view).collect()
            }
            "bookmarks" => {
                let mut rows =
                    tx.find(&self.bookmarks, &by_id("profile_id", collection.profile_id))?;
                rows.sort_by_key(|r| {
                    (
                        std::cmp::Reverse(r.modified_at),
                        r.source_id.clone(),
                        r.entry.clone(),
                    )
                });
                rows.iter().map(bookmark_view).collect()
            }
            _ => return Err(invalid()),
        };
        Ok(json!({"items":items,"revision":collection.revision}))
    }

    /// Validates and applies one record mutation in the caller’s transaction.
    fn mutate(
        &self,
        tx: &SqliteTransaction<'_>,
        collection: &mut Collection,
        action: &str,
        revision: i64,
        mut value: Value,
    ) -> Result<Value> {
        let id = collection.profile_id;
        let save = action.starts_with("save_");
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
        )?;
        match collection.kind.as_str() {
            "homes" => {
                let home_id = if save {
                    let map = value.as_object().ok_or_else(invalid)?;
                    if !map.contains_key("definition")
                        || map.keys().any(|k| k != "home_id" && k != "definition")
                    {
                        return Err(invalid());
                    }
                    validation::home(&value["definition"])?;
                    value
                        .get("home_id")
                        .map(|v| serde_json::from_value::<Uuid>(v.clone()))
                        .transpose()
                        .map_err(|_| invalid())?
                } else {
                    if !exact(&value, &["home_id"]) {
                        return Err(invalid());
                    }
                    Some(serde_json::from_value(value["home_id"].clone()).map_err(|_| invalid())?)
                };
                let home_id = home_id.unwrap_or_else(Uuid::new_v4);
                let key = PairKey([id.to_string(), home_id.to_string()]);
                let current = tx.get(&self.homes, &key)?;
                expected(current.as_ref().map(|r| r.revision), revision)?;
                if !save && current.is_none() {
                    return Err(conflict());
                }
                if save && current.is_none() && value.get("home_id").is_some() {
                    return Err(reject(
                        "resource_not_found",
                        404,
                        "Home identity is inaccessible; omit home_id for creation.",
                    ));
                }
                if let Some(existing) = current.as_ref() {
                    if value
                        .get("definition")
                        .is_some_and(|d| d.get("id") != Some(&json!(existing.home_id)))
                    {
                        return Err(invalid());
                    }
                }
                let tombstone = home_id.to_string();
                if save {
                    let mut definition = value["definition"].take();
                    let slug = validation::slug(definition["name"].as_str().unwrap());
                    if tx
                        .find(&self.homes, &by_id("profile_id", id))?
                        .iter()
                        .any(|h| {
                            h.home_id != home_id
                                && validation::slug(h.definition["name"].as_str().unwrap_or(""))
                                    == slug
                        })
                    {
                        return Err(reject(
                            "invalid_data",
                            422,
                            "A home with the same name or URL already exists.",
                        ));
                    }
                    definition["id"] = json!(home_id);
                    definition["createdAt"] = json!(current
                        .as_ref()
                        .map_or(now, |r| r.definition["createdAt"].as_i64().unwrap_or(now)));
                    definition["updatedAt"] = json!(current.as_ref().map_or(now, |r| now
                        .max(r.definition["updatedAt"].as_i64().unwrap_or(now) + 1)));
                    let row = Home {
                        profile_id: id,
                        home_id,
                        schema_version: crate::PROFILE_DOCUMENT_VERSION,
                        revision: record_revision(
                            collection,
                            &tombstone,
                            current.as_ref().map(|r| r.revision),
                        )?,
                        navigation_order: definition["navigationOrder"].as_i64().unwrap(),
                        definition,
                    };
                    tx.put(&self.homes, &row)?;
                    Ok(home_view(&row))
                } else {
                    let current = current.unwrap();
                    collection
                        .deleted_revisions
                        .insert(tombstone, next(current.revision)?);
                    tx.delete_where(
                        &self.homes,
                        &by_id("profile_id", id).where_string(
                            &arachnea_core::persistence::Field::string("home_id"),
                            home_id.to_string(),
                        ),
                    )?;
                    let mut settings = tx
                        .get(&self.settings, &id.to_string())?
                        .context("streaming settings missing")?;
                    if settings.values["defaultHomeId"] == home_id.to_string() {
                        settings.values["defaultHomeId"] = Value::Null;
                        settings.revision = next(settings.revision)?;
                        tx.put(&self.settings, &settings)?;
                    }
                    Ok(json!({"deleted":true}))
                }
            }
            "sections" => {
                if save {
                    validation::section(&value)?;
                } else if !exact(&value, &["section_key"]) {
                    return Err(invalid());
                }
                let section_key = value["section_key"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(invalid)?
                    .to_owned();
                let key = PairKey([id.to_string(), section_key.clone()]);
                let current = tx.get(&self.sections, &key)?;
                expected(current.as_ref().map(|r| r.revision), revision)?;
                if save {
                    let row = HomeSection {
                        profile_id: id,
                        section_key: section_key.clone(),
                        schema_version: crate::PROFILE_DOCUMENT_VERSION,
                        revision: record_revision(
                            collection,
                            &section_key,
                            current.as_ref().map(|r| r.revision),
                        )?,
                        order: value["order"].as_i64().unwrap(),
                        values: json!({"collectionMode":value["collectionMode"],"thumbnailOrientation":value["thumbnailOrientation"],"thumbnailImageFit":value["thumbnailImageFit"]}),
                    };
                    tx.put(&self.sections, &row)?;
                    Ok(section_view(&row))
                } else {
                    let current = current.ok_or_else(conflict)?;
                    collection
                        .deleted_revisions
                        .insert(section_key.clone(), next(current.revision)?);
                    tx.delete_where(
                        &self.sections,
                        &by_id("profile_id", id).where_string(
                            &arachnea_core::persistence::Field::string("section_key"),
                            section_key,
                        ),
                    )?;
                    Ok(json!({"deleted":true}))
                }
            }
            "bookmarks" => {
                if !exact(
                    &value,
                    if save {
                        &["source_id", "entry", "bookmark"]
                    } else {
                        &["source_id", "entry"]
                    },
                ) {
                    return Err(invalid());
                }
                let source = value["source_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?
                    .to_owned();
                let entry = value["entry"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?
                    .to_owned();
                let key = TripleKey([id.to_string(), source.clone(), entry.clone()]);
                let current = tx.get(&self.bookmarks, &key)?;
                expected(current.as_ref().map(|r| r.revision), revision)?;
                let identity = serde_json::to_string(&[&source, &entry])?;
                if save {
                    validation::bookmark(&value["bookmark"])?;
                    let row = Bookmark {
                        profile_id: id,
                        source_id: source,
                        entry,
                        schema_version: crate::PROFILE_DOCUMENT_VERSION,
                        revision: record_revision(
                            collection,
                            &identity,
                            current.as_ref().map(|r| r.revision),
                        )?,
                        modified_at: now,
                        values: value["bookmark"].take(),
                    };
                    tx.put(&self.bookmarks, &row)?;
                    Ok(bookmark_view(&row))
                } else {
                    let current = current.ok_or_else(conflict)?;
                    collection
                        .deleted_revisions
                        .insert(identity, next(current.revision)?);
                    tx.delete_where(
                        &self.bookmarks,
                        &by_id("profile_id", id)
                            .where_string(
                                &arachnea_core::persistence::Field::string("source_id"),
                                source,
                            )
                            .where_string(
                                &arachnea_core::persistence::Field::string("entry"),
                                entry,
                            ),
                    )?;
                    Ok(json!({"deleted":true}))
                }
            }
            _ => Err(invalid()),
        }
    }

    /// Validates a complete ID permutation and changes only order, timestamp and revision fields.
    fn reorder(
        &self,
        tx: &SqliteTransaction<'_>,
        collection: &Collection,
        ids: Vec<String>,
    ) -> Result<Value> {
        let current = self.list(tx, collection)?;
        let key = if collection.kind == "homes" {
            "home_id"
        } else {
            "section_key"
        };
        let existing = current["items"].as_array().unwrap();
        if ids.len() != existing.len()
            || ids
                .iter()
                .enumerate()
                .any(|(i, id)| ids[..i].contains(id) || !existing.iter().any(|v| v[key] == *id))
        {
            return Err(invalid());
        }
        for (order, id) in ids.into_iter().enumerate() {
            if collection.kind == "homes" {
                let mut row = tx
                    .get(
                        &self.homes,
                        &PairKey([collection.profile_id.to_string(), id]),
                    )?
                    .context("reordered home missing")?;
                row.navigation_order = order as i64;
                row.definition["navigationOrder"] = json!(order);
                row.revision = next(row.revision)?;
                row.definition["updatedAt"] = json!(i64::try_from(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis()
                )?
                .max(row.definition["updatedAt"].as_i64().unwrap_or(0) + 1));
                tx.put(&self.homes, &row)?;
            } else {
                let mut row = tx
                    .get(
                        &self.sections,
                        &PairKey([collection.profile_id.to_string(), id]),
                    )?
                    .context("reordered section missing")?;
                row.order = order as i64;
                row.revision = next(row.revision)?;
                tx.put(&self.sections, &row)?;
            }
        }
        Ok(Value::Null)
    }
}

/// Continues versions after deletion so an old snapshot cannot target a recreated record.
fn record_revision(collection: &Collection, key: &str, current: Option<i64>) -> Result<i64> {
    current
        .or_else(|| collection.deleted_revisions.get(key).copied())
        .map_or(Ok(1), next)
}
fn home_view(row: &Home) -> Value {
    json!({"profile_id":row.profile_id,"home_id":row.home_id,"revision":row.revision,"definition":row.definition})
}
fn section_view(row: &HomeSection) -> Value {
    json!({"profile_id":row.profile_id,"section_key":row.section_key,"revision":row.revision,"order":row.order,"collectionMode":row.values["collectionMode"],"thumbnailOrientation":row.values["thumbnailOrientation"],"thumbnailImageFit":row.values["thumbnailImageFit"]})
}
fn bookmark_view(row: &Bookmark) -> Value {
    json!({"profile_id":row.profile_id,"source_id":row.source_id,"entry":row.entry,"revision":row.revision,"modified_at":row.modified_at,"bookmark":row.values})
}

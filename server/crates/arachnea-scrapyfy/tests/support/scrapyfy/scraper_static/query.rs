use super::*;

impl StaticScraperQuery {
    /// Returns the flattened list of leaf field names produced by this query.
    pub fn get_field_names(&self) -> Vec<String> {
        let mut names = Vec::new();

        for entry in &self.entries {
            if let Some(value) = &entry.value {
                match entry.output_type {
                    Some(ScraperOutputType::Object | ScraperOutputType::ObjectArray) => {
                        collect_static_object_field_names(&entry.name, value, &mut names);
                    }
                    _ => names.push(entry.name.clone()),
                }
            }

            for item in &entry.items {
                for item_name in item.keys() {
                    names.push(format!("{} > {}", entry.name, item_name));
                }
            }
        }

        names.sort();
        names.dedup();
        names
    }
}

#[cfg(any(test, feature = "test-support"))]
fn collect_static_object_field_names(prefix: &str, value: &Value, names: &mut Vec<String>) {
    match value {
        Value::Mapping(mapping) => {
            for (key, child) in mapping {
                let Some(key) = key.as_str() else {
                    continue;
                };
                let child_prefix = format!("{} > {}", prefix, key);
                match child {
                    Value::Mapping(_) | Value::Sequence(_) => {
                        collect_static_object_field_names(&child_prefix, child, names);
                    }
                    _ => names.push(child_prefix),
                }
            }
        }
        Value::Sequence(items) => {
            for item in items {
                collect_static_object_field_names(prefix, item, names);
            }
        }
        _ => {}
    }
}

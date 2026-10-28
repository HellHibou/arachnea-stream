//! YAML names and serde representation of scraper output types.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// JSON output type requested by scraper YAML for one output node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScraperOutputType {
    /// JSON string scalar.
    String,
    /// JSON number scalar.
    Number,
    /// JSON boolean scalar.
    Boolean,
    /// JSON object produced from a scraper node.
    Object,
    /// JSON string array.
    StringArray,
    /// JSON number array.
    NumberArray,
    /// JSON boolean array.
    BooleanArray,
    /// JSON object array produced from scraper node items.
    ObjectArray,
}

impl ScraperOutputType {
    /// Returns the YAML spelling of this output type.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Object => "object",
            Self::StringArray => "string[]",
            Self::NumberArray => "number[]",
            Self::BooleanArray => "boolean[]",
            Self::ObjectArray => "object[]",
        }
    }

    /// Returns `true` when this type describes one JSON scalar value.
    pub fn is_scalar(self) -> bool {
        matches!(self, Self::String | Self::Number | Self::Boolean)
    }

    /// Returns the scalar element type for one primitive array type.
    pub fn array_element_type(self) -> Option<Self> {
        match self {
            Self::StringArray => Some(Self::String),
            Self::NumberArray => Some(Self::Number),
            Self::BooleanArray => Some(Self::Boolean),
            _ => None,
        }
    }
}

impl Serialize for ScraperOutputType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ScraperOutputType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.trim() {
            "string" => Ok(Self::String),
            "number" => Ok(Self::Number),
            "boolean" => Ok(Self::Boolean),
            "object" => Ok(Self::Object),
            "string[]" => Ok(Self::StringArray),
            "number[]" => Ok(Self::NumberArray),
            "boolean[]" => Ok(Self::BooleanArray),
            "object[]" => Ok(Self::ObjectArray),
            other => Err(serde::de::Error::custom(format!(
                "unknown scraper output type `{}`",
                other
            ))),
        }
    }
}

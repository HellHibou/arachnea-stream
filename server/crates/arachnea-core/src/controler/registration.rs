//! Typed command registration, JSON conversion and conditional response validation.

use super::{
    ControlerFunctionInput, ControlerJsonInput, ControlerJsonOutput, ControlerService,
    ControlerStreamInput, ControlerStreamOutput, JsonControlerFuture, RequestControlerContext,
    StreamControlerFuture, HEADER_ETAG,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{collections::HashMap, future::Future, sync::Arc};

/// Deserializes controller input into a specific type.
///
/// # Type Parameters
/// * `I` - The type to deserialize into, must implement `DeserializeOwned`.
///
/// # Arguments
/// * `payload` - The controller function input to deserialize.
///
/// # Returns
/// `Ok(I)` if deserialization succeeds.
/// `Err(String)` if deserialization fails.
pub fn deserialize_input<I>(payload: ControlerFunctionInput) -> Result<I, String>
where
    I: DeserializeOwned,
{
    match payload {
        ControlerFunctionInput::Json(value) => {
            serde_json::from_value(value).map_err(|e| format!("Deserialization error: {}", e))
        }
        ControlerFunctionInput::Query(query) => {
            let value = query_string_to_json_value(&query)?;
            serde_json::from_value(value).map_err(|e| format!("Deserialization error: {}", e))
        }
    }
}

/// Parses a raw URL query string into a JSON object value.
///
/// Each parameter value is parsed as JSON when it is a valid JSON value;
/// otherwise it remains a string. This lets GET clients send arrays, objects,
/// quoted strings, numbers, booleans, and null values through the query
/// string while preserving unquoted text values.
///
/// Repeated keys are collected into a JSON array of their raw string values.
///
/// # Arguments
///
/// * `query` - Raw URL query string (without the leading `?`).
///
/// # Returns
///
/// A JSON object mapping parameter names to their decoded values.
///
/// # Errors
///
/// Returns an error when the query string cannot be percent-decoded.
fn query_string_to_json_value(query: &str) -> Result<Value, String> {
    use serde_json::Map;

    let pairs = serde_urlencoded::from_str::<Vec<(String, String)>>(query)
        .map_err(|e| format!("Deserialization error: {}", e))?;
    let mut object = Map::new();

    for (key, raw_value) in pairs {
        let value = serde_json::from_str::<Value>(&raw_value).unwrap_or(Value::String(raw_value));

        match object.get_mut(&key) {
            Some(existing) => {
                if let Some(array) = existing.as_array_mut() {
                    array.push(value);
                } else {
                    let previous = existing.take();
                    *existing = Value::Array(vec![previous, value]);
                }
            }
            None => {
                object.insert(key, value);
            }
        }
    }

    Ok(Value::Object(object))
}

/// Serializes output into a JSON value.
///
/// # Type Parameters
/// * `O` - The type to serialize, must implement `Serialize`.
///
/// # Arguments
/// * `response` - The response to serialize.
///
/// # Returns
/// `Ok(Value)` if serialization succeeds.
/// `Err(String)` if serialization fails.
fn serialize_output<O>(response: O) -> Result<Value, String>
where
    O: Serialize,
{
    serde_json::to_value(response).map_err(|e| format!("Serialization error: {}", e))
}

/// Convenience extension trait for registering typed controller callbacks.
///
/// This trait provides ergonomic methods for registering strongly-typed
/// controller functions without manual serialization/deserialization boilerplate.
pub trait ControlerServiceExt: ControlerService {
    /// Registers a strongly typed callback that may fail under a command name.
    ///
    /// The callback input is deserialized from either JSON or a raw query string,
    /// depending on the backend and HTTP method used. The callback also receives
    /// the [`RequestControlerContext`] built by the controller backend. Errors are
    /// forwarded to the controller backend as user-visible command errors instead
    /// of panicking.
    ///
    /// When the callback produces a global ETag and the client's
    /// `If-None-Match` header matches it, the backend answers `304 Not Modified`
    /// with no body; otherwise the serialized payload is returned with an `ETag`
    /// response header when one was produced.
    ///
    /// # Type Parameters
    /// * `I` - Input type that implements `DeserializeOwned`.
    /// * `O` - Output type that implements `Serialize`.
    /// * `F` - Function type taking input and the request context.
    /// * `Fut` - Future type that resolves to `Result<(O, Option<String>), E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `fct` - The function to register.
    fn register_result_function<I, O, F, Fut, E>(&mut self, name: &str, fct: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        F: Fn(I, RequestControlerContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(O, Option<String>), E>> + Send + 'static,
        E: std::fmt::Display,
    {
        let fct = Arc::new(fct);
        let handler = move |input: ControlerJsonInput| -> JsonControlerFuture {
            let fct = Arc::clone(&fct);
            Box::pin(async move {
                let if_none_match = normalize_etag(input.if_none_match());
                let payload = deserialize_input(input.payload)?;
                let (response, etag) = fct(payload, input.context)
                    .await
                    .map_err(|e| e.to_string())?;

                if let Some(etag) = etag.as_ref() {
                    if if_none_match.as_deref() == Some(etag.as_str()) {
                        return Ok(ControlerJsonOutput {
                            value: Value::Null,
                            status: 304,
                            headers: HashMap::from([(
                                HEADER_ETAG.to_string(),
                                format!("\"{etag}\""),
                            )]),
                        });
                    }
                }

                let mut output = ControlerJsonOutput::from(serialize_output(response)?);
                if let Some(etag) = etag {
                    output
                        .headers
                        .insert(HEADER_ETAG.to_string(), format!("\"{etag}\""));
                }
                Ok(output)
            })
        };

        self.register_json_function(name, Arc::new(handler));
    }

    /// Registers an async callback bound to shared state without per-call boilerplate.
    ///
    /// See [`ControlerServiceExt::register_result_function`] for the
    /// conditional-validation contract.
    ///
    /// # Type Parameters
    /// * `S` - State type that implements `Send + Sync`.
    /// * `I` - Input type that implements `DeserializeOwned`.
    /// * `O` - Output type that implements `Serialize`.
    /// * `F` - Function type that takes state, the request context, and input.
    /// * `Fut` - Future type that resolves to `Result<(O, Option<String>), E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `state` - The shared state to bind to the function.
    /// * `fct` - The function to register.
    fn register_result_function_with_state<S, I, O, F, Fut, E>(
        &mut self,
        name: &str,
        state: Arc<S>,
        fct: F,
    ) where
        S: Send + Sync + 'static,
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        F: Fn(Arc<S>, RequestControlerContext, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(O, Option<String>), E>> + Send + 'static,
        E: std::fmt::Display,
    {
        self.register_result_function(name, move |input: I, context: RequestControlerContext| {
            let state = Arc::clone(&state);
            fct(state, context, input)
        });
    }

    /// Registers an async binary stream callback bound to shared state without per-call boilerplate.
    ///
    /// # Type Parameters
    /// * `S` - State type that implements `Send + Sync`.
    /// * `F` - Function type that takes state and stream input.
    /// * `Fut` - Future type that resolves to `Result<ControlerStreamOutput, E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `state` - The shared state to bind to the function.
    /// * `fct` - The function to register.
    fn register_stream_function_with_state<S, F, Fut, E>(
        &mut self,
        name: &str,
        state: Arc<S>,
        fct: F,
    ) where
        S: Send + Sync + 'static,
        F: Fn(Arc<S>, ControlerStreamInput) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ControlerStreamOutput, E>> + Send + 'static,
        E: std::fmt::Display,
    {
        let fct = Arc::new(fct);
        let handler = move |input: ControlerStreamInput| -> StreamControlerFuture {
            let state = Arc::clone(&state);
            let fct = Arc::clone(&fct);
            Box::pin(async move { fct(state, input).await.map_err(|e| e.to_string()) })
        };

        self.register_stream_function(name, Arc::new(handler));
    }
}

/// Normalizes an `If-None-Match` header value for comparison.
///
/// Strips surrounding quotes and the weak-validator `W/` prefix so both the
/// incoming header and the produced ETag compare as bare opaque strings.
///
/// # Arguments
/// * `value` - Optional raw ETag or `If-None-Match` candidate.
pub(crate) fn normalize_etag(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    let value = value.trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

impl<T: ControlerService + ?Sized> ControlerServiceExt for T {}

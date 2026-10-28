//! Background execution and buffering of Tauri binary protocol responses.

use crate::controler::{
    ControlerStreamInput, ControlerStreamOutput, ResponseBody, StreamControlerFunction,
};

/// Runs a binary callback off the UI thread and responds with its buffered result.
///
/// # Arguments
/// * `request` - Original custom-protocol request, including headers and body.
/// * `responder` - Tauri responder consumed after collecting the callback output.
/// * `handler` - Selected binary command callback.
/// * `entry_point` - Public command URL used by generated links.
/// * `path` - Remaining route path after the command.
/// * `query` - Raw route query preserved for signed upstream URLs.
pub(super) fn respond(
    request: ::tauri::http::Request<Vec<u8>>,
    responder: ::tauri::UriSchemeResponder,
    handler: StreamControlerFunction,
    entry_point: String,
    path: String,
    query: String,
) {
    let method = request.method().to_string();
    let headers_map = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect::<std::collections::HashMap<String, String>>();

    tauri::async_runtime::spawn(async move {
        let response = async {
            let mut output = handler(ControlerStreamInput {
                path,
                query,
                method,
                headers: headers_map,
                body: request.body().to_vec(),
                entry_point,
                force_buffer_response: true,
            })
            .await?;

            // The protocol responder requires a complete body. Collect it
            // in this background task so network waits leave the UI free.
            if let ResponseBody::Streamed(stream) = &mut output.body {
                use futures::StreamExt;
                let mut body = Vec::new();
                let mut stream = std::pin::pin!(stream);
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => body.extend_from_slice(&bytes),
                        Err(e) => {
                            tracing::warn!(target: "arachnea_core::controler::tauri",
                                error = %e,
                                "stream error while collecting body for Tauri response"
                            );
                            return Err(format!("Stream error: {}", e));
                        }
                    }
                }
                output.body = ResponseBody::Buffered(body);
            }

            Ok(output)
        }
        .await;

        let response = match response {
            Ok(ControlerStreamOutput {
                status,
                body,
                content_type,
                headers,
            }) => {
                let body = match body {
                    ResponseBody::Buffered(mut bytes) => {
                        if request.method() == ::tauri::http::Method::HEAD {
                            bytes.clear();
                        }
                        bytes
                    }
                    ResponseBody::Streamed(_) => {
                        unreachable!("streaming responses are collected inside the handler call")
                    }
                };
                let mut builder = ::tauri::http::Response::builder()
                    .status(
                        ::tauri::http::StatusCode::from_u16(status)
                            .unwrap_or(::tauri::http::StatusCode::OK),
                    )
                    .header("Content-Type", &content_type);
                for (name, value) in &headers {
                    if name.to_ascii_lowercase() != "content-type" {
                        builder = builder.header(name.as_str(), value.as_str());
                    }
                }
                builder
                    .body(body)
                    .expect("Failed to build the Tauri stream response.")
            }
            Err(error) => ::tauri::http::Response::builder()
                .status(400)
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(error.into_bytes())
                .expect("Failed to build the Tauri stream error response."),
        };
        responder.respond(response);
    });
}

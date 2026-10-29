use super::STREAM_RESOLVER_GROUP_NAME;
use arachnea_scrapyfy::{ScraperAgregator, SharedProxyConfigHandle};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::test_support::configure_test_application_data_dir;

pub(super) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    configure_test_application_data_dir();
    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime for tests")
        .block_on(future)
}

pub(super) fn unique_temp_dir() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after UNIX_EPOCH")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "arachnea-stream-hoster-test-{}-{}-{}",
        std::process::id(),
        nanos,
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(super) fn setup_resolver_sources(sources: &[(&str, &str)]) -> (ScraperAgregator, PathBuf) {
    configure_test_application_data_dir();
    let dir = unique_temp_dir();
    std::fs::create_dir_all(&dir).expect("test service dir should be created");

    let mut service_entries = Vec::new();
    for (name, yaml) in sources {
        let file_name = format!("{name}.yaml");
        std::fs::write(dir.join(&file_name), yaml).expect("test YAML should be written");
        service_entries.push(format!(r#"{{ "path": "{}", "enabled": true }}"#, file_name));
    }

    let services_json = format!("[{}]", service_entries.join(","));
    let config_path = dir.join("services.json");
    std::fs::write(&config_path, services_json).expect("test services.json should be written");

    let mut agregator = ScraperAgregator::new_with_proxy_handle(SharedProxyConfigHandle::new());
    agregator
        .add_query_collection_from_config_json(STREAM_RESOLVER_GROUP_NAME, &config_path)
        .expect("test resolver sources should load");

    (agregator, dir)
}

pub(super) fn serve_html_once(body: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_thread = Arc::clone(&request_count);

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test request should arrive");
        request_count_for_thread.fetch_add(1, Ordering::SeqCst);
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("test response should be written");
    });

    (url, request_count)
}

/// Serves a redirecting shim (JS `window.location.href`) then the final player page,
/// routing on the requested path (`/player` identifies the final document).
pub(super) fn serve_js_redirect_chain(
    shim: &'static str,
    final_body: &'static str,
) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_thread = Arc::clone(&request_count);

    std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("test request should arrive");
            request_count_for_thread.fetch_add(1, Ordering::SeqCst);
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let request = String::from_utf8_lossy(&buffer).to_string();
            let body = if request.contains("/player") {
                final_body
            } else {
                shim
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("test response should be written");
        }
    });

    (url, request_count)
}

pub(super) fn decode_proxy_opts(proxied_url: &str) -> serde_json::Value {
    let encoded = proxied_url
        .split("/opts_")
        .nth(1)
        .and_then(|value| value.split('/').next())
        .expect("proxied URL should contain opts segment");
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .expect("opts segment should be base64url");
    serde_json::from_slice(&bytes).expect("opts segment should be JSON")
}

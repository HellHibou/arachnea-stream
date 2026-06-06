use std::error::Error;

use arachnea_dns::core::{
    ArachneaDnsCore, CoreConfig, LocalRecord, Record, RecordData, UsageProfile as DnsUsageProfile,
};
use arachnea_proxy::core::{ArachneaProxyCore, Destination, ProxyConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// cargo run -p arachnea-proxy --no-default-features --features arachnea-dns --example dns_resolver

/// Runs a proxy-core connection whose hostname is resolved by `arachnea-dns`.
///
/// # Errors
///
/// Returns an error when the local test listener cannot start, the DNS or proxy
/// core cannot be built, or the local HTTP exchange fails.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request).await?;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nhello proxy\n")
            .await?;
        std::io::Result::Ok(())
    });

    let dns_core = ArachneaDnsCore::new(dns_config_with_local_host("resolver.proxy.test"))?;
    let core = ArachneaProxyCore::new_with_arachnea_dns(ProxyConfig::default(), dns_core)?;
    let mut stream = core
        .connect(Destination::host_port("resolver.proxy.test", port))
        .await?;

    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: resolver.proxy.test\r\nConnection: close\r\n\r\n")
        .await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    server.await??;
    println!("{}", String::from_utf8_lossy(&response));
    Ok(())
}

/// Builds a DNS config with one local hostname pointing at loopback.
///
/// # Parameters
///
/// - `host`: Local hostname that the proxy will resolve through `arachnea-dns`.
///
/// # Returns
///
/// DNS core configuration containing the local A record.
fn dns_config_with_local_host(host: &str) -> CoreConfig {
    CoreConfig {
        local_records: vec![LocalRecord {
            name: host.to_owned(),
            records: vec![Record {
                name: host.to_owned(),
                ttl: 60,
                data: RecordData::A {
                    address: "127.0.0.1".parse().expect("valid loopback IPv4 address"),
                },
            }],
            wildcard: false,
            priority_over_upstream: true,
        }],
        ..DnsUsageProfile::SystemRelay.config()
    }
}

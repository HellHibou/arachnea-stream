use crate::core::ArachneaDnsCore;
use crate::server::{handle_packet, ServerConfig};
use anyhow::{Context, Result};
use ipnet::IpNet;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::Mutex,
};
use tracing::{error, info, warn};

/// DNS server wrapper around one shared core instance.
/// 
/// This struct represents a complete DNS server that listens for DNS
/// queries on UDP and TCP sockets, applies access control policies,
/// and delegates resolution to a shared DNS core instance.
pub struct Server {
    /// Server configuration.
    /// 
    /// Configuration for the DNS server's listeners and network settings.
    config: ServerConfig,
    /// Shared DNS core instance.
    /// 
    /// The DNS resolver core that performs actual DNS resolution. This
    /// is shared among all listener tasks and provides caching, policy
    /// enforcement, and upstream communication.
    core: Arc<ArachneaDnsCore>,
}

impl Server {
    /// Creates a server from listener config and a core instance.
    ///
    /// # Parameters
    ///
    /// - `config`: DNS listener configuration.
    /// - `core`: DNS core shared by listener tasks.
    ///
    /// # Returns
    ///
    /// Server wrapper ready to start.
    pub fn new(config: ServerConfig, core: ArachneaDnsCore) -> Self {
        Self {
            config,
            core: Arc::new(core),
        }
    }

    /// Starts UDP and TCP listeners and runs until the task is cancelled.
    ///
    /// # Errors
    ///
    /// Returns an error when listener startup or runtime I/O fails.
    pub async fn start(self) -> Result<()> {
        if !self.config.enabled {
            warn!("server is disabled by configuration");
            return Ok(());
        }

        let rate_limit = self
            .core
            .effective_config()
            .config
            .security
            .rate_limit_per_client_per_second;
        let udp_policy = Arc::new(RuntimePolicy::new(
            &self.config,
            self.config.listen_udp,
            rate_limit,
        )?);
        let tcp_policy = Arc::new(RuntimePolicy::new(
            &self.config,
            self.config.listen_tcp,
            rate_limit,
        )?);

        let udp = run_udp_with_policy(self.config.listen_udp, Arc::clone(&self.core), udp_policy);
        let tcp = run_tcp_with_policy(self.config.listen_tcp, Arc::clone(&self.core), tcp_policy);
        tokio::try_join!(udp, tcp)?;
        Ok(())
    }

    /// Flushes transient state before shutdown.
    pub async fn shutdown_gracefully(&self) {
        self.core.flush_cache().await;
    }
}

/// Runs the UDP DNS listener.
///
/// # Parameters
///
/// - `addr`: UDP socket address to bind.
/// - `core`: Shared DNS core used for packet handling.
///
/// # Errors
///
/// Returns an error when binding or socket receive operations fail.
pub async fn run_udp(addr: SocketAddr, core: Arc<ArachneaDnsCore>) -> Result<()> {
    run_udp_with_policy(
        addr,
        core,
        Arc::new(RuntimePolicy::allow_loopback_default(addr, None)),
    )
    .await
}

/// Runs the UDP DNS listener with runtime ACL and rate-limit policy.
///
/// # Parameters
///
/// - `addr`: UDP socket address to bind.
/// - `core`: Shared DNS core used for packet handling.
/// - `policy`: Runtime policy applied to incoming peers.
///
/// # Errors
///
/// Returns an error when binding or socket receive operations fail.
async fn run_udp_with_policy(
    addr: SocketAddr,
    core: Arc<ArachneaDnsCore>,
    policy: Arc<RuntimePolicy>,
) -> Result<()> {
    let socket = Arc::new(
        UdpSocket::bind(addr)
            .await
            .with_context(|| format!("cannot bind UDP listener on {addr}"))?,
    );
    info!("DNS UDP listener ready on {}", socket.local_addr()?);

    let mut buf = vec![0_u8; 4096];
    loop {
        let (len, peer) = socket.recv_from(&mut buf).await?;
        let packet = buf[..len].to_vec();
        if !policy.allows(peer).await {
            warn!(%peer, "UDP DNS request rejected by ACL or rate limit");
            continue;
        }
        let socket = Arc::clone(&socket);
        let core = Arc::clone(&core);
        tokio::spawn(async move {
            match handle_packet(&core, &packet).await {
                Ok(response) => {
                    if let Err(error) = socket.send_to(&response, peer).await {
                        error!(%peer, %error, "failed to send UDP DNS response");
                    }
                }
                Err(error) => error!(%peer, %error, "failed to handle UDP DNS packet"),
            }
        });
    }
}

/// Runs the TCP DNS listener.
///
/// # Parameters
///
/// - `addr`: TCP socket address to bind.
/// - `core`: Shared DNS core used for stream handling.
///
/// # Errors
///
/// Returns an error when binding or accepting TCP connections fails.
pub async fn run_tcp(addr: SocketAddr, core: Arc<ArachneaDnsCore>) -> Result<()> {
    run_tcp_with_policy(
        addr,
        core,
        Arc::new(RuntimePolicy::allow_loopback_default(addr, None)),
    )
    .await
}

/// Runs the TCP DNS listener with runtime ACL and rate-limit policy.
///
/// # Parameters
///
/// - `addr`: TCP socket address to bind.
/// - `core`: Shared DNS core used for stream handling.
/// - `policy`: Runtime policy applied to incoming peers.
///
/// # Errors
///
/// Returns an error when binding or accepting TCP connections fails.
async fn run_tcp_with_policy(
    addr: SocketAddr,
    core: Arc<ArachneaDnsCore>,
    policy: Arc<RuntimePolicy>,
) -> Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind TCP listener on {addr}"))?;
    info!("DNS TCP listener ready on {}", listener.local_addr()?);

    loop {
        let (stream, peer) = listener.accept().await?;
        if !policy.allows(peer).await {
            warn!(%peer, "TCP DNS connection rejected by ACL or rate limit");
            continue;
        }
        let core = Arc::clone(&core);
        tokio::spawn(async move {
            if let Err(error) = handle_tcp_stream(stream, core).await {
                error!(%peer, %error, "failed to handle TCP DNS stream");
            }
        });
    }
}

/// Handles one TCP DNS stream.
///
/// # Parameters
///
/// - `stream`: TCP stream carrying length-prefixed DNS messages.
/// - `core`: Shared DNS core used for packet handling.
///
/// # Errors
///
/// Returns an error when reading, writing, or DNS packet handling fails.
async fn handle_tcp_stream(mut stream: TcpStream, core: Arc<ArachneaDnsCore>) -> Result<()> {
    loop {
        let mut len_bytes = [0_u8; 2];
        if stream.read_exact(&mut len_bytes).await.is_err() {
            return Ok(());
        }
        let len = u16::from_be_bytes(len_bytes) as usize;
        let mut packet = vec![0_u8; len];
        stream.read_exact(&mut packet).await?;

        let response = handle_packet(&core, &packet).await?;
        stream
            .write_all(&(response.len() as u16).to_be_bytes())
            .await?;
        stream.write_all(&response).await?;
    }
}

/// Runtime ACL and rate-limit policy for DNS listeners.
/// 
/// This struct implements access control and rate limiting for DNS server
/// listeners. It enforces network-based access control lists and per-client
/// request rate limits to protect against abuse and unauthorized access.
struct RuntimePolicy {
    /// Allowed client networks.
    /// 
    /// List of IP networks (in CIDR notation) that are permitted to query
    /// this DNS server.
    allow: Vec<IpNet>,
    /// Denied client networks.
    /// 
    /// List of IP networks that are explicitly denied access, even if they
    /// would be allowed by the allow list. Deny rules take precedence.
    deny: Vec<IpNet>,
    /// Whether to allow loopback clients when allow list is empty.
    /// 
    /// If true and the allow list is empty, clients connecting from
    /// loopback addresses (127.0.0.1, ::1) are allowed. This provides
    /// sensible defaults for development and testing.
    allow_loopback_when_empty: bool,
    /// Per-client rate limit in requests per second.
    /// 
    /// Optional rate limit that restricts how many requests each client
    /// IP address can make per second. None means no rate limiting.
    rate_limit_per_second: Option<u32>,
    /// Rate limit buckets for each client IP.
    /// 
    /// Hash map tracking the request count for each client IP address
    /// within the current rate limiting window.
    buckets: Mutex<HashMap<IpAddr, RateBucket>>,
}

impl RuntimePolicy {
    /// Creates a runtime policy from server configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Server configuration containing ACL settings.
    /// - `listen_addr`: Listener address used for loopback defaults.
    /// - `rate_limit_per_second`: Optional per-client request limit.
    ///
    /// # Returns
    ///
    /// Runtime policy ready for listener use.
    ///
    /// # Errors
    ///
    /// Returns an error when configured CIDR networks are invalid.
    fn new(
        config: &ServerConfig,
        listen_addr: SocketAddr,
        rate_limit_per_second: Option<u32>,
    ) -> Result<Self> {
        let allow = parse_networks(&config.network.allow_clients)?;
        let deny = parse_networks(&config.network.deny_clients)?;
        Ok(Self {
            allow,
            deny,
            allow_loopback_when_empty: listen_addr.ip().is_loopback(),
            rate_limit_per_second,
            buckets: Mutex::new(HashMap::new()),
        })
    }

    /// Creates a default policy that allows loopback clients for loopback binds.
    ///
    /// # Parameters
    ///
    /// - `listen_addr`: Listener address used to determine loopback behavior.
    /// - `rate_limit_per_second`: Optional per-client request limit.
    ///
    /// # Returns
    ///
    /// Runtime policy with no explicit ACL lists.
    fn allow_loopback_default(listen_addr: SocketAddr, rate_limit_per_second: Option<u32>) -> Self {
        Self {
            allow: Vec::new(),
            deny: Vec::new(),
            allow_loopback_when_empty: listen_addr.ip().is_loopback(),
            rate_limit_per_second,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns whether a peer is allowed by ACL and rate limit.
    ///
    /// # Parameters
    ///
    /// - `peer`: Remote peer socket address.
    ///
    /// # Returns
    ///
    /// `true` when the peer may submit a request.
    async fn allows(&self, peer: SocketAddr) -> bool {
        let ip = peer.ip();
        if self.deny.iter().any(|network| network.contains(&ip)) {
            return false;
        }

        let acl_allowed = if self.allow.is_empty() {
            self.allow_loopback_when_empty && ip.is_loopback()
        } else {
            self.allow.iter().any(|network| network.contains(&ip))
        };

        acl_allowed && self.check_rate_limit(ip).await
    }

    /// Applies the per-IP rate limit.
    ///
    /// # Parameters
    ///
    /// - `ip`: Client IP address to account.
    ///
    /// # Returns
    ///
    /// `true` when the client remains within the configured rate.
    async fn check_rate_limit(&self, ip: IpAddr) -> bool {
        let Some(limit) = self.rate_limit_per_second else {
            return true;
        };

        let now = Instant::now();
        let mut buckets = self.buckets.lock().await;
        let bucket = buckets.entry(ip).or_insert(RateBucket {
            window_started: now,
            count: 0,
        });

        if now.duration_since(bucket.window_started) >= Duration::from_secs(1) {
            bucket.window_started = now;
            bucket.count = 0;
        }

        if bucket.count >= limit {
            return false;
        }

        bucket.count += 1;
        true
    }
}

/// Per-client request counter for the current one-second window.
/// 
/// This struct tracks the number of requests made by a single client IP
/// address within the current rate limiting window. The window is
/// typically one second, and the count is reset when the window expires.
struct RateBucket {
    /// Timestamp when the current window started.
    /// 
    /// The instant in time when the current rate limiting window began.
    /// This is used to determine when the window should be reset.
    window_started: Instant,
    /// Number of requests in the current window.
    /// 
    /// The count of requests made by this client since window_started.
    /// When this count reaches the rate limit, further requests are
    /// rejected until the window resets.
    count: u32,
}

/// Parses CIDR network strings into IP network values.
///
/// # Parameters
///
/// - `values`: CIDR strings from server configuration (e.g., "192.168.1.0/24").
///
/// # Returns
///
/// Vector of parsed IP networks suitable for ACL matching.
///
/// # Errors
///
/// Returns an error when any CIDR string is invalid or cannot be parsed.
fn parse_networks(values: &[String]) -> Result<Vec<IpNet>> {
    values
        .iter()
        .map(|value| {
            value
                .parse::<IpNet>()
                .with_context(|| format!("invalid CIDR network `{value}`"))
        })
        .collect()
}

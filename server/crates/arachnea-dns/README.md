# arachnea-dns

`arachnea-dns` provides the DNS layer for Arachnea. It exposes a reusable resolver core and, with the `server` feature, UDP/TCP server entry points that adapt standard DNS traffic to the same core behavior.

This README is the design and usage home for the DNS crate. Remaining work is tracked in the root `docs/TODO.md`.

## Design Contract

- Keep resolver logic in the core and keep server code as protocol adaptation.
- Make transports interchangeable: system DNS, classic UDP/TCP, DoT, DoH, DoQ, and future ODoH.
- Keep policies configurable without code changes: upstreams, cache, filtering, anonymization, DNSSEC, timeouts, retries, and logging.
- Keep profiles inspectable. A profile must expand into effective configuration that callers can inspect.
- Do not make filtering, family-safety, blocklists, local records, or Smart DNS into profiles. They are composable features.
- Keep the cache per `ArachneaDnsCore` instance, disabled by default, and memory-only in v1.
- Apply DNS decisions in this order: `blocklist > local_records > smart_dns > cache > upstream`.
- Do not expose an administration endpoint in v1.
- Default server exposure must be conservative: loopback by default, explicit ACLs for network listeners, and no public recursion unless configured.

## Centralized defaults

`src/lib.rs` groups the crate's internal defaults and operational limits by
responsibility, with their owning feature gates. This includes resolver mode,
timeouts and retries, cache capacity and TTLs, privacy/security defaults, TLS
verification, file/listener defaults, recursive lookup bounds, receive buffers
and negative-evidence retention. Runtime configuration overrides and all existing
values are preserved; editing a constant requires recompilation.

The default resolver timeout is 2,000 milliseconds with two retries. The per-core
memory cache remains disabled; its default capacity is 100,000 entries and its
fresh TTL cap is 86,400 seconds. Stale retention has a separate 86,400-second
budget, and stale answers use a 30-second TTL when enabled. The server's default
listeners remain `127.0.0.1:5353` and its default per-client rate limit is 50
requests per second.

Core DNSSEC defaults remain `off`, while secure file profiles without an
explicit security override retain their `report_only` preset. A compact file
forwarder without a port retains the existing fallback of 853 for DoT and 443
for DoQ/ODoH; specify `port` explicitly for another endpoint, as the DoQ sample
does. These are preserved configuration defaults, not changes to transport
support or DNSSEC behavior. Equal-valued limits serving different roles remain
separate constants. Root hints still embed `resources/root_hints.txt` unchanged.

## Main Capabilities

- Asynchronous resolver API through `ArachneaDnsCore`.
- Resolution by name, record type, A/AAAA convenience, multi-query calls, cache flush, stats, and effective-config inspection.
- Typed errors for timeout, NXDOMAIN, NODATA, SERVFAIL, DNSSEC validation, invalid configuration, unavailable upstream, unsupported feature, blocked query, and protocol failures.
- Profiles: `system_relay`, `single_forwarder`, `resilience`, `privacy`, `secure`, `advanced`, and recursive behavior when no explicit upstream is configured.
- Upstream transports: system DNS, UDP, TCP with truncation fallback, DoH, DoT, DoQ, and a recursive resolver using embedded or configured root hints.
- Local records, blocklists, Smart DNS fallback records, per-instance cache, negative cache, serve-stale support, and initial aggressive DNSSEC negative caching through NSEC.
- Optional UDP/TCP server with TOML configuration, ACLs, per-client rate limiting, tracing logs, JSON log mode, and DNS response adaptation.

## Profiles

| Profile | Intent |
| --- | --- |
| `system_relay` | Use the operating system resolver as the source of truth, useful for local relay behavior, cache, logs, blocks, or local records. |
| `single_forwarder` | Forward through one configured upstream over UDP, TCP, DoT, DoH, or DoQ. |
| `resilience` | Try multiple sources with fallback; NXDOMAIN fallback is the default resilience behavior. |
| `privacy` | Prefer encrypted upstreams, disabled ECS, minimized logs, and no unencrypted resolver unless explicitly configured. |
| `secure` | Prefer DNSSEC-aware and encrypted upstream behavior without forcing a public resolver. |
| `advanced` | Apply only explicitly configured behavior. |

DNS privacy reduces DNS leaks, but it does not anonymize all application traffic and does not replace a proxy, Tor, or a VPN when full traffic privacy is required.

## Features

| Feature | Purpose |
| --- | --- |
| `core` | Enables the reusable resolver core. |
| `server` | Enables configuration-file support and UDP/TCP server entry points. |
| `recursive` | Enables recursive-resolution support used by the core feature set. |
| `dot` | Enables DNS-over-TLS dependencies. |
| `doh` | Enables DNS-over-HTTPS dependencies. |
| `doq` | Enables DNS-over-QUIC dependencies. |
| `dnssec` | Enables DNSSEC-related resolver dependencies. |
| `odoh` | Reserves support for Oblivious DoH work. |
| `android` | Reserves Android-specific integration support. |

Profiles must fail with a clear configuration error when a required feature is missing.

## Current Status

- `ArachneaDnsCore` and the core public types are implemented.
- System, UDP, TCP, DoH, DoT, DoQ, recursive resolution, configurable root hints, and DNSSEC validation through Hickory are present behind their features.
- The recursive resolver follows referrals, uses glue when available, resolves missing NS addresses recursively, falls back to TCP on truncation, and enforces depth limits.
- TOML server configuration supports profile, server, network, resolver, forwarder, upstreams, cache, privacy, security, local records, and blocklists.
- Smart DNS public types are present; answer and proxy-target fallback behavior exist, while upstream route actions and full TOML loading remain TODO.
- The DNS server exposes `serve`, `resolve`, `validate-config`, and `show-effective-config`.

## Known Gaps

The durable remaining work from the retired specification is tracked in the root `docs/TODO.md`. Important DNS gaps include ODoH transport, finer DNSSEC modes, Smart DNS route/TOML support, external blocklist reload, NSEC3 aggressive-cache synthesis, DNS Cookies, alternative roots, optional proxy-backed DNS egress, and censorship metadata through API and EDNS Extended DNS Error.

Several accepted configuration switches are not enforced yet: resolver `retries`, `parallel_upstreams`, `allow_insecure_fallback` and `upstream_strategy`; upstream `verify_tls`; privacy `qname_minimisation`, `ecs`, `odoh`, `query_padding` and `log_qnames`; `dns_cookies`; and the server `allow_recursion` ACL. The current forwarder tries upstreams in declaration order and uses library-default TLS settings. Only DNSSEC `Off` and `Strict` change resolution; the intermediate modes are metadata labels for now.

## Common Commands

Run an example through the system resolver profile:

```powershell
cargo run -p arachnea-dns --example system_relay -- www.google.lu
```

Validate a configuration:

```powershell
cargo run -p arachnea-dns --features server --bin arachnea-dns-server -- validate-config crates/arachnea-dns/config-sample/system_relay.toml
```

Show the effective configuration:

```powershell
cargo run -p arachnea-dns --features server --bin arachnea-dns-server -- show-effective-config --config crates/arachnea-dns/config-sample/system_relay.toml
```

Run the DNS server:

```powershell
cargo run -p arachnea-dns --features server --bin arachnea-dns-server -- serve --config crates/arachnea-dns/config-sample/system_relay.toml
```

Resolve a record through a configuration:

```powershell
cargo run -p arachnea-dns --features server --bin arachnea-dns-server -- resolve example.com --record-type A --config crates/arachnea-dns/config-sample/system_relay.toml
```

## Useful Paths

- `src/lib.rs`: feature-gated internal constants and existing public module reexports.
- `src/core/`: reusable resolver core, policies, configuration, cache, transports, recursive resolver, and errors.
- `src/server/`: server configuration, UDP/TCP listeners, packet handling, and CLI support. The private listener module is named `runtime` and loads the unchanged `src/server/server.rs` source through `#[path]`; public `server::{Server, run_udp, run_tcp}` and crate-root reexports remain available. Listener events keep the `arachnea_dns::server::server` tracing target for existing log filters, even though their private module metadata now names `runtime`.
- `tests/core/config.rs`, `tests/core/cache.rs`, `tests/core/resolver.rs`, and `tests/server/config.rs`: existing tests grouped by the exact source file they cover. Conditional path declarations preserve private access and feature gates; function-name filters still work, while qualified test names now follow the owning source module. No tests remain under `src/`.
- `resources/root_hints.txt`: embedded default root hints.
- `config-sample/`: TOML examples for supported profiles and transports.
- `examples/`: Rust API usage examples.

Run the existing tests with `cargo test -p arachnea-dns` for the default
features, `cargo test -p arachnea-dns --no-default-features --features core --lib`
for the minimal core, or `cargo test -p arachnea-dns --all-features` to include
server configuration tests. The current suites use local configuration and
cached/static evidence; they do not establish live interoperability with every
encrypted DNS endpoint.

## References

Important standards for this crate include RFC 8484, RFC 7858, RFC 9250, RFC 9230, RFC 9156, RFC 9076, RFC 7871, RFC 7873, RFC 8767, RFC 8198, RFC 8945, and RFC 9103. Hickory DNS remains the primary Rust DNS ecosystem reference used by the crate.

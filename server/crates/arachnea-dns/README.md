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

## Common Commands

Run an example through the system resolver profile:

```powershell
cargo run -p arachnea-dns --example system_relay -- www.google.lu
```

Validate a configuration:

```powershell
cargo run -p arachnea-dns -- validate-config crates/arachnea-dns/config-sample/system_relay.toml
```

Show the effective configuration:

```powershell
cargo run -p arachnea-dns -- show-effective-config --config crates/arachnea-dns/config-sample/system_relay.toml
```

Run the DNS server:

```powershell
cargo run -p arachnea-dns -- serve --config crates/arachnea-dns/config-sample/system_relay.toml
```

Resolve a record through a configuration:

```powershell
cargo run -p arachnea-dns -- resolve example.com --record-type A --config crates/arachnea-dns/config-sample/system_relay.toml
```

## Useful Paths

- `src/core/`: reusable resolver core, policies, configuration, cache, transports, recursive resolver, errors, and tests.
- `src/server/`: server configuration, UDP/TCP listeners, packet handling, and CLI support.
- `resources/root_hints.txt`: embedded default root hints.
- `config-sample/`: TOML examples for supported profiles and transports.
- `examples/`: Rust API usage examples.

## References

Important standards for this crate include RFC 8484, RFC 7858, RFC 9250, RFC 9230, RFC 9156, RFC 9076, RFC 7871, RFC 7873, RFC 8767, RFC 8198, RFC 8945, and RFC 9103. Hickory DNS remains the primary Rust DNS ecosystem reference used by the crate.

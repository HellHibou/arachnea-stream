# Security Policy

## Supported versions

Security fixes are applied to the latest release and to `master`. Please
update to the most recent version before reporting an issue.

## Reporting a vulnerability

**Do not open a public issue for security problems.**

Report vulnerabilities privately through GitHub's "Report a vulnerability"
button on the repository's *Security* tab (or by contacting the maintainers
directly if you know them). Include:

- a description of the issue and its impact;
- the steps or inputs needed to reproduce it;
- the affected component (desktop app, REST server, admin API, proxy,
  resolver, scraper engine) and, when possible, a minimal reproduction.

You will get an acknowledgment within a few days, followed by updates as the
issue is triaged. We will credit reporters in the release notes unless
anonymity is requested.

## Scope

The following areas are in scope:

- the HTTP/Tauri controllers and the administration API (authentication,
  sessions, CSRF protections);
- the built-in proxy and its listener (loopback posture, unintended exposure);
- the scraper engine's handling of untrusted YAML collections and untrusted
  remote content (injection, path traversal, SSRF);
- the stream/player resolvers and URL rewriting;
- credential storage and the encrypted persistence store;
- the release packaging and portable archives.

Out of scope:

- the legality or availability of the content offered by third-party sources
  described by YAML collections;
- issues in third-party websites' own protections;
- attacks that require physical access to an unlocked machine or an already
  compromised host.

## Hardening notes for operators

- The server binds to loopback by default. Binding to `0.0.0.0`
  (`--server-public`) exposes the web interface and API to your local
  network: the admin API then requires authentication for non-loopback
  clients, so set an administrator password before doing so.
- The proxy listener is loopback-only unless explicitly configured otherwise.

# Local TLS test fixtures

These certificates and the PKCS#8 private key are public test-only material,
generated locally with OpenSSL. Never use this key or trust this CA outside tests.
The CA private key is not stored in the repository.

- `test-ca.der`: self-signed test CA, explicitly trusted by the Rquest fixture.
- `localhost-cert.der`: server certificate signed by that CA, valid for
  `localhost` and `127.0.0.1`, with server-authentication usage.
- `localhost-key.der`: unencrypted PKCS#8 server private key for the local fixture.

Certificates are valid from October 14, 2026 until October 11, 2036. Regenerate
the CA/server pair before expiration if these tests are still maintained. Tests
retain certificate and hostname verification; they do not disable TLS checks.
The TLS fixture uses one accepted connection for two HTTP responses to prove
keep-alive reuse through the pinned loopback route.
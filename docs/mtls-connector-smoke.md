# Real Rustls client-certificate connector stage

This stacked draft includes a real Rustls client connector construction path
in src/transport/mtls_connector.rs. It parses a dedicated PEM CA and client
certificate/key without silently selecting anonymous TLS, a system-root
trust store or plaintext. Sensitive PEM bytes and paths are not retained in
the returned typed errors. The connector type is the same tokio-rustls
TlsConnector used by the exact pinned ORESoftware/ores-transport TCP client.

Source CI constructs ephemeral per-run CA/server/client and untrusted CA
certificates with OpenSSL (no committed private key or shared credential).
The independent public-dependency crate compiles the EXACT production
connector source, then executes an actual TCP+TLS 1.3 handshake. It covers:

- trusted client certificate and trusted localhost server (positive)
- missing client certificate (server refuses)
- client certificate signed by an untrusted CA (server refuses)
- an otherwise trusted server under the wrong DNS name (client refuses)
- missing, oversized and malformed PEM material (connector refuses)

The pre-existing src/mtls_policy.rs 32-state table remains the source of
truth for choosing MutualTls vs explicit sidecar plaintext. This builder
does not enable a TCP route, supply a trusted server name, or alter startup;
the service must combine the existing fail-closed policy with the connector
when approved. This check does NOT prove that the real private
ores-transport crate compiled, or that PersistentTcpClient actually
connected through the four-avenue Gateway. The private Cargo admission and
real end-to-end transport checks remain required. The signer and certificate
rotation/revocation architecture also requires independent security review.
No production /readyz behavior changes.

## Additional negative handshake coverage

The source-owned Rustls fixture also checks that a client with an unrelated
pinned CA refuses the otherwise valid localhost server (no system trust-store
fallback), that a real trusted-CA-signed *expired* client certificate is
rejected by the server, and that an endpoint responding with raw plaintext
cannot complete a TLS session or trigger an automatic plaintext retry. The
expired client certificate is issued with fixed 2020 validity using an
ephemeral per-run CA and OpenSSL; no sensitive key is committed. This is still
a connector-level test, not the private ores-transport PersistentTcpClient.

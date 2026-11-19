# Fanwaave web listener recovery (partial implementation of #19)

The old startup path printed the bind address and returned success without
opening a listening socket. This candidate binds a real nonblocking loopback
socket and serves limited static HTTP routes via Axum:

- GET /: static homepage only
- GET /healthz: process liveness only, never backend readiness
- GET /readyz: always HTTP 503 until an authenticated gateway is wired

This does NOT implement DirectReader, authorize operations, use the database,
start outbound four-transport operations, or prove mTLS handshakes. No
application API endpoints are exposed in this stage.

## Startup admission

Before binding, production code refuses non-loopback or malformed addresses
and configured API, NATS, database, or client-certificate inputs, even when
explicitly empty. Startup and Debug errors retain only an OS error kind,
never credential-bearing raw error text.

The source CI compiles and runs isolated tests of the actual production
listener-admission module with rustc --test before cargo fetch. Those tests
demonstrate real socket binding on loopback and rejection of unsafe startup.
They do NOT prove the Axum app compiles or works end-to-end.

## Remaining issue #19 gates

1. Obtain authorized SHA-pinned private Cargo source access (issue #18).
2. Implement an authenticated, read-only DirectReader and connect
   four_transports::gateway_from_env BEFORE binding any public socket.
3. Verify actual local TLS/mTLS handshakes, negative certificate cases,
   envelope authorization, shutdown, and protection against plaintext retry.
4. Execute whole-crate cargo check, cargo clippy and cargo test on the
   exact PR head, followed by independent security review and branch policy.

Never deploy this static-only stage as the authenticated production service.

## Isolated live HTTP verification

The same-repository `test-support/http-smoke` crate compiles the **actual**
production `src/server.rs`, `src/transport/listener.rs`, and page modules
against public Axum/Tokio dependencies. It replaces only the upstream-dependent
`WebConfig` type with a narrowly equivalent test shape. With an ephemeral
loopback socket, it sends real HTTP requests to the production router, verifies
home/health/liveness, unready and unknown-route statuses, and exercises a
bounded graceful shutdown. It also checks that production `run` rejects a
public bind and configured-but-unwired upstream without opening a socket.

This **does not** compile the complete production app or exercise real database
queries, `DirectReader`, four-transport dispatch, credential authorization,
or TLS. The unprivileged, no-secret workflow runs this before private Cargo
admission. Do not treat a passing isolated harness as full app acceptance.

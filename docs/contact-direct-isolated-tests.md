## Dependency-isolated actual adapter test

The source CI job uses a PostgreSQL 16 service and imports the exact
production src/transport/contact_direct.rs module into a Rust test crate,
while providing ONLY a small local stand-in for the inaccessible private
ores-transport DirectReader trait. It depends on the actual public
fanwaave-lib-core and fanwaave-interfaces immutable draft SHAs. The
test executes the real reader with canonical migration-backed fixtures,
valid signed grants, cross-job grants, unknown kid and wrong scope.

This is **not** proof that the actual private ores-transport crate compiled
or that HTTP/TCP/NATS dispatch, TLS, authorization issuance, or full web-server
Cargo checks passed. Those source admission gates remain blocked. The
fixture migration is downloaded from the public, pinned core revision and
verified against its Git blob SHA before PostgreSQL applies it.

No runtime source, token or private Git history is copied between orgs.

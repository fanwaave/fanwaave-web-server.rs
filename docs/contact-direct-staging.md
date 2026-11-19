# Scoped contact DirectReader staging — security draft

This branch is **stacked on web-server draft #20** and changes no main/dev
source. It proposes a narrow adapter from the approved contact-read types
to ores_transport::DirectReader<ContactReadOperation, Option<ContactJobView>>.

Dependencies:
- fanwaave-interfaces draft #24, 9fe7e98120a4fade4092fc5cf2eb555041fb26ef
- fanwaave-lib-core draft #29, e0563dbfe268ebba6c7eb05e9a3c9df368f58b0a
- ORESoftware/ores-transport pinned 7d4c88d68528608de52fdf0d9b538b55100e1f0b

This is a PROPOSED draft dependency repin only, not a released or reviewed
change to web-server #20. The production static listener remains unchanged
and /readyz remains HTTP 503.

The adapter handles only GetContactJob. It validates the canonical job UUID
before calling ReadContext::get_contact_job, which independently validates
the signed resource-bound bearer, key rotation, expiry, principal, issuer,
audience and read scope BEFORE parameterized SQL. Authentication failures
map to TransportError::Unauthorized; schema/data errors map to
InvalidResponse; database and configuration failures map to Upstream.
Unknown IDs are values (None), not fabricated jobs.

This module contains NO public HTTP route, no JWT issuer, no default
credentials, no global secret fallback, no TLS connector and no bypass
of the static listener's fail-closed admission. An authorized service issuer
and actual authenticated HTTP/TCP/NATS handlers are still required.

**CI limits:** ores-transport and k8s-web-api-data-plane are private
ORESoftware dependencies. The source Actions runner is available but
cannot fetch these authorities without owner-approved read credentials.
Rustfmt/FP/static tests alone cannot establish full adapter compilation.
No cross-org private source mirror is authorized. Maintain this draft and
DO NOT repin, merge, deploy or mark production readiness prematurely.

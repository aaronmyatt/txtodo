# relay-access-control-adr

## Summary

ADR and doc corrections for relay access control: record the node-id mechanism, disambiguate the
iroh relay (what `--relay` points at) from `relay/`'s blob mailbox, and fix README/docs/relay.md's
"this project does not run a public relay" claim against ADR 0018's hosted decision.

## As built

ADR 0027 written (the node-id gate via iroh-relay's `access.http`, the OTP-verified accounts
service, colocated, with the QUIC-address-discovery and fail-closed costs recorded). ADR 0018's
justification marked superseded, its hosted/manual-grant decision left standing. README,
`docs/relay.md` and `docs/questions.md` Q5 no longer say the project runs no public relay, and now
keep the iroh relay and `relay/`'s blob mailbox apart by name. relay's two doc-drift tests
re-verified green.

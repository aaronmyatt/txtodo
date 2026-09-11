# 0003 — Use iroh for QUIC, hole-punching and relay; mdns-sd for LAN discovery

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 003; do not relitigate)

## Context
Sync must work on a LAN with no server, across the internet with NAT, and through an optional dumb relay. Three transports from three crates would triple the surface.

## Decision
We will use iroh for the QUIC endpoint, hole-punching and relay, and `mdns-sd` for `_txtodo._udp` discovery on the LAN. M4 enables local discovery only; M8 turns on relay.

## Consequences
- Good: one dependency covers three carriers; iroh's relay is the reference relay's transport.
- Bad: iroh is young; its relay protocol may change under us.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- libp2p: larger, more moving parts than we use.
- Hand-rolled QUIC + STUN: the hole-punching corner cases are the whole job.

# 0017 — MCP is desktop-only; iOS agents are relay-only

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q4)

## Context
LAN MCP on iOS needs `NSLocalNetworkUsageDescription` and Bonjour service declarations (Apple
docs: https://developer.apple.com/documentation/bundleresources/information-property-list/nslocalnetworkusagedescription),
a permission prompt and App Store review surface the other platforms don't carry. M9 needed a
decision before building the iOS MCP transport.

## Decision
We will scope MCP to desktop platforms only — macOS, Linux, Windows. iOS has no LAN MCP transport;
an agent talking to a user's tasks from an iOS context goes through the relay instead.

## Consequences
- Good: avoids the Local Network permission prompt and Bonjour entitlement surface on iOS
  entirely; desktop MCP ships without an iOS-shaped compromise in its design.
- Bad: an agent running natively on iOS cannot reach a LAN daemon directly — it is relay-only,
  which was already the plan for cross-device sync (see ADR 0018) but is now also the only path
  for agent tool calls from iOS.
- Neutral / follow-ups: M9 iOS MCP transport work is now "wire the relay path," not "wire LAN MCP."

## Alternatives considered
- Accept the Local Network permission prompt on iOS: doable, but adds a user-facing permission ask
  and App Store review scrutiny for a platform that already has a relay path available.

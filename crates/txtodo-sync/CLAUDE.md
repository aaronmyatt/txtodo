# txtodo-sync

## Purpose
Protocol, transports, pairing, crypto. Plan M4/M8.

## Public interface
Hello/Want/Ops/Ack messages, transports, `pair`, group key rotation.

## Invariants
- Every message versioned, authenticated, encrypted. Keys only in keystore.
- May depend only on: txtodo-model, txtodo-store.

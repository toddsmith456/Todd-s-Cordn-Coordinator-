## Cordn Coordinator 0.5.9 (Android)

### Fixes and resilience improvements

- **Publish Relay List fix:** Resolved `invalid args \`payload\` for command \`publish_relay_list\`: missing field \`privateKeyHex\``. The publish command now resolves identity/config natively (Android KeyStore / local identity file) and no longer requires private key material from the frontend.
- **R1/R2 recovery hardening:** Transient transport/runtime failures no longer exhaust a small retry budget and stop permanently. The coordinator keeps the foreground service alive in a degraded reconnecting state with capped exponential backoff (60s, 2m, 5m, 10m, 15m, 30m, 60m cap), resets after sustained healthy runtime, and distinguishes obvious fatal config/identity errors.
- **R3 relay health gating:** Status/health labels distinguish total relay loss, partial connectivity, and full connectivity. Relay counts, rejected subscriptions, and repair activity are surfaced in diagnostics/UI.
- **R6 diagnostics:** Recovery attempt count, next retry time, and last fatal reason are exposed in status/UI/notification state.
- **R7 transport resilience:** Vendored ContextVM SDK server event loop patched to re-acquire relay notifications on broadcast channel closure instead of exiting fatally.

### Install / update

Install over the existing Todd fork; **do not uninstall or clear app data** unless you intend to reset identity/state.

**Samsung Galaxy A01: choose armv7.** Other supported devices can use arm64 as appropriate.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.

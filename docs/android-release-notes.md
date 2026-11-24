## Private beta: R2 + R6 relay resilience/diagnostics

- **R2 (recovery backoff):** Replaced the 3-attempts-per-hour recovery budget with exponential backoff (60s, 2m, 5m, 10m, 15m, 30m, 60m cap). The coordinator no longer gives up permanently after transient transport failures; it keeps retrying with capped backoff. Recovery streak resets after 5 minutes of healthy runtime or on manual start.
- **R6 (diagnostics):** Status now surfaces recovery attempt count, next retry time, and last fatal reason. Health label shows relay counts/rejected subscriptions, and the UI displays recovery state and relay health details.
- Includes prior beta fix: `publish_relay_list` no longer requires frontend private key payload.

**Beta build** — intended for testing relay-outage recovery and diagnostics. Install over the existing Todd fork; do not uninstall or clear app data unless you intend to reset.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.

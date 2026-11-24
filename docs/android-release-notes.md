## Private beta: R1 + R3 + R7 relay resilience hardening

- **R1 (defense in depth):** Coordinator service distinguishes fatal config/identity errors from transient transport failures. Transient failures keep the foreground service alive in a degraded reconnecting state with capped exponential backoff (from R2), instead of giving up.
- **R3 (multi-relay health gating):** Health labels now distinguish partial connectivity (`Partially connected (x/total)`) from full connectivity and from total relay loss. UI status dot/gating treats partial connectivity as running-with-warning; relay counts/rejected subscriptions/repairs remain visible.
- **R7 (vendored SDK patch):** Patched vendored `contextvm-sdk` server event loop to re-acquire relay notifications on broadcast channel closure instead of exiting fatally. This reduces "whole service quits" risk from recoverable notification-channel/pool churn.
- Includes prior beta fixes: publish relay list payload fix, R2 backoff, R6 diagnostics.

**Beta build** — intended for testing relay-outage recovery, partial relay loss, and service resilience. Install over the existing Todd fork; do not uninstall or clear app data unless you intend to reset.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.

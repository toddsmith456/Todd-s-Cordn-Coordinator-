## Todd's Coordinator 0.5.11 (Android)

### Fixes

- **Publish Relay List freeze fix:** The "Publish relay list" action no longer risks freezing the app. The publish command now runs off the UI path, checks relay connectivity, enforces an overall 30s timeout, disconnects the temporary publish client cleanly, and returns clear errors when no relays connect, no relay accepts the event, or publishing times out.
- Includes 0.5.9 resilience/diagnostics improvements (publish payload fix, R1/R2/R3/R6/R7 relay recovery and health diagnostics).

### Install / update

Install over the existing Todd fork; **do not uninstall or clear app data** unless you intend to reset identity/state.

**Samsung Galaxy A01: choose armv7.** Other supported devices can use arm64 as appropriate.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.

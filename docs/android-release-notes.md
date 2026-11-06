## Local health and targeted recovery

- Replaces the misleading “ready” status with passive local transport health: relay counts, ingestion progress, subscription rejections and stale/unavailable status.
- The app and notification explicitly say **delivery unverified**. Quiet groups are not treated as failures; a relay connection or publish acknowledgement is not proof of client receipt.
- Transient `rate-limited:` / `error:` subscription closures retry only the same known subscription and existing relay, starting after 60 seconds with exponential backoff up to 15 minutes. Authorization/blocked rejections are not automatically retried.
- Supervises permanent ingestion/cleanup/maintenance tasks, without treating completed announcement jobs as failures. Stream maintenance no longer blocks ingestion.
- Network-change callbacks assist existing reconnect logic; no new permissions, relay destinations or active health probes.
- Recovery after native errors waits for the native call to return, then waits 60 seconds. At most three recovery attempts per hour per service lifetime. No overlapping native runtimes or forced process kills. Manual Stop cancels pending recovery, including Stop during startup.
- Bounded local health snapshot is overwritten every 15 seconds. No analytics, external monitoring, payload logging, new identifiers, or identity/database resets were added. Existing diagnostic logging remains unchanged.
- Keeps the existing custom scheduled restarts, package ID, signing key, name/icon, identity, settings and stored data.

### Install or update

**Samsung Galaxy A01: choose armv7**, which worked for your previous version. Install over Todd’s existing fork; **do not uninstall or clear app data**. Other devices can use arm64 or x86_64 as appropriate. Original Cordn remains separate.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.

### Verification and limitations

SDK: 623 passing tests, including ingestion-death detection and a real localhost WebSocket relay’s CLOSED/backoff/re-subscribe/EOSE recovery. Shared host and frontend checks also run; release CI builds/signature-checks all three ABIs and runs Android scheduler, health and recovery-policy unit tests. No physical A01 screen-off/overnight endurance test is available. The original device-level cause remains unproven; this is targeted recovery, not a guarantee against Samsung power management, relay outages, every stall, or every regression. A non-cooperative native shutdown will not start a second runtime; manual intervention may still be required.

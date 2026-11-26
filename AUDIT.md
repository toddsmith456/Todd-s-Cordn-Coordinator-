# Todd's Coordinator — Beta Audit Report

**Date:** 2026-11-24  
**Repo:** `toddsmith456/Todd-s-Cordn-Coordinator-`  
**Audited commit (baseline):** `de6a1e5` (`master`)  
**Beta branch:** `beta/publish-relay-fix` (includes the publish-relay-list fix described below)  
**Beta tag pushed:** `android-v0.5.9-beta.1`  
**Scope:** Coordinator Android app / Tauri plugin / cordn-server runtime, with emphasis on relay resilience and the "one relay down kills the whole service" failure mode.

> This report is an audit with options. **No additional fixes were applied beyond the confirmed publish-relay-list crash reported in the screenshot**, which was fixed on the beta branch so the beta artifact is testable. All other items below are findings with remediation options for you to choose from.

---

## Executive summary

| Severity | Count | Highlights |
|---|---:|---|
| Critical | 1 | A transport critical-task failure kills the coordinator; Android recovery gives up after 3 attempts/hour and leaves the service dead until manual restart. A single-relay outage can plausibly contribute depending on SDK/pool behavior; the service is not resiliently isolated per relay. |
| High | 3 | Publish-relay-list command contract mismatch (fixed in beta); publish path has no per-relay timeout/partial-success UX; relay targets include hardcoded fallbacks with no operator visibility/control in UI. |
| Medium | 5 | Desktop duplicate `publish_relay_list` method; version skew; private key crosses plugin boundary; no publish reentrancy guard; status/health semantics can be misleading during degraded relay states. |
| Low / Hygiene | 4 | ACL/TOML sync footgun, release-note/version maintenance, unused payload type, logging/redaction review items. |

**The main question — “if one relay the coordinator uses goes down the entire service quits working”:** There is a credible end-to-end fatal path. The ContextVM transport's event loop is treated as a critical task; if it exits/fails, `cordn-server` bails, the Android native run returns error, `CordnService` retries up to 3 times per hour, then stops with “Coordinator exited; retry budget reached. Start manually to retry.” I did **not** prove from vendored SDK source that a single relay disconnect alone always triggers this; I did confirm the fatal propagation path and several resilience gaps that make relay problems capable of taking the whole service down. See §3.

---

## 1. Confirmed bug (fixed in beta)

### B1 — `publish_relay_list` fails: missing `privateKeyHex` (confirmed from screenshot + code)

**Severity:** High (user-facing feature broken)  
**Status:** **Fixed on `beta/publish-relay-fix`**  
**Evidence:**
- Screenshot error: `invalid args \`payload\` for command \`publish_relay_list\`: missing field \`privateKeyHex\``
- `apps/cordn-coordinator/src/lib/coordinator.ts:108-110` (master): invoked `publish_relay_list` with `{ payload: {} }`.
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/src/models.rs:122-126`: `PublishRelayListPayload { private_key_hex: String, relay_urls: Vec<String> }` — both required.
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/src/commands.rs:72-76`: Tauri command took `payload: PublishRelayListPayload`, so deserialization failed before reaching platform impl.
- `mobile.rs:120-143`: ignored `_payload` and obtained identity/config from Kotlin (`IdentityStore`/config file) — the frontend should never have needed to send private key material.
- `desktop.rs:289+`: also ignored `_payload` and read identity/config from local files.

**Root cause:** Command contract required frontend-supplied `privateKeyHex`/`relayUrls`, but the platform implementations already resolved those natively. The TS UI sent an empty payload.

**Fix applied in beta:**
- `commands.rs`: `publish_relay_list` takes no payload.
- `mobile.rs` / `desktop.rs`: method signatures take no payload.
- `coordinator.ts`: invokes `publish_relay_list` with no args.
- `desktop.rs`: removed duplicate `publish_relay_list` that returned `Unsupported` (see B2).

**Options / follow-up:**
- **Option B1-A (applied):** No-arg command; native resolves identity/config. *Recommended.* Keeps private key out of frontend invoke payload.
- **Option B1-B:** Make payload fields optional with defaults. *Not recommended* — preserves an unnecessary private-key-in-frontend contract.
- **Option B1-C:** Keep typed payload but have TS fetch/pass key. *Not recommended* — increases key exposure and duplicates identity logic.

---

## 2. Other confirmed bugs / concrete defects

### B2 — Desktop duplicate `publish_relay_list` method

**Severity:** Medium (desktop build break / dead confusing code)  
**Status:** **Fixed in beta**  
**Evidence:** `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/src/desktop.rs` had `pub fn publish_relay_list` at ~289 (real implementation) and again at ~404 returning `Err(crate::Error::Unsupported)`. Duplicate methods in the same impl are a Rust compile error. Android CI does not compile desktop cfg, so it slipped through.  
**Fix:** removed duplicate Unsupported method.  
**Options:** B2-A keep single real impl (applied); B2-B add desktop CI/check job (`cargo check --target ...` or at least `cargo check` with desktop cfg) to prevent recurrence.

### B3 — `publish_relay_list` total failure is surfaced as `InvalidConfig`

**Severity:** Medium  
**Evidence:** `android.rs:336` maps publish failure to `crate::Error::InvalidConfig(format!("publish failed: {error}"))`; `desktop.rs` same pattern. Network/publish failures are not config validation errors.  
**Impact:** UI/error handling may misclassify errors; logs/telemetry become misleading.  
**Options:**
- **B3-A:** Introduce/return a publish/network error variant and map it through the plugin error type.
- **B3-B:** Keep string error but rename prefix/category to `publish failed` / `network error` without claiming invalid config. Smaller change.

### B4 — Version skew

**Severity:** Low/Medium (release hygiene)  
**Evidence:** Handoff notes configs at 0.5.6 while released 0.5.8. Beta bump set `0.5.9-beta.1` in `package.json`, `tauri.conf.json`, and `src-tauri/Cargo.toml`. Other workspace crates keep their own versions.  
**Options:** B4-A add a release checklist/script that bumps all coordinator version surfaces; B4-B have CI assert tag version matches tauri.conf.json/package.json.

---

## 3. Relay resilience / “one relay down kills whole service” (primary audit)

This is the main concern you asked me to investigate. I traced the operational relay path, the health/failure propagation path, the Android recovery path, and the self-description publish path.

### 3.1 End-to-end fatal propagation path (confirmed)

1. **Transport event loop is critical.** In vendored `contextvm-sdk` (`src/transport/server/mod.rs`, vendor branch): the event loop task is spawned; when it exits, it sets `health.failed()` if not already failed (`if !loop_health.snapshot().critical_task_failed { loop_health.failed(); }`). `critical_task_failed()` also marks failed if any of the first 3 task handles is finished.
2. **Cordn runtime treats critical transport failure as fatal.** `crates/cordn-server/src/runtime.rs:134-147`: after `service.waiting().await`, if not cancelled and `task_health.snapshot().critical_task_failed` is true, it `bail!("critical transport task exited")`.
3. **Android native wrapper converts run error into service error.** `apps/.../android.rs:180-220`: `run_until` result error becomes `"Coordinator transport exited; local recovery may retry"` (or the specific error), writes status `error`, returns `Err`.
4. **CordnService retries, then gives up.** `CordnService.kt:131-151`: on native error, if `!recoveryPolicy.stopped && allowRecovery()`, it waits 60s and restarts. `RecoveryPolicy.kt`: max 3 recovery attempts per 1 hour (`elapsedRealtime` window). After budget exhausted: writes `"Coordinator exited; retry budget reached. Start manually to retry."`, stops foreground service, stops self.
5. **User-visible outcome:** coordinator appears dead; no automatic recovery until manual start, even if the underlying relay issue was transient.

**Evidence files:**
- `crates/cordn-server/src/runtime.rs:52-67,134-147`
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/src/android.rs:173-227`
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/android/src/main/kotlin/org/cordn/coordinator/service/CordnService.kt:114-151`
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/android/src/main/kotlin/org/cordn/coordinator/service/RecoveryPolicy.kt`
- vendored SDK: `src/transport/server/mod.rs` around 860-888, 606-613; `src/transport/server/health.rs` `failed()`.

### 3.2 Can one relay going down kill the whole service?

**Short answer:** *Plausibly yes, depending on SDK/pool behavior and deployment shape; not safely ruled out by current architecture.* 

**What I can confirm:**
- The coordinator uses a single ContextVM transport configured with `cfg.relay_urls` (`runtime.rs:273-319`, `with_relay_urls(cfg.relay_urls.clone())`). There is no application-level multi-transport failover.
- If the transport's notification/event loop closes or exits abnormally, the SDK marks critical failure. I observed `RecvError::Closed => break` in the vendored event loop and the post-loop `failed()` marking.
- If the configured relay set is effectively one relay (default config has two, but users can configure one; or all but one can be down), loss of connectivity can become total.
- Android recovery budget is small (3/hour). A flapping relay or repeated transport exits can exhaust it quickly, causing prolonged outage.

**What I could not fully prove without deeper SDK/runtime testing:**
- Whether nostr-sdk/ContextVM treats a single relay disconnect as fatal when other relays remain healthy. The SDK has CLOSED-subscription repair/health tracking (`health.closed`, `due_repairs`, `accepted`) which suggests some per-subscription resilience. But the fatal path above means any critical-task classification is catastrophic.
- Exact behavior when `wait_for_connection`/relay pool has partial connectivity.

**Risk assessment:** **Critical/High**. Even if a single relay disconnect is usually tolerated by the pool, the current architecture has no defense-in-depth: any critical transport failure cascades to full service death with limited automatic recovery.

### 3.3 Publish-relay-list path resilience (separate from operational transport)

**Evidence:** `android.rs:251-348`, `desktop.rs:289-356`.

- Creates a fresh nostr-sdk client per publish — good isolation from long-running transport.
- Adds all configured relays + hardcoded fallbacks, dedupes, connects, waits for connection (10s), then sends to all targets.
- On per-relay failure, `send_event_builder_to` output is inspected; sent/failed counts returned. Good.
- On total send failure (`Err(error)`), returns error — caller gets failure. UI shows error. No partial success if SDK returns Err rather than per-target failures.
- `wait_for_connection` is called but its result is not checked; if no relay connects in time, behavior depends on nostr-sdk (may proceed and fail send). That can produce a confusing “publish failed” after delay.
- Publish is user-initiated; no background retry. This is acceptable for a manual button, but the UI should clearly show per-relay outcome.

### 3.4 Self-description publish (cordn-server announce) is comparatively resilient

**Evidence:** `crates/cordn-server/src/announce.rs`, `runtime.rs:167-257`.

- Good: retries only failed relays, uses a ladder, caps retries by refresh interval, refresh republishes all targets, treats per-relay refusal/failure as non-fatal. `SelfPublisher::publish` returns failed targets; total error maps to all targets failed.
- Good: separate publisher client from ContextVM transport; fallback relays are publication targets only.
- Gap: initial `wait_for_connection(CONNECT_WAIT_SECONDS)` is awaited but not checked for success/failure; if zero connections established, first publish round may be noisy/ineffective.
- Gap: `RelayPublisher::connect` logs add-relay errors but still proceeds. Reasonable, but could expose “0 connected relays” to health/status.

### 3.5 Remediation options for relay resilience

Pick a combination; I recommend **R1 + R2 + R4 + R6**.

| Option | Change | Pros | Cons / Risk | Effort |
|---|---|---|---|---|
| **R1 — Defense in depth: do not let a single transport critical failure permanently kill service** | In Android wrapper/runtime, distinguish recoverable transport exit from fatal config/identity errors. Reset/recreate transport with backoff; keep service alive in degraded “reconnecting” state. Escalate to user only after long backoff or explicit fatal class. | Directly addresses “whole service quits working”. | Must avoid tight crash loops, duplicate DB writers, or split-brain runtimes. Needs careful state machine. | Med/High |
| **R2 — Increase/reshape recovery budget** | Change `RecoveryPolicy` from 3/hour to exponential backoff with cap (e.g. 1m, 2m, 5m, 15m, 30m, 60m...) and no hard “give up forever” for transient transport errors; or reset budget on healthy run duration. | Prevents permanent death after brief relay outage. | Can mask persistent bugs; needs max backoff and user-visible degraded state. | Low/Med |
| **R3 — Multi-relay minimum / relay health gating** | Require >=1 healthy relay before declaring running; expose per-relay status. If relay count drops but >=1 remains, stay running and warn. | Makes “one relay down” non-fatal by definition when others healthy. | Depends on SDK exposing per-relay health; may need transport/config changes. | Med |
| **R4 — Isolate publish-relay-list from transport health; add per-relay timeout/structured result** | Already isolated process-wise by fresh client; improve by explicit per-relay connect/send timeout, return per-relay statuses, treat >=1 success as success with warnings. | Better UX and resilience for the reported feature. | UI/model changes. | Med |
| **R5 — Add fallback operational relays / relay rotation** | If configured relays all fail, optionally try known-good fallback relays for operational transport (with explicit user opt-in). | Improves availability. | Privacy/trust/operational semantics change; coordinator identity/announcement implications. Needs product decision. | High |
| **R6 — Observability & user-facing diagnostics** | Surface “last fatal reason”, “recovery attempt N, next retry at T”, per-relay connected/rejected/subscription repair state in UI/diagnostics. | Users can tell relay outage from app bug; easier support. | UI work; careful not to leak sensitive data. | Low/Med |
| **R7 — Upstream/contextvm-sdk fix or fork patch** | If SDK marks critical on recoverable relay/pool conditions, patch vendored SDK (you already vendor it in-repo on `vendor/contextvm-sdk` branch / pinned rev). | Fixes root cause at right layer. | Requires SDK expertise and regression testing. | Med/High |

**Recommended first move:** R2 + R1 (minimum viable resilience), plus R6 for diagnosability. Then validate R3/R7 against contextvm-sdk behavior with an integration test that simulates one relay dropping while another remains up.

---

## 4. Security & privacy findings

### S1 — Private key crosses Tauri plugin boundary on Android publish path

**Severity:** Medium  
**Evidence:** `CordnServicePlugin.kt:179-205` returns `privateKeyHex` to Rust via plugin response; `mobile.rs:120-143` receives it and passes to `android::publish_relay_list`. The key originates from Android KeyStore-backed `IdentityStore`.  
**Risk:** Key material is exposed in-memory across Kotlin→Rust boundary and exists in a normal Rust String/hex decode path. Better than sending from frontend, but still broader exposure than necessary.  
**Options:**
- **S1-A:** Move nostr-sdk publish entirely into Kotlin/Rust side without returning raw key over plugin response; e.g., Kotlin calls a native function that takes encrypted reference / performs publish internally. (Best)
- **S1-B:** Keep current boundary but zeroize key buffers where possible and avoid logging. (Partial)
- **S1-C:** Accept current design as low-risk given local-only plugin boundary; document it. (Least effort)

### S2 — Publish payload previously accepted private key from frontend

**Severity:** High (design) — **fixed in beta** by B1-A.  
**Evidence:** old command contract required `privateKeyHex`. Fixed to no-arg.

### S3 — Logging / diagnostics may include sensitive operational details

**Severity:** Low/Medium  
**Evidence:** tracing logs to `cordn.log`; diagnostics returns tail. Relay URLs and errors logged. Private keys are not intentionally logged, but error strings should be audited.  
**Options:** S3-A add redaction rules for hex secrets/nsec patterns in log writer; S3-B document that diagnostics may contain relay URLs and operational errors.

---

## 5. Android lifecycle / service behavior

### L1 — Recovery gives up permanently after 3 attempts/hour

**Severity:** High (availability)  
**Evidence:** `RecoveryPolicy.kt` — 3 attempts in 3,600,000ms window; after that `CordnService.kt` writes error “retry budget reached. Start manually to retry.”  
**Options:** see R2. Also consider resetting budget after a sustained healthy period (e.g., 10 minutes running).

### L2 — `run_until` error mapping loses original error class

**Severity:** Low  
**Evidence:** `android.rs:180` maps any run_until error to `"Coordinator transport exited; local recovery may retry"` (string), discarding original cause except logged earlier? It writes `last_error` with that string. The actual transport error may be more actionable.  
**Options:** L2-A preserve original error message in status/log while keeping user-friendly summary; L2-B classify transient vs fatal.

### L3 — Native stop during startup/recovery edge cases

**Severity:** Medium (needs test)  
**Evidence:** `CordnService.kt` handles restartRequested/recoveryPending/stopped flags; generally careful. There are windows where stop + recovery timer interact. Existing tests? Not fully audited.  
**Options:** L3-A add Android unit tests for stop-during-recovery, stop-during-startup, recovery-budget-exhausted; L3-B add explicit state machine.

### L4 — Battery/Doze and OEM kills remain environmental

**Severity:** Medium (product reality)  
**Evidence:** release notes already say “not a guarantee against Samsung power management...”. Service uses foreground notification, battery exemption request.  
**Options:** L4-A keep honest UX; L4-B add health heartbeat age warning (already present); L4-C consider periodic user-visible “last healthy” timestamp.

---

## 6. nostr-sdk / publish correctness

### N1 — `wait_for_connection` result ignored

**Severity:** Medium  
**Evidence:** `android.rs:291-292`, `desktop.rs` equivalent, `runtime.rs:215-217` all call `client.wait_for_connection(Duration::from_secs(10)).await;` and ignore the return value.  
**Impact:** If no relay connects, code proceeds to publish; may fail late or behave SDK-dependently.  
**Options:** N1-A check return; if no connections, return clear error / partial status; N1-B log warning and proceed best-effort.

### N2 — Hardcoded fallback relays in publish path

**Severity:** Low/Medium  
**Evidence:** `android.rs:260-266` hardcodes `purplepag.es, relay.damus.io, relay.primal.net, nos.lol, relay.nostr.net` as fallbacks. Desktop duplicates same list.  
**Impact:** Good for discoverability; but operator cannot control/see them; failures there count in failed total. Also potential privacy/policy surprise.  
**Options:** N2-A show “published to X/Y including discovery fallbacks” in UI; N2-B make fallbacks configurable; N2-C keep hardcoded but document.

### N3 — EventBuilder kind 10002 content is empty string

**Severity:** Low  
**Evidence:** `EventBuilder::new(Kind::Custom(10002), "").tags(tags)` — correct for NIP-65 relay list. OK.  
No change needed.

### N4 — Publish sends to `all_targets` but tags only include configured `relay_urls`

**Severity:** Low (confirm intent)  
**Evidence:** `android.rs:298-306` builds `r` tags from `req.relay_urls` (configured) but sends event to `all_targets` (configured + fallbacks). This is reasonable: publish the list to fallbacks for discoverability, but the list itself names the coordinator’s operational relays. Confirm this matches product intent.  
**Options:** N4-A keep as-is, document; N4-B add explicit comment in code.

### N5 — Fresh client per publish, no explicit disconnect/cleanup

**Severity:** Low  
**Evidence:** client created in block_on, function returns. nostr-sdk client should drop; explicit disconnect is polite.  
**Options:** N5-A call `client.disconnect().await` / `shutdown` in a finally-like path.

---

## 7. CI / build / release hygiene

### C1 — Android release workflow creates GitHub Release on tag

**Severity:** Informational (process)  
**Evidence:** `.github/workflows/release-android.yml` triggers on `android-v*`, builds, signs, runs `scripts/publish-android-release.py` which creates/updates a GitHub Release and marks prerelease if version contains `-`.  
**Implication:** Pushing `android-v0.5.9-beta.1` creates a **prerelease**. “Private beta” depends on repo visibility; if repo is public, prerelease is public. I pushed the tag as requested to produce the artifact.

### C2 — No local Android build environment in sandbox

**Severity:** Operational  
**Evidence:** sandbox has no Android SDK/NDK, no Rust, no gradle. APK build relies on GitHub Actions.  
**Options:** C2-A accept CI-built artifact; C2-B provide a Docker/devcontainer with Android+Rust+Tauri toolchain; C2-C add workflow_dispatch artifact-only build.

### C3 — ACL/TOML sync footgun

**Severity:** Medium (process)  
**Evidence:** handoff + `permissions/default.toml` + `permissions/autogenerated/commands/publish_relay_list.toml` both present and aligned now.  
**Options:** C3-A add CI check that every command in generate_handler/default permissions has autogenerated TOML; C3-B script to generate/sync.

### C4 — `publish-android-release.py` assumes GH release API reachable and artifacts present

**Severity:** Low  
**Evidence:** script fails if no APKs found; debug-lists artifacts. OK.

---

## 8. Product/UX observations

### U1 — Publish result is coarse

**Severity:** Low/Medium  
**Evidence:** UI receives `{sent, failed, targets}`. It should display which relays failed and whether fallbacks were included.  
**Options:** U1-A show per-relay status; U1-B at minimum show “Published to sent/total targets (including discovery fallbacks)”.

### U2 — “Relay connected — delivery unverified” is honest but easy to miss

**Severity:** Low  
**Evidence:** `CoordinatorHealth.kt` / `desktop.rs` health_label. Good honesty.  
**Options:** keep; consider adding last-successful-publish timestamp for relay list.

### U3 — Manual publish has no busy/disable guard visible in audit scope

**Severity:** Low  
**Evidence:** `+page.svelte` has `publishingRelayList` flag (seen in grep context). Good - prevents double click. Confirmed.

---

## 9. Suggested validation plan (after you pick options)

1. **Relay-down reproduction:** run coordinator with 2 relays; kill one relay after steady state; confirm service remains running and health degrades gracefully. Then kill all relays; confirm recovery/backoff behavior and that service returns when relays return (without manual restart, if R1/R2 chosen).
2. **Transport crash simulation:** inject/mock critical task failure; verify RecoveryPolicy/backoff and final UX.
3. **Publish relay list:** verify beta fix on Android device; confirm kind 10002 appears on relays; confirm per-relay partial failure behavior.
4. **Desktop compile check:** `cargo check -p cordn-android-service` under desktop cfg (or CI job) to catch B2-class issues.
5. **Regression:** existing Gradle unit tests (`testDebugUnitTest`) and Rust tests (`cargo test` where feasible).

---

## 10. Files inspected (primary)

- `HANDOFF.md`
- `apps/cordn-coordinator/src/lib/coordinator.ts`
- `apps/cordn-coordinator/src/routes/+page.svelte` (grep/context)
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/src/{lib.rs,commands.rs,models.rs,mobile.rs,desktop.rs,android.rs}`
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/android/src/main/kotlin/org/cordn/coordinator/service/{CordnServicePlugin.kt,CordnService.kt,CoordinatorHealth.kt,RecoveryPolicy.kt}`
- `apps/cordn-coordinator/src-tauri/plugins/cordn-android-service/permissions/{default.toml,autogenerated/commands/publish_relay_list.toml}`
- `crates/cordn-server/src/{runtime.rs,announce.rs,config.rs}`
- `.github/workflows/release-android.yml`
- `scripts/publish-android-release.py`
- vendored SDK (`remotes/origin/vendor/contextvm-sdk`): `src/transport/server/{mod.rs,health.rs}` relevant sections

---

## 11. Recommended decision matrix

| If you want... | Pick |
|---|---|
| Fastest path to a usable beta for the reported bug | Already done on `beta/publish-relay-fix` / `android-v0.5.9-beta.1` |
| Stop “service dies until manual restart” after transient relay/transport failures | **R2 + R1**, with R6 |
| Confidence that one-relay-down is tolerated while others healthy | **R3 + R7** validated by integration test |
| Better publish-relay-list UX/diagnostics | **R4 + U1** |
| Minimal risk, minimal change | **R2 only** (backoff/reset budget) + R6 |

*End of report.*

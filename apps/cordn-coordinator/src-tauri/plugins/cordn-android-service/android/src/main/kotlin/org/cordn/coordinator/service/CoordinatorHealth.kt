package org.cordn.coordinator.service

import java.io.File
import org.json.JSONObject

internal object CoordinatorHealth {
    fun read(directory: File): JSONObject? = runCatching {
        JSONObject(File(directory, "cordn-health.json").readText())
    }.getOrNull()

    fun label(state: String, health: JSONObject?, status: JSONObject? = null): String {
        if (state == "starting") {
            val attempt = status?.optInt("recoveryAttempt", 0) ?: 0
            val nextRetryAt = status?.optLong("nextRetryAt", 0L) ?: 0L
            if (attempt > 0) {
                val remainingMs = (nextRetryAt - System.currentTimeMillis()).coerceAtLeast(0L)
                val mins = remainingMs / 60_000
                val secs = (remainingMs % 60_000) / 1000
                val eta = if (mins > 0) "${mins}m ${secs}s" else "${secs}s"
                return "Recovering (attempt $attempt) — retrying in $eta"
            }
            return "starting"
        }
        if (state != "running") return state
        if (health == null) return "Health not yet available"
        val age = System.currentTimeMillis() - health.optLong("updatedAt")
        if (age > 90_000 || age < -30_000) return "Status stale — check coordinator"
        if (health.optBoolean("criticalTaskFailed") || health.optLong("loopAgeMs") > 180_000)
            return "Degraded — transport not progressing"
        val connected = health.optInt("connectedRelays")
        val total = health.optInt("totalRelays")
        if (connected == 0) {
            return if (total > 0) "Waiting for relay connection (0/$total)" else "Waiting for relay connection"
        }
        val rejected = health.optInt("rejectedSubscriptions")
        if (rejected > 0) {
            return if (total > 0) "Degraded — subscription rejected ($rejected, $connected/$total connected)"
            else "Degraded — subscription rejected ($rejected)"
        }
        return if (total > 0) "Relay connected ($connected/$total) — delivery unverified"
        else "Relay connected — delivery unverified"
    }
}

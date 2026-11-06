package org.cordn.coordinator.service

import java.io.File
import org.json.JSONObject

internal object CoordinatorHealth {
    fun read(directory: File): JSONObject? = runCatching {
        JSONObject(File(directory, "cordn-health.json").readText())
    }.getOrNull()

    fun label(state: String, health: JSONObject?): String {
        if (state != "running") return state
        if (health == null) return "Health not yet available"
        val age = System.currentTimeMillis() - health.optLong("updatedAt")
        if (age > 90_000 || age < -30_000) return "Status stale — check coordinator"
        if (health.optBoolean("criticalTaskFailed") || health.optLong("loopAgeMs") > 180_000)
            return "Degraded — transport not progressing"
        if (health.optInt("connectedRelays") == 0) return "Waiting for relay connection"
        if (health.optInt("rejectedSubscriptions") > 0) return "Degraded — subscription rejected"
        return "Relay connected — delivery unverified"
    }
}

package org.cordn.coordinator.service

/** In-memory budget. No network, identifiers, wall-clock or persistent history. */
internal class RecoveryPolicy {
    var stopped = false
    private val attempts = java.util.ArrayDeque<Long>()
    fun allowRecovery(now: Long): Boolean {
        if (stopped) return false
        while (attempts.isNotEmpty() && now - attempts.peekFirst() > 3_600_000L) attempts.removeFirst()
        if (attempts.size >= 3) return false
        attempts.addLast(now)
        return true
    }
}

package org.cordn.coordinator.service

/**
 * In-memory recovery backoff. No network, identifiers, wall-clock or persistent history.
 *
 * R2: exponential backoff with a cap, no permanent give-up for transient transport errors.
 * The streak resets when the coordinator has been healthy long enough (see [reset]).
 */
internal class RecoveryPolicy {
    var stopped = false
    private var failureStreak: Int = 0

    /** Current consecutive recovery attempt count (0 when healthy/no streak). */
    fun attemptCount(): Int = failureStreak

    /**
     * Record a failure and return the delay before the next recovery attempt, or null when
     * recovery is suppressed (stopped). Never returns null just because of budget exhaustion.
     */
    fun scheduleRetry(nowElapsedMs: Long): Long? {
        if (stopped) return null
        // nowElapsedMs kept for API symmetry/future windowing; backoff is streak-based.
        failureStreak += 1
        return delayForAttempt(failureStreak)
    }

    /** Reset the failure streak after sustained healthy operation. */
    fun reset() {
        failureStreak = 0
    }

    companion object {
        // 60s, 2m, 5m, 10m, 15m, 30m, 60m cap
        internal val BACKOFF_MS = longArrayOf(
            60_000L,
            120_000L,
            300_000L,
            600_000L,
            900_000L,
            1_800_000L,
            3_600_000L,
        )

        internal fun delayForAttempt(attempt: Int): Long {
            if (attempt <= 0) return BACKOFF_MS.first()
            val idx = (attempt - 1).coerceAtMost(BACKOFF_MS.lastIndex)
            return BACKOFF_MS[idx]
        }
    }
}

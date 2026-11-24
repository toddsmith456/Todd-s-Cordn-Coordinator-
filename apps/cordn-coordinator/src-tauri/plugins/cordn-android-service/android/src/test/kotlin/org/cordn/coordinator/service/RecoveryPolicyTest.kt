package org.cordn.coordinator.service

import org.junit.Assert.*
import org.junit.Test

class RecoveryPolicyTest {
    @Test
    fun stopAlwaysWins() {
        val policy = RecoveryPolicy()
        policy.stopped = true
        assertNull(policy.scheduleRetry(0))
        assertNull(policy.scheduleRetry(9_000_000))
        policy.stopped = false
        assertEquals(60_000L, policy.scheduleRetry(9_000_001))
    }

    @Test
    fun exponentialBackoffCapsAndNeverGivesUp() {
        val policy = RecoveryPolicy()
        val expected = longArrayOf(60_000L, 120_000L, 300_000L, 600_000L, 900_000L, 1_800_000L, 3_600_000L)
        for ((i, delay) in expected.withIndex()) {
            assertEquals("attempt ${i + 1}", delay, policy.scheduleRetry((i * 60_000L)))
        }
        // capped: keeps retrying, never returns null
        repeat(5) { j ->
            assertEquals("capped attempt ${j}", 3_600_000L, policy.scheduleRetry(10_000_000L + j))
        }
        assertEquals(expected.size + 5, policy.attemptCount())
    }

    @Test
    fun resetClearsStreak() {
        val policy = RecoveryPolicy()
        assertEquals(60_000L, policy.scheduleRetry(0))
        assertEquals(120_000L, policy.scheduleRetry(60_000))
        policy.reset()
        assertEquals(0, policy.attemptCount())
        assertEquals(60_000L, policy.scheduleRetry(120_000))
    }
}

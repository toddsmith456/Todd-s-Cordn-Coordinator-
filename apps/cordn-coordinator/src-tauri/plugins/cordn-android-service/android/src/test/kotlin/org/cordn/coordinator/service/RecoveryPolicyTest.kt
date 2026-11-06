package org.cordn.coordinator.service
import org.junit.Assert.*
import org.junit.Test

class RecoveryPolicyTest {
    @Test fun stopAlwaysWins() {
        val policy = RecoveryPolicy()
        policy.stopped = true
        assertFalse(policy.allowRecovery(0))
        assertFalse(policy.allowRecovery(9_000_000))
        policy.stopped = false
        assertTrue(policy.allowRecovery(9_000_001))
    }
    @Test fun threeRetriesPerHourMaximum() {
        val policy = RecoveryPolicy()
        assertTrue(policy.allowRecovery(0))
        assertTrue(policy.allowRecovery(60_000))
        assertTrue(policy.allowRecovery(120_000))
        assertFalse(policy.allowRecovery(180_000))
        assertFalse(policy.allowRecovery(3_600_000))
        assertTrue(policy.allowRecovery(3_600_001))
    }
}

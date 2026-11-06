package org.cordn.coordinator.service
import org.junit.Assert.*
import org.junit.Test
import org.json.JSONObject

class CoordinatorHealthTest {
    private fun current() = JSONObject().put("updatedAt", System.currentTimeMillis()).put("connectedRelays", 1)
    @Test fun quietConnectedIsNeverDeliveryProof() {
        val label = CoordinatorHealth.label("running", current())
        assertEquals("Relay connected — delivery unverified", label)
    }
    @Test fun staleOfflineRejectedAndDeadAreDistinct() {
        assertTrue(CoordinatorHealth.label("running", current().put("updatedAt", 1)).startsWith("Status stale"))
        assertTrue(CoordinatorHealth.label("running", current().put("connectedRelays", 0)).startsWith("Waiting"))
        assertTrue(CoordinatorHealth.label("running", current().put("rejectedSubscriptions", 1)).startsWith("Degraded"))
        assertTrue(CoordinatorHealth.label("running", current().put("criticalTaskFailed", true)).startsWith("Degraded"))
        assertEquals("stopped", CoordinatorHealth.label("stopped", current()))
    }
}

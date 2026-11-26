package org.cordn.coordinator.service

internal class CordnNative {
    external fun runCoordinator(instanceId: String, configJson: String, privateKeyHex: String, dataDir: String): String?
    external fun prepareStart(instanceId: String)
    external fun stopCoordinator(instanceId: String)
    external fun networkChanged(instanceId: String)

    private companion object {
        init {
            System.loadLibrary("cordn_coordinator_app")
        }
    }
}

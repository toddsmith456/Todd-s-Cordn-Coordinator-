package org.cordn.coordinator.service

internal class CordnNative {
    external fun runCoordinator(configJson: String, privateKeyHex: String, dataDir: String): String?
    external fun stopCoordinator()

    private companion object {
        init {
            System.loadLibrary("cordn_coordinator_app")
        }
    }
}

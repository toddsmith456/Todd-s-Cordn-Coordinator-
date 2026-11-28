package org.cordn.coordinator.service

import android.util.AtomicFile
import java.io.File
import org.json.JSONObject

/** Local settings contain no identity, relay, or message data. */
internal object ReliabilitySettings {
    private fun settings(root: File, id: String) = AtomicFile(File(InstanceRegistry.instanceDir(root, id), "reliability.json"))
    private fun desired(root: File, id: String) = AtomicFile(File(InstanceRegistry.instanceDir(root, id), "desired-running.json"))

    fun read(root: File, id: String): JSONObject = try {
        settings(root, id).openRead().bufferedReader().use { JSONObject(it.readText()) }
    } catch (_: java.io.FileNotFoundException) {
        JSONObject().put("mode", "balanced").put("resumeAfterBoot", false)
    }
    fun save(root: File, id: String, mode: String, resume: Boolean) {
        require(mode in setOf("conservative", "balanced", "aggressive")) { "Invalid recovery mode" }
        write(settings(root, id), JSONObject().put("mode", mode).put("resumeAfterBoot", resume))
    }
    fun wanted(root: File, id: String): Boolean = try {
        desired(root, id).openRead().bufferedReader().use { JSONObject(it.readText()).getBoolean("running") }
    } catch (_: java.io.FileNotFoundException) { false }
    fun setWanted(root: File, id: String, value: Boolean) {
        write(desired(root, id), JSONObject().put("running", value))
    }
    private fun write(file: AtomicFile, value: JSONObject) {
        file.baseFile.parentFile?.mkdirs()
        val output = file.startWrite()
        try { output.write(value.toString().toByteArray()); file.finishWrite(output) }
        catch (e: Exception) { file.failWrite(output); throw e }
    }
}

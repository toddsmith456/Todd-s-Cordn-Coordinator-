package org.cordn.coordinator.service

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.content.Intent
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.RandomAccessFile
import org.json.JSONArray
import org.json.JSONObject

@InvokeArg class StartArgs { lateinit var configJson: String; var privateKeyHex: String? = null }
@InvokeArg class InstanceStartArgs { lateinit var instanceId: String; lateinit var configJson: String; var privateKeyHex: String? = null }
@InvokeArg class InstanceArgs { lateinit var instanceId: String }
@InvokeArg class CreateInstanceArgs { var name: String? = null }
@InvokeArg class RenameInstanceArgs { lateinit var instanceId: String; lateinit var name: String }
@InvokeArg class DeleteInstanceArgs { lateinit var instanceId: String }
@InvokeArg class ScheduleArgs { var operation: String = "get"; var enabled: Boolean = false; var times: Array<String> = arrayOf("03:00") }
@InvokeArg class InstanceScheduleArgs { lateinit var instanceId: String; lateinit var payload: JSObject }

@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "postNotifications")])
class CordnServicePlugin(private val activity: Activity) : Plugin(activity) {
    private fun root() = activity.noBackupFilesDir
    private fun ensure() = InstanceRegistry.ensure(root())
    private fun defaultId() = InstanceRegistry.defaultInstanceId(root())
    private fun dir(id: String) = InstanceRegistry.instanceDir(root(), id).also { it.mkdirs() }
    private fun validId(id: String): String {
        require(id.isNotBlank() && id.length <= 64 && id.all { it.isLetterOrDigit() || it == '-' || it == '_' }) { "invalid instance id" }
        require(InstanceRegistry.find(root(), id) != null) { "unknown instance: $id" }
        return id
    }

    @Command
    fun listInstances(invoke: Invoke) {
        try {
            ensure()
            val arr = InstanceRegistry.list(root())
            val out = JSONArray()
            for (o in arr) {
                val id = o.optString("id")
                val item = JSONObject()
                item.put("instance", o)
                item.put("status", statusJson(id))
                val cfgFile = File(dir(id), "cordn-config.json")
                item.put("config", if (cfgFile.exists()) JSONObject(cfgFile.readText()) else JSONObject.NULL)
                out.put(item)
            }
            val resp = JSObject()
            resp.put("instances", out)
            invoke.resolve(resp)
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command
    fun createInstance(invoke: Invoke) {
        try {
            ensure()
            val args = invoke.parseArgs(CreateInstanceArgs::class.java)
            val name = args.name?.trim().takeUnless { it.isNullOrEmpty() } ?: "Coordinator ${InstanceRegistry.list(root()).size + 1}"
            require(name.length <= 64) { "instance name is too long" }
            val obj = InstanceRegistry.add(root(), name)
            invoke.resolve(JSObject(obj.toString()))
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command
    fun renameInstance(invoke: Invoke) {
        try {
            ensure()
            val args = invoke.parseArgs(RenameInstanceArgs::class.java)
            val id = validId(args.instanceId)
            val name = args.name.trim()
            require(name.isNotEmpty() && name.length <= 64) { "instance name must be 1-64 characters" }
            invoke.resolve(JSObject(InstanceRegistry.rename(root(), id, name).toString()))
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command
    fun deleteInstance(invoke: Invoke) {
        try {
            ensure()
            val args = invoke.parseArgs(DeleteInstanceArgs::class.java)
            val id = validId(args.instanceId)
            if (isInstanceRunning(id)) return invoke.reject("Stop the coordinator before deleting it")
            InstanceRegistry.remove(root(), id)
            listInstances(invoke)
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command fun start(invoke: Invoke) { startInternal(invoke, defaultId(), null) }
    @Command fun startInstance(invoke: Invoke) {
        try { val args = invoke.parseArgs(InstanceStartArgs::class.java); startInternal(invoke, validId(args.instanceId), args) }
        catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }
    private fun startInternal(invoke: Invoke, instanceId: String, instanceArgs: InstanceStartArgs?) {
        try {
            ensure(); validId(instanceId)
            val configJson: String
            val privateKeyHex: String?
            if (instanceArgs != null) { configJson = instanceArgs.configJson; privateKeyHex = instanceArgs.privateKeyHex }
            else { val args = invoke.parseArgs(StartArgs::class.java); configJson = args.configJson; privateKeyHex = args.privateKeyHex }
            val intent = Intent(activity, CordnService::class.java)
                .putExtra(CordnService.EXTRA_CONFIG, configJson)
                .putExtra(CordnService.EXTRA_INSTANCE_ID, instanceId)
            privateKeyHex?.let { intent.putExtra(CordnService.EXTRA_PRIVATE_KEY, it) }
            ContextCompat.startForegroundService(activity, intent)
            invoke.resolve(JSObject(statusJson(instanceId, "starting").toString()))
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command fun stop(invoke: Invoke) { stopInternal(invoke, defaultId()) }
    @Command fun stopInstance(invoke: Invoke) { try { stopInternal(invoke, validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }
    private fun stopInternal(invoke: Invoke, instanceId: String) {
        activity.startService(Intent(activity, CordnService::class.java).setAction(CordnService.ACTION_STOP).putExtra(CordnService.EXTRA_INSTANCE_ID, instanceId))
        invoke.resolve(JSObject(statusJson(instanceId, "stopping").toString()))
    }

    @Command fun status(invoke: Invoke) { invoke.resolve(JSObject(statusJson(defaultId()).toString())) }
    @Command fun statusInstance(invoke: Invoke) { try { invoke.resolve(JSObject(statusJson(validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)).toString())) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }

    @Command fun config(invoke: Invoke) { configInternal(invoke, defaultId()) }
    @Command fun configInstance(invoke: Invoke) { try { configInternal(invoke, validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }
    private fun configInternal(invoke: Invoke, instanceId: String) {
        val response = JSObject()
        val file = File(dir(instanceId), "cordn-config.json")
        response.put("config", if (file.exists()) JSONObject(file.readText()) else JSONObject.NULL)
        invoke.resolve(response)
    }

    @Command fun diagnostics(invoke: Invoke) { diagnosticsInternal(invoke, defaultId()) }
    @Command fun diagnosticsInstance(invoke: Invoke) { try { diagnosticsInternal(invoke, validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }
    private fun diagnosticsInternal(invoke: Invoke, instanceId: String) {
        try {
            val response = JSObject()
            val d = dir(instanceId)
            val databaseFile = File(d, "cordn.sqlite")
            val statusFile = File(d, "cordn-status.json")
            val logFile = File(d, "cordn.log")
            val rootLog = File(root(), "cordn.log")
            response.put("logs", tail(if (logFile.exists()) logFile else rootLog))
            response.put("storageBytes", listOf(databaseFile, File("${databaseFile.path}-wal"), File("${databaseFile.path}-shm")).sumOf { it.length() })
            val running = statusFile.exists() && JSONObject(statusFile.readText()).optString("state") == "running"
            response.put("startedAt", if (running) CoordinatorHealth.read(d)?.optLong("startedAt") ?: JSONObject.NULL else JSONObject.NULL)
            val tables = mapOf("keyPackages" to "key_packages", "welcomes" to "welcomes", "joinRequests" to "join_requests", "groupMessages" to "group_messages")
            tables.keys.forEach { response.put(it, JSONObject.NULL) }
            if (databaseFile.exists()) {
                runCatching {
                    SQLiteDatabase.openDatabase(databaseFile.path, null, SQLiteDatabase.OPEN_READONLY).use { database ->
                        for ((key, table) in tables) {
                            database.rawQuery("SELECT COUNT(*) FROM $table", null).use { cursor -> if (cursor.moveToFirst()) response.put(key, cursor.getLong(0)) }
                        }
                    }
                }
            }
            invoke.resolve(response)
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command fun requestBatteryExemption(invoke: Invoke) {
        try {
            if (!batteryExempt()) activity.startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${activity.packageName}")))
            invoke.resolve(JSObject())
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command fun flushData(invoke: Invoke) { flushInternal(invoke, defaultId()) }
    @Command fun flushDataInstance(invoke: Invoke) { try { flushInternal(invoke, validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }
    private fun flushInternal(invoke: Invoke, instanceId: String) {
        if (isInstanceRunning(instanceId)) return invoke.reject("Stop the coordinator before erasing data")
        deleteDatabase(instanceId)
        invoke.resolve(JSObject(statusJson(instanceId).toString()))
    }

    @Command fun resetIdentity(invoke: Invoke) { resetInternal(invoke, defaultId()) }
    @Command fun resetIdentityInstance(invoke: Invoke) { try { resetInternal(invoke, validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }
    private fun resetInternal(invoke: Invoke, instanceId: String) {
        if (isInstanceRunning(instanceId)) return invoke.reject("Stop the coordinator before resetting the identity")
        IdentityStore(activity, dir(instanceId), IdentityStore.aliasFor(instanceId)).reset()
        deleteDatabase(instanceId)
        File(dir(instanceId), "cordn-status.json").delete()
        invoke.resolve(JSObject(statusJson(instanceId).toString()))
    }

    @Command fun publishRelayList(invoke: Invoke) { publishInternal(invoke, defaultId()) }
    @Command fun publishRelayListInstance(invoke: Invoke) { try { publishInternal(invoke, validId(invoke.parseArgs(InstanceArgs::class.java).instanceId)) } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) } }
    private fun publishInternal(invoke: Invoke, instanceId: String) {
        try {
            val privateKeyHex = IdentityStore(activity, dir(instanceId), IdentityStore.aliasFor(instanceId)).getOrCreate(null)
            val configFile = File(dir(instanceId), "cordn-config.json")
            val relayUrls = if (configFile.exists()) {
                val config = JSONObject(configFile.readText())
                val urls = config.optJSONArray("relayUrls")
                if (urls != null) (0 until urls.length()).map { urls.getString(it) } else listOf("wss://relay.contextvm.org", "wss://relay.nostr.net")
            } else listOf("wss://relay.contextvm.org", "wss://relay.nostr.net")
            if (relayUrls.isEmpty()) return invoke.reject("No relay URLs configured")
            val response = JSObject()
            response.put("privateKeyHex", privateKeyHex)
            val urlsArray = JSONArray(); relayUrls.forEach { urlsArray.put(it) }; response.put("relayUrls", urlsArray)
            invoke.resolve(response)
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    @Command fun restartSchedule(invoke: Invoke) { restartInternal(invoke, defaultId(), null) }
    @Command fun restartScheduleInstance(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(InstanceScheduleArgs::class.java)
            restartInternal(invoke, validId(args.instanceId), args.payload)
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }
    private fun restartInternal(invoke: Invoke, instanceId: String, payloadObj: JSObject?) {
        try {
            val op: String
            val enabled: Boolean
            val times: List<String>
            if (payloadObj != null) {
                op = payloadObj.optString("operation", "get")
                enabled = payloadObj.optBoolean("enabled", false)
                val arr = payloadObj.optJSONArray("times")
                times = if (arr != null) (0 until arr.length()).map { arr.getString(it) } else listOf("03:00")
            } else {
                val args = invoke.parseArgs(ScheduleArgs::class.java)
                op = args.operation
                enabled = args.enabled
                times = args.times.toList()
            }
            when (op) {
                "save" -> RestartSchedule.save(activity, instanceId, enabled, times)
                "permission" -> if (android.os.Build.VERSION.SDK_INT >= 31 && !RestartSchedule.allowed(activity)) {
                    activity.startActivity(Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:${activity.packageName}")))
                }
                "get" -> RestartSchedule.schedule(activity, instanceId)
                else -> throw IllegalArgumentException("Unknown schedule operation")
            }
            invoke.resolve(JSObject(RestartSchedule.response(activity, instanceId).toString()))
        } catch (e: Exception) { invoke.reject(e.message ?: e.toString()) }
    }

    private fun deleteDatabase(instanceId: String) {
        val database = File(dir(instanceId), "cordn.sqlite")
        listOf(database, File("${database.path}-wal"), File("${database.path}-shm")).forEach { it.delete() }
    }

    private fun statusJson(instanceId: String, stateOverride: String? = null): JSONObject {
        val file = File(dir(instanceId), "cordn-status.json")
        val stored = if (file.exists()) JSONObject(file.readText()) else JSONObject()
        var state = stateOverride ?: stored.optString("state", "stopped")
        if (stateOverride == null && state in setOf("starting", "running", "stopping") && !isInstanceRunning(instanceId)) state = "stopped"
        val response = JSONObject().put("state", state)
        for (key in listOf("publicKey", "lastError")) if (stored.has(key) && !stored.isNull(key)) response.put(key, stored.get(key))
        for (key in listOf("recoveryAttempt", "nextRetryAt")) if (stored.has(key) && !stored.isNull(key)) response.put(key, stored.get(key))
        val health = CoordinatorHealth.read(dir(instanceId))
        response.put("healthLabel", CoordinatorHealth.label(state, health, stored))
        if (state == "running" && health != null) response.put("health", health)
        response.put("batteryExempt", batteryExempt())
        return response
    }

    @Suppress("DEPRECATION")
    private fun serviceRunning(): Boolean =
        (activity.getSystemService(Activity.ACTIVITY_SERVICE) as ActivityManager)
            .getRunningServices(Int.MAX_VALUE)
            .any { it.service.className == CordnService::class.java.name }

    private fun isInstanceRunning(instanceId: String): Boolean {
        if (!serviceRunning()) return false
        val file = File(dir(instanceId), "cordn-status.json")
        val state = if (file.exists()) runCatching { JSONObject(file.readText()).optString("state") }.getOrDefault("stopped") else "stopped"
        return state in setOf("starting", "running", "stopping")
    }

    private fun batteryExempt(): Boolean =
        (activity.getSystemService(Activity.POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(activity.packageName)

    private fun tail(file: File): String {
        if (!file.exists()) return ""
        return RandomAccessFile(file, "r").use { input ->
            val start = (input.length() - MAX_LOG_BYTES).coerceAtLeast(0)
            input.seek(start)
            val bytes = ByteArray((input.length() - start).toInt())
            val read = input.read(bytes).coerceAtLeast(0)
            String(bytes, 0, read, Charsets.UTF_8).let { if (start == 0L) it else it.substringAfter('\n') }
        }
    }

    companion object { private const val MAX_LOG_BYTES = 64 * 1024L }
}

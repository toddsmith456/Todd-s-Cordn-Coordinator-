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
import org.json.JSONObject

@InvokeArg
class StartArgs {
    lateinit var configJson: String
    var privateKeyHex: String? = null
}

@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "postNotifications"),
    ],
)
class CordnServicePlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun start(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(StartArgs::class.java)
            val intent = Intent(activity, CordnService::class.java)
                .putExtra(CordnService.EXTRA_CONFIG, args.configJson)
            args.privateKeyHex?.let {
                intent.putExtra(CordnService.EXTRA_PRIVATE_KEY, it)
            }
            ContextCompat.startForegroundService(activity, intent)
            invoke.resolve(status("starting"))
        } catch (error: Exception) {
            invoke.reject(error.message ?: error.toString())
        }
    }

    @Command
    fun stop(invoke: Invoke) {
        activity.startService(
            Intent(activity, CordnService::class.java).setAction(CordnService.ACTION_STOP),
        )
        invoke.resolve(status("stopping"))
    }

    @Command
    fun status(invoke: Invoke) {
        invoke.resolve(status())
    }

    @Command
    fun config(invoke: Invoke) {
        val response = JSObject()
        val file = File(activity.noBackupFilesDir, "cordn-config.json")
        response.put("config", if (file.exists()) JSONObject(file.readText()) else JSONObject.NULL)
        invoke.resolve(response)
    }

    @Command
    fun diagnostics(invoke: Invoke) {
        try {
            val response = JSObject()
            val databaseFile = File(activity.noBackupFilesDir, "cordn.sqlite")
            val statusFile = File(activity.noBackupFilesDir, "cordn-status.json")
            response.put("logs", tail(File(activity.noBackupFilesDir, "cordn.log")))
            response.put(
                "storageBytes",
                listOf(databaseFile, File("${databaseFile.path}-wal"), File("${databaseFile.path}-shm"))
                    .sumOf { it.length() },
            )
            val running = statusFile.exists() &&
                JSONObject(statusFile.readText()).optString("state") == "running"
            response.put("startedAt", if (running) statusFile.lastModified() else JSONObject.NULL)

            val tables = mapOf(
                "keyPackages" to "key_packages",
                "welcomes" to "welcomes",
                "joinRequests" to "join_requests",
                "groupMessages" to "group_messages",
            )
            tables.keys.forEach { response.put(it, JSONObject.NULL) }
            if (databaseFile.exists()) {
                runCatching {
                    SQLiteDatabase.openDatabase(
                        databaseFile.path,
                        null,
                        SQLiteDatabase.OPEN_READONLY,
                    ).use { database ->
                        for ((key, table) in tables) {
                            database.rawQuery("SELECT COUNT(*) FROM $table", null).use { cursor ->
                                if (cursor.moveToFirst()) response.put(key, cursor.getLong(0))
                            }
                        }
                    }
                }
            }
            invoke.resolve(response)
        } catch (error: Exception) {
            invoke.reject(error.message ?: error.toString())
        }
    }

    @Command
    fun requestBatteryExemption(invoke: Invoke) {
        try {
            if (!batteryExempt()) {
                activity.startActivity(
                    Intent(
                        Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                        Uri.parse("package:${activity.packageName}"),
                    ),
                )
            }
            invoke.resolve(JSObject())
        } catch (error: Exception) {
            invoke.reject(error.message ?: error.toString())
        }
    }

    private fun status(stateOverride: String? = null): JSObject {
        val file = File(activity.noBackupFilesDir, "cordn-status.json")
        val stored = if (file.exists()) JSONObject(file.readText()) else JSONObject()
        val response = JSObject()
        var state = stateOverride ?: stored.optString("state", "stopped")
        if (stateOverride == null && state in setOf("starting", "running", "stopping") && !serviceRunning()) {
            state = "stopped"
        }
        response.put("state", state)
        for (key in listOf("publicKey", "nprofile", "lastError")) {
            if (stored.has(key) && !stored.isNull(key)) response.put(key, stored.get(key))
        }
        response.put("batteryExempt", batteryExempt())
        return response
    }

    // ponytail: Android still exposes this app's own services; replace with a Binder status call if it stops doing so.
    @Suppress("DEPRECATION")
    private fun serviceRunning(): Boolean =
        (activity.getSystemService(Activity.ACTIVITY_SERVICE) as ActivityManager)
            .getRunningServices(Int.MAX_VALUE)
            .any { it.service.className == CordnService::class.java.name }

    private fun batteryExempt(): Boolean =
        (activity.getSystemService(Activity.POWER_SERVICE) as PowerManager)
            .isIgnoringBatteryOptimizations(activity.packageName)

    private fun tail(file: File): String {
        if (!file.exists()) return ""
        return RandomAccessFile(file, "r").use { input ->
            val start = (input.length() - MAX_LOG_BYTES).coerceAtLeast(0)
            input.seek(start)
            val bytes = ByteArray((input.length() - start).toInt())
            val read = input.read(bytes).coerceAtLeast(0)
            String(bytes, 0, read, Charsets.UTF_8).let {
                if (start == 0L) it else it.substringAfter('\n')
            }
        }
    }

    companion object {
        private const val MAX_LOG_BYTES = 64 * 1024L
    }
}

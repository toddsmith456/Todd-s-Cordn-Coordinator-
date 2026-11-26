package org.cordn.coordinator.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.util.AtomicFile
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONArray
import org.json.JSONObject

internal class CordnService : Service() {
    private data class Worker(
        val instanceId: String,
        val running: AtomicBoolean = AtomicBoolean(false),
        val recoveryPolicy: RecoveryPolicy = RecoveryPolicy(),
        var recoveryPending: Boolean = false,
        var restartRequested: Boolean = false,
        var restartWakeLock: PowerManager.WakeLock? = null,
        var notificationText: String = "Starting coordinator…",
    )

    private val native = CordnNative()
    private val workers = ConcurrentHashMap<String, Worker>()
    private val handler = Handler(Looper.getMainLooper())
    private var networkCallback: android.net.ConnectivityManager.NetworkCallback? = null
    private var lastNetwork: android.net.Network? = null

    private val healthTick = object : Runnable {
        override fun run() {
            updateNotification()
            handler.postDelayed(this, 30_000L)
        }
    }

    override fun onCreate() {
        super.onCreate()
        InstanceRegistry.ensure(noBackupFilesDir)
        createNotificationChannel()
        handler.post(healthTick)
        val manager = getSystemService(CONNECTIVITY_SERVICE) as android.net.ConnectivityManager
        val callback = object : android.net.ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(network: android.net.Network, caps: android.net.NetworkCapabilities) {
                if (caps.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                    caps.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_VALIDATED)) {
                    handler.post {
                        if (lastNetwork != network) {
                            lastNetwork = network
                            for ((id, w) in workers) {
                                if (w.running.get() && !w.recoveryPolicy.stopped) native.networkChanged(id)
                            }
                        }
                    }
                }
            }
            override fun onLost(network: android.net.Network) {
                handler.post { if (lastNetwork == network) lastNetwork = null }
            }
        }
        runCatching { manager.registerDefaultNetworkCallback(callback); networkCallback = callback }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        InstanceRegistry.ensure(noBackupFilesDir)
        promoteToForeground()

        val instanceId = intent?.getStringExtra(EXTRA_INSTANCE_ID)?.takeIf { it.isNotBlank() }
            ?: InstanceRegistry.defaultInstanceId(noBackupFilesDir)

        if (intent?.action == ACTION_RESTART) {
            val w = workers[instanceId]
            runCatching { RestartSchedule.schedule(this, instanceId) }
            if (w != null && !w.recoveryPolicy.stopped && w.running.get() && !w.restartRequested && RestartSchedule.read(this, instanceId).optBoolean("enabled") && RestartSchedule.allowed(this)) {
                w.restartRequested = true
                w.restartWakeLock = (getSystemService(POWER_SERVICE) as PowerManager)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:scheduled-restart:$instanceId")
                    .apply { acquire(120_000L) }
                writeStatus(instanceId, "stopping")
                native.stopCoordinator(instanceId)
            } else if (w == null || !w.running.get()) {
                maybeStopService()
            }
            return if (anyRunning()) START_STICKY else START_NOT_STICKY
        }

        if (intent?.action == ACTION_STOP) {
            val targetIds = if (intent.hasExtra(EXTRA_INSTANCE_ID)) listOf(instanceId) else workers.keys().toList()
            for (id in targetIds) {
                val w = workers[id] ?: continue
                w.recoveryPolicy.stopped = true
                w.recoveryPending = false
                w.restartRequested = false
                if (w.running.get()) {
                    writeStatus(id, "stopping")
                    native.stopCoordinator(id)
                } else {
                    writeStatus(id, "stopped")
                }
            }
            if (!anyRunning()) {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return START_NOT_STICKY
            }
            updateNotification()
            return START_STICKY
        }

        val worker = workers.getOrPut(instanceId) { Worker(instanceId) }
        worker.recoveryPolicy.stopped = false
        worker.recoveryPending = false
        worker.recoveryPolicy.reset()
        startWorker(worker, intent)
        return START_STICKY
    }

    private fun startWorker(worker: Worker, intent: Intent?) {
        val instanceId = worker.instanceId
        if (!worker.running.compareAndSet(false, true)) return
        try {
            val instanceDir = InstanceRegistry.instanceDir(noBackupFilesDir, instanceId)
            instanceDir.mkdirs()
            val configJson = intent?.getStringExtra(EXTRA_CONFIG)?.also { persistConfig(instanceId, it) }
                ?: readConfig(instanceId)
            val privateKey = IdentityStore(this, instanceDir, IdentityStore.aliasFor(instanceId))
                .getOrCreate(intent?.getStringExtra(EXTRA_PRIVATE_KEY))
            writeStatus(instanceId, "starting")
            native.prepareStart(instanceId)
            Thread({
                val error = native.runCoordinator(instanceId, configJson, privateKey, instanceDir.absolutePath)
                Handler(Looper.getMainLooper()).post {
                    worker.running.set(false)
                    if (worker.restartRequested && !worker.recoveryPolicy.stopped) {
                        worker.restartRequested = false
                        startWorker(worker, null)
                        Handler(Looper.getMainLooper()).postDelayed({ releaseRestartWakeLock(worker) }, 30_000L)
                    } else if (error != null && !worker.recoveryPolicy.stopped) {
                        if (isFatalCoordinatorError(error)) {
                            writeStatus(instanceId, "error", error)
                            releaseRestartWakeLock(worker)
                            maybeStopService()
                        } else {
                            val delayMs = worker.recoveryPolicy.scheduleRetry(android.os.SystemClock.elapsedRealtime())
                            if (delayMs != null) {
                                worker.recoveryPending = true
                                val attempt = worker.recoveryPolicy.attemptCount()
                                val nextRetryAt = System.currentTimeMillis() + delayMs
                                writeStatus(instanceId, "starting", error = error, recoveryAttempt = attempt, nextRetryAt = nextRetryAt)
                                worker.notificationText = "Recovering — attempt $attempt, retrying in ${formatDelay(delayMs)}"
                                updateNotification()
                                releaseRestartWakeLock(worker)
                                handler.postDelayed({
                                    if (worker.recoveryPending && !worker.recoveryPolicy.stopped && !worker.running.get()) {
                                        worker.recoveryPending = false
                                        startWorker(worker, null)
                                    }
                                }, delayMs)
                            } else {
                                writeStatus(instanceId, "stopped")
                                releaseRestartWakeLock(worker)
                                maybeStopService()
                            }
                        }
                    } else {
                        if (error != null) writeStatus(instanceId, "error", error) else writeStatus(instanceId, "stopped")
                        releaseRestartWakeLock(worker)
                        maybeStopService()
                    }
                }
            }, "cordn-coordinator-$instanceId").start()
            scheduleHealthyReset(worker)
        } catch (error: Exception) {
            worker.running.set(false)
            releaseRestartWakeLock(worker)
            writeStatus(instanceId, "error", error.message ?: error.toString())
            maybeStopService()
        }
        updateNotification()
    }

    private fun scheduleHealthyReset(worker: Worker) {
        val runnable = Runnable {
            if (worker.running.get() && !worker.recoveryPending && !worker.recoveryPolicy.stopped) {
                worker.recoveryPolicy.reset()
            }
        }
        handler.postDelayed(runnable, HEALTHY_RESET_MS)
    }

    private fun anyRunning(): Boolean = workers.values.any { it.running.get() || it.recoveryPending }

    private fun maybeStopService() {
        updateNotification()
        if (!anyRunning()) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun updateNotification() {
        if (anyRunning()) promoteToForeground()
    }

    private fun formatDelay(delayMs: Long): String {
        val totalSeconds = (delayMs / 1000).coerceAtLeast(1)
        val minutes = totalSeconds / 60
        val seconds = totalSeconds % 60
        return if (minutes <= 0) "${seconds}s" else if (seconds == 0L) "${minutes}m" else "${minutes}m ${seconds}s"
    }

    private fun isFatalCoordinatorError(error: String): Boolean {
        val lower = error.lowercase()
        return listOf(
            "private key is not a valid nostr secret key",
            "private key must be 64 hex characters",
            "invalid private key",
            "server name is required",
            "at least one relay is required",
            "relay url must use ws:// or wss://",
            "invalid relay url",
        ).any { lower.contains(it) }
    }

    private fun releaseRestartWakeLock(worker: Worker) {
        worker.restartWakeLock?.let { if (it.isHeld) it.release() }
        worker.restartWakeLock = null
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        networkCallback?.let { callback ->
            runCatching { (getSystemService(CONNECTIVITY_SERVICE) as android.net.ConnectivityManager).unregisterNetworkCallback(callback) }
        }
        for ((id, w) in workers) {
            w.recoveryPolicy.stopped = true
            w.recoveryPending = false
            w.restartRequested = false
            releaseRestartWakeLock(w)
            if (w.running.get()) native.stopCoordinator(id)
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun promoteToForeground() {
        val openIntent = packageManager.getLaunchIntentForPackage(packageName)
        val openPendingIntent = openIntent?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        }
        val stopPendingIntent = PendingIntent.getService(
            this, 1, Intent(this, CordnService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val runningCount = workers.values.count { it.running.get() }
        val recoveringCount = workers.values.count { it.recoveryPending }
        val text = when {
            runningCount <= 0 && recoveringCount > 0 -> "Recovering $recoveringCount coordinator${if (recoveringCount == 1) "" else "s"}…"
            runningCount == 1 -> {
                val w = workers.values.firstOrNull { it.running.get() }
                w?.notificationText ?: "1 coordinator running"
            }
            runningCount > 1 -> "$runningCount coordinators running"
            else -> "Starting coordinator…"
        }
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_cordn)
            .setContentTitle("Todd's Coordinator")
            .setContentText(text)
            .setOnlyAlertOnce(true)
            .setContentIntent(openPendingIntent)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOngoing(true)
            .addAction(R.drawable.ic_stat_cordn, "Stop all", stopPendingIntent)
            .build()
        val type = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Cordn coordinator", NotificationManager.IMPORTANCE_LOW),
            )
        }
    }

    private fun persistConfig(instanceId: String, value: String) {
        val file = AtomicFile(InstanceRegistry.instanceDir(noBackupFilesDir, instanceId).resolve("cordn-config.json"))
        file.baseFile.parentFile?.mkdirs()
        val output = file.startWrite()
        try { output.write(value.toByteArray()); file.finishWrite(output) }
        catch (error: Exception) { file.failWrite(output); throw error }
    }

    private fun readConfig(instanceId: String): String =
        AtomicFile(InstanceRegistry.instanceDir(noBackupFilesDir, instanceId).resolve("cordn-config.json"))
            .openRead().bufferedReader().use { it.readText() }

    private fun writeStatus(instanceId: String, state: String, error: String? = null, recoveryAttempt: Int = 0, nextRetryAt: Long? = null) {
        val instanceDir = InstanceRegistry.instanceDir(noBackupFilesDir, instanceId)
        instanceDir.mkdirs()
        val statusFile = File(instanceDir, "cordn-status.json")
        val previous = if (statusFile.exists()) runCatching { JSONObject(statusFile.readText()) }.getOrNull() else null
        val status = JSONObject().put("state", state)
        if (previous != null && previous.has("publicKey") && !previous.isNull("publicKey")) status.put("publicKey", previous.optString("publicKey"))
        val lastError = when {
            error != null -> error
            state == "stopped" -> null
            previous != null && previous.has("lastError") && !previous.isNull("lastError") -> previous.optString("lastError")
            else -> null
        }
        if (lastError != null) status.put("lastError", lastError)
        if (recoveryAttempt > 0) status.put("recoveryAttempt", recoveryAttempt)
        if (nextRetryAt != null) status.put("nextRetryAt", nextRetryAt)
        val file = AtomicFile(statusFile)
        val output = file.startWrite()
        try { output.write(status.toString().toByteArray()); file.finishWrite(output) }
        catch (_: Exception) { file.failWrite(output) }
    }

    companion object {
        const val ACTION_RESTART = "com.toddsmith456.cordn.coordinator.SCHEDULED_RESTART"
        const val ACTION_STOP = "org.cordn.coordinator.STOP"
        const val EXTRA_CONFIG = "config"
        const val EXTRA_PRIVATE_KEY = "privateKey"
        const val EXTRA_INSTANCE_ID = "instanceId"
        private const val CHANNEL_ID = "cordn-coordinator"
        private const val NOTIFICATION_ID = 4101
        private const val HEALTHY_RESET_MS = 300_000L
    }
}

internal object InstanceRegistry {
    private const val FILE = "instances.json"
    const val DEFAULT_ID = "default"
    const val DEFAULT_NAME = "Coordinator 1"
    const val MAX = 5

    fun instancesFile(root: File): File = File(root, FILE)
    fun instancesDir(root: File): File = File(root, "instances")
    fun instanceDir(root: File, id: String): File = File(instancesDir(root), id)

    fun ensure(root: File): JSONArray {
        val file = instancesFile(root)
        val existing = if (file.exists()) runCatching { JSONArray(file.readText()) }.getOrNull() else null
        if (existing != null && existing.length() > 0) return existing

        val hasLegacy = listOf("cordn-config.json","cordn-identity.json","cordn-status.json","cordn.sqlite","cordn.log").any { File(root, it).exists() }
        val id = DEFAULT_ID
        val dir = instanceDir(root, id)
        dir.mkdirs()
        if (hasLegacy) {
            for (name in listOf("cordn-config.json","cordn-identity.json","cordn-status.json","cordn-health.json","cordn.log","cordn-restart-schedule.json","cordn.sqlite","cordn.sqlite-wal","cordn.sqlite-shm")) {
                val src = File(root, name)
                if (src.exists()) {
                    val dst = File(dir, name)
                    if (!dst.exists()) {
                        runCatching { src.copyTo(dst, overwrite = false); src.delete() }
                    }
                }
            }
        }
        val arr = JSONArray().put(JSONObject().put("id", id).put("name", DEFAULT_NAME).put("createdAt", System.currentTimeMillis()))
        write(root, arr)
        return arr
    }

    fun list(root: File): List<JSONObject> {
        val arr = ensure(root)
        return (0 until arr.length()).mapNotNull { arr.optJSONObject(it) }
    }

    fun write(root: File, arr: JSONArray) {
        val file = AtomicFile(instancesFile(root))
        val out = file.startWrite()
        try { out.write(arr.toString().toByteArray()); file.finishWrite(out) } catch (e: Exception) { file.failWrite(out); throw e }
    }

    fun add(root: File, name: String): JSONObject {
        val arr = ensure(root)
        if (arr.length() >= MAX) throw IllegalStateException("Maximum of $MAX coordinators allowed")
        var id = "c${System.currentTimeMillis()}"
        var suffix = 0
        val existingIds = (0 until arr.length()).mapNotNull { arr.optJSONObject(it)?.optString("id") }.toSet()
        while (id in existingIds) { suffix++; id = "c${System.currentTimeMillis()}-$suffix" }
        val obj = JSONObject().put("id", id).put("name", name).put("createdAt", System.currentTimeMillis())
        arr.put(obj)
        instanceDir(root, id).mkdirs()
        write(root, arr)
        return obj
    }

    fun rename(root: File, instanceId: String, name: String): JSONObject {
        val arr = ensure(root)
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            if (o.optString("id") == instanceId) { o.put("name", name); write(root, arr); return o }
        }
        throw IllegalArgumentException("unknown instance: $instanceId")
    }

    fun remove(root: File, instanceId: String): JSONArray {
        val arr = ensure(root)
        if (arr.length() <= 1) throw IllegalStateException("At least one coordinator is required")
        val next = JSONArray()
        var found = false
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            if (o.optString("id") == instanceId) { found = true } else next.put(o)
        }
        if (!found) throw IllegalArgumentException("unknown instance: $instanceId")
        write(root, next)
        val dir = instanceDir(root, instanceId)
        if (dir.exists() && dir.path.startsWith(instancesDir(root).path)) dir.deleteRecursively()
        return next
    }

    fun defaultInstanceId(root: File): String {
        val list = list(root)
        return list.firstOrNull { it.optString("id") == DEFAULT_ID }?.optString("id") ?: list.firstOrNull()?.optString("id") ?: DEFAULT_ID
    }

    fun find(root: File, instanceId: String): JSONObject? = list(root).firstOrNull { it.optString("id") == instanceId }
}

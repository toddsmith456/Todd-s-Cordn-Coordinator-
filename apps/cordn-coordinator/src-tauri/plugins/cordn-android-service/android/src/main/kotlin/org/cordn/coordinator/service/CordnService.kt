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
import android.util.AtomicFile
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONObject

internal class CordnService : Service() {
    private val native = CordnNative()
    private val workerRunning = AtomicBoolean(false)

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        promoteToForeground()

        if (intent?.action == ACTION_STOP) {
            if (workerRunning.get()) {
                writeStatus("stopping")
                native.stopCoordinator()
            } else {
                writeStatus("stopped")
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
            return START_NOT_STICKY
        }

        if (!workerRunning.compareAndSet(false, true)) return START_STICKY

        try {
            val configJson = intent?.getStringExtra(EXTRA_CONFIG)?.also(::persistConfig)
                ?: readConfig()
            val privateKey = IdentityStore(this).getOrCreate(intent?.getStringExtra(EXTRA_PRIVATE_KEY))
            writeStatus("starting")
            Thread(
                {
                    val error = native.runCoordinator(
                        configJson,
                        privateKey,
                        noBackupFilesDir.absolutePath,
                    )
                    workerRunning.set(false)
                    if (error != null) writeStatus("error", error) else writeStatus("stopped")
                    Handler(Looper.getMainLooper()).post {
                        stopForeground(STOP_FOREGROUND_REMOVE)
                        stopSelf()
                    }
                },
                "cordn-coordinator",
            ).start()
        } catch (error: Exception) {
            workerRunning.set(false)
            writeStatus("error", error.message ?: error.toString())
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }

        return START_STICKY
    }

    override fun onDestroy() {
        if (workerRunning.get()) native.stopCoordinator()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun promoteToForeground() {
        val openIntent = packageManager.getLaunchIntentForPackage(packageName)
        val openPendingIntent = openIntent?.let {
            PendingIntent.getActivity(
                this,
                0,
                it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
        val stopPendingIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, CordnService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_cordn)
            .setContentTitle("Cordn coordinator is running")
            .setContentText("Connected through the configured Nostr relays")
            .setContentIntent(openPendingIntent)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOngoing(true)
            .addAction(R.drawable.ic_stat_cordn, "Stop", stopPendingIntent)
            .build()
        val type = if (Build.VERSION.SDK_INT >= 34) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
        } else {
            0
        }
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    "Cordn coordinator",
                    NotificationManager.IMPORTANCE_LOW,
                ),
            )
        }
    }

    private fun persistConfig(value: String) {
        val file = AtomicFile(configFile())
        val output = file.startWrite()
        try {
            output.write(value.toByteArray())
            file.finishWrite(output)
        } catch (error: Exception) {
            file.failWrite(output)
            throw error
        }
    }

    private fun readConfig(): String = AtomicFile(configFile()).openRead().bufferedReader().use {
        it.readText()
    }

    private fun configFile() = File(noBackupFilesDir, "cordn-config.json")

    private fun writeStatus(state: String, error: String? = null) {
        val status = JSONObject().put("state", state)
        if (error != null) status.put("lastError", error)
        val file = AtomicFile(File(noBackupFilesDir, "cordn-status.json"))
        val output = file.startWrite()
        try {
            output.write(status.toString().toByteArray())
            file.finishWrite(output)
        } catch (_: Exception) {
            file.failWrite(output)
        }
    }

    companion object {
        const val ACTION_STOP = "org.cordn.coordinator.STOP"
        const val EXTRA_CONFIG = "config"
        const val EXTRA_PRIVATE_KEY = "privateKey"
        private const val CHANNEL_ID = "cordn-coordinator"
        private const val NOTIFICATION_ID = 4101
    }
}

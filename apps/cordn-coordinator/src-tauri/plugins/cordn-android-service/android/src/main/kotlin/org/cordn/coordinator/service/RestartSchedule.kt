package org.cordn.coordinator.service

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat
import android.os.Build
import android.util.AtomicFile
import java.io.File
import java.util.Calendar
import org.json.JSONArray
import org.json.JSONObject

internal object RestartSchedule {
    private fun file(context: Context, instanceId: String): AtomicFile =
        AtomicFile(File(InstanceRegistry.instanceDir(context.noBackupFilesDir, instanceId), "cordn-restart-schedule.json"))

    // legacy compat
    fun read(context: Context): JSONObject = read(context, InstanceRegistry.defaultInstanceId(context.noBackupFilesDir))

    fun read(context: Context, instanceId: String): JSONObject = runCatching {
        file(context, instanceId).openRead().bufferedReader().use { JSONObject(it.readText()) }
    }.getOrElse { JSONObject().put("enabled", false).put("times", JSONArray().put("03:00")) }

    fun allowed(context: Context): Boolean = Build.VERSION.SDK_INT < 31 ||
        context.getSystemService(AlarmManager::class.java).canScheduleExactAlarms()

    fun save(context: Context, enabled: Boolean, times: List<String>) {
        save(context, InstanceRegistry.defaultInstanceId(context.noBackupFilesDir), enabled, times)
    }

    fun save(context: Context, instanceId: String, enabled: Boolean, times: List<String>, hourly: Boolean = false) {
        require(times.size in 1..2) { "Choose one or two restart times" }
        require(times.all { it.matches(Regex("([01][0-9]|2[0-3]):[0-5][0-9]")) }) { "Invalid restart time" }
        require(times.distinct().size == times.size) { "Restart times must be different" }
        require(!enabled || allowed(context)) { "Allow Alarms & reminders first, then save the schedule" }
        if (times.size == 2) {
            val minutes = times.map { it.substring(0, 2).toInt() * 60 + it.substring(3).toInt() }
            val gap = kotlin.math.abs(minutes[0] - minutes[1])
            require(minOf(gap, 1440 - gap) >= 15) { "Keep restarts at least 15 minutes apart" }
        }
        val data = JSONObject().put("enabled", enabled).put("times", JSONArray(times)).put("hourly", hourly)
        val f = file(context, instanceId)
        f.baseFile.parentFile?.mkdirs()
        val output = f.startWrite()
        try { output.write(data.toString().toByteArray()); f.finishWrite(output) }
        catch (error: Exception) { f.failWrite(output); throw error }
        schedule(context, instanceId)
    }

    fun nextTime(time: String, now: Long = System.currentTimeMillis()): Long {
        val parts = time.split(":")
        val date = Calendar.getInstance().apply {
            timeInMillis = now
            set(Calendar.HOUR_OF_DAY, parts[0].toInt())
            set(Calendar.MINUTE, parts[1].toInt())
            set(Calendar.SECOND, 0)
            set(Calendar.MILLISECOND, 0)
            if (timeInMillis <= now) add(Calendar.DAY_OF_YEAR, 1)
        }
        return date.timeInMillis
    }

    fun nextHour(now: Long = System.currentTimeMillis()): Long = Calendar.getInstance().run {
        timeInMillis = now
        set(Calendar.MINUTE, 0); set(Calendar.SECOND, 0); set(Calendar.MILLISECOND, 0)
        add(Calendar.HOUR_OF_DAY, 1)
        timeInMillis
    }

    private fun requestCode(instanceId: String, index: Int): Int {
        val base = (instanceId.hashCode() and 0x7fffffff) % 100000
        return 4200 + base * 2 + index
    }

    private fun pending(context: Context, instanceId: String, index: Int): PendingIntent {
        val intent = Intent(context, CordnService::class.java)
            .setAction(CordnService.ACTION_RESTART)
            .putExtra(CordnService.EXTRA_INSTANCE_ID, instanceId)
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        val rc = requestCode(instanceId, index)
        return if (Build.VERSION.SDK_INT >= 26) PendingIntent.getForegroundService(context, rc, intent, flags)
        else PendingIntent.getService(context, rc, intent, flags)
    }

    fun schedule(context: Context) {
        InstanceRegistry.list(context.noBackupFilesDir).forEach { o ->
            schedule(context, o.optString("id"))
        }
    }

    fun schedule(context: Context, instanceId: String) {
        val manager = context.getSystemService(AlarmManager::class.java)
        for (index in 0..1) manager.cancel(pending(context, instanceId, index))
        val data = read(context, instanceId)
        if (!data.optBoolean("enabled") || !allowed(context)) return
        val times = data.getJSONArray("times")
        if (data.optBoolean("hourly", false)) {
            manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, nextHour(), pending(context, instanceId, 0))
        } else for (index in 0 until times.length()) {
            manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, nextTime(times.getString(index)), pending(context, instanceId, index))
        }
    }

    fun response(context: Context): JSONObject = response(context, InstanceRegistry.defaultInstanceId(context.noBackupFilesDir))

    fun response(context: Context, instanceId: String): JSONObject {
        val data = read(context, instanceId)
        data.put("exactAlarmAllowed", allowed(context))
        val times = data.getJSONArray("times")
        val next = if (data.optBoolean("enabled") && allowed(context))
            if (data.optBoolean("hourly", false)) nextHour() else (0 until times.length()).minOf { nextTime(times.getString(it)) } else null
        val policy = ReliabilitySettings.read(context.noBackupFilesDir, instanceId)
        data.put("recoveryMode", policy.optString("mode", "balanced"))
        data.put("resumeAfterBoot", policy.optBoolean("resumeAfterBoot", false))
        return data.put("nextAlarm", next ?: JSONObject.NULL)
    }
}

class RestartScheduleReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        runCatching { RestartSchedule.schedule(context) }
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) {
            val shouldResume = runCatching {
                InstanceRegistry.list(context.noBackupFilesDir).any { item ->
                    val id = item.getString("id")
                    ReliabilitySettings.read(context.noBackupFilesDir, id).optBoolean("resumeAfterBoot", false) &&
                        ReliabilitySettings.wanted(context.noBackupFilesDir, id)
                }
            }.getOrDefault(false)
            if (shouldResume) runCatching {
                ContextCompat.startForegroundService(context, Intent(context, CordnService::class.java).setAction(CordnService.ACTION_BOOT_RESUME))
            }
        }
    }
}

package org.cordn.coordinator.service

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.AtomicFile
import java.io.File
import java.util.Calendar
import org.json.JSONArray
import org.json.JSONObject

internal object RestartSchedule {
    private fun file(context: Context) = AtomicFile(File(context.noBackupFilesDir, "cordn-restart-schedule.json"))
    fun read(context: Context): JSONObject = runCatching {
        file(context).openRead().bufferedReader().use { JSONObject(it.readText()) }
    }.getOrElse { JSONObject().put("enabled", false).put("times", JSONArray().put("03:00")) }

    fun allowed(context: Context): Boolean = Build.VERSION.SDK_INT < 31 ||
        context.getSystemService(AlarmManager::class.java).canScheduleExactAlarms()

    fun save(context: Context, enabled: Boolean, times: List<String>) {
        require(times.size in 1..2) { "Choose one or two restart times" }
        require(times.all { it.matches(Regex("([01][0-9]|2[0-3]):[0-5][0-9]")) }) { "Invalid restart time" }
        require(times.distinct().size == times.size) { "Restart times must be different" }
        require(!enabled || allowed(context)) { "Allow Alarms & reminders first, then save the schedule" }
        if (times.size == 2) {
            val minutes = times.map { it.substring(0, 2).toInt() * 60 + it.substring(3).toInt() }
            val gap = kotlin.math.abs(minutes[0] - minutes[1])
            require(minOf(gap, 1440 - gap) >= 15) { "Keep restarts at least 15 minutes apart" }
        }
        val data = JSONObject().put("enabled", enabled).put("times", JSONArray(times))
        val f = file(context)
        val output = f.startWrite()
        try {
            output.write(data.toString().toByteArray())
            f.finishWrite(output)
        } catch (error: Exception) {
            f.failWrite(output)
            throw error
        }
        schedule(context)
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

    private fun pending(context: Context, index: Int): PendingIntent {
        val intent = Intent(context, CordnService::class.java).setAction(CordnService.ACTION_RESTART)
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        return if (Build.VERSION.SDK_INT >= 26) PendingIntent.getForegroundService(context, 4200 + index, intent, flags)
        else PendingIntent.getService(context, 4200 + index, intent, flags)
    }

    fun schedule(context: Context) {
        val manager = context.getSystemService(AlarmManager::class.java)
        for (index in 0..1) manager.cancel(pending(context, index))
        val data = read(context)
        if (!data.optBoolean("enabled") || !allowed(context)) return
        val times = data.getJSONArray("times")
        for (index in 0 until times.length()) {
            manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, nextTime(times.getString(index)), pending(context, index))
        }
    }

    fun response(context: Context): JSONObject {
        val data = read(context)
        data.put("exactAlarmAllowed", allowed(context))
        val times = data.getJSONArray("times")
        val next = if (data.optBoolean("enabled") && allowed(context))
            (0 until times.length()).minOf { nextTime(times.getString(it)) } else null
        return data.put("nextAlarm", next ?: JSONObject.NULL)
    }
}

class RestartScheduleReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        // Restore only alarms, never start a manually stopped coordinator.
        runCatching { RestartSchedule.schedule(context) }
    }
}

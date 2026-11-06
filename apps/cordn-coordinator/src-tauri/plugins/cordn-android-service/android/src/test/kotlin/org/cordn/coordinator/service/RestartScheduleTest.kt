package org.cordn.coordinator.service

import org.junit.Assert.*
import org.junit.Test
import java.util.Calendar
import java.util.TimeZone

class RestartScheduleTest {
    private fun at(year: Int, month: Int, day: Int, hour: Int, minute: Int): Long =
        Calendar.getInstance().apply {
            clear()
            set(year, month - 1, day, hour, minute)
        }.timeInMillis

    @Test fun sameDayAndNextDay() {
        val previous = TimeZone.getDefault()
        try {
            TimeZone.setDefault(TimeZone.getTimeZone("America/Chicago"))
            assertEquals(at(2026, 11, 5, 15, 0), RestartSchedule.nextTime("15:00", at(2026, 11, 5, 3, 0)))
            assertEquals(at(2026, 11, 6, 3, 0), RestartSchedule.nextTime("03:00", at(2026, 11, 5, 3, 0)))
            assertEquals(at(2026, 11, 6, 0, 0), RestartSchedule.nextTime("00:00", at(2026, 11, 5, 23, 59)))
        } finally { TimeZone.setDefault(previous) }
    }

    @Test fun calendarDaysFollowDaylightSaving() {
        val previous = TimeZone.getDefault()
        try {
            TimeZone.setDefault(TimeZone.getTimeZone("America/Chicago"))
            val before = at(2026, 3, 7, 15, 0)
            val next = RestartSchedule.nextTime("15:00", before)
            assertEquals(at(2026, 3, 8, 15, 0), next)
            assertEquals(23L * 60 * 60 * 1000, next - before)
            val fall = at(2026, 10, 31, 15, 0)
            assertEquals(25L * 60 * 60 * 1000, RestartSchedule.nextTime("15:00", fall) - fall)
        } finally { TimeZone.setDefault(previous) }
    }
}

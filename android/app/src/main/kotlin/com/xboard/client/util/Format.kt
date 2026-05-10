package com.xboard.client.util

import java.text.DateFormat
import java.util.Date
import java.util.Locale

private val SIZE_UNITS = arrayOf("B", "KB", "MB", "GB", "TB", "PB")
private val DATE_FMT_LAZY: ThreadLocal<DateFormat> = object : ThreadLocal<DateFormat>() {
    override fun initialValue(): DateFormat =
        DateFormat.getDateInstance(DateFormat.MEDIUM, Locale.getDefault())
}
private val DATETIME_FMT_LAZY: ThreadLocal<DateFormat> = object : ThreadLocal<DateFormat>() {
    override fun initialValue(): DateFormat =
        DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, Locale.getDefault())
}

/**
 * Render a byte count as a human string. Mirrors the desktop's
 * `formatBytes` helper for cross-platform consistency.
 *
 * Sub-KB values stay as bytes to avoid "0.00 KB"; everything else is
 * 2 decimals + unit. Negative inputs get clamped to 0 since they
 * indicate bookkeeping errors upstream and would render as gibberish.
 */
fun formatBytes(bytes: ULong): String {
    if (bytes == 0UL) return "0 B"
    var size = bytes.toDouble()
    var unitIndex = 0
    while (size >= 1024 && unitIndex < SIZE_UNITS.size - 1) {
        size /= 1024
        unitIndex += 1
    }
    return if (unitIndex == 0) {
        "${size.toLong()} ${SIZE_UNITS[0]}"
    } else {
        String.format(Locale.US, "%.2f %s", size, SIZE_UNITS[unitIndex])
    }
}

fun formatBytes(bytes: Long): String =
    if (bytes < 0) "0 B" else formatBytes(bytes.toULong())

/** Convert cents → "¥X.YZ". Mirrors the desktop's currency display. */
fun formatYuan(cents: Long): String = String.format(Locale.US, "%.2f", cents / 100.0)

/** Unix seconds → localized "MMM d, yyyy". Returns empty string on null. */
fun formatDate(unixSec: Long?): String =
    unixSec?.let { DATE_FMT_LAZY.get()!!.format(Date(it * 1_000)) } ?: ""

/** Unix seconds → localized "MMM d, yyyy h:mm a". Returns empty string on null. */
fun formatDateTime(unixSec: Long?): String =
    unixSec?.let { DATETIME_FMT_LAZY.get()!!.format(Date(it * 1_000)) } ?: ""

/** GB transfer cap → "X GB". The plan's `transferEnable` is already in
 *  GB (per the dictionary contract), but Subscribe's `transferEnable`
 *  is in bytes — different units, different overloads. */
fun formatTransferGb(gb: ULong): String = "$gb GB"

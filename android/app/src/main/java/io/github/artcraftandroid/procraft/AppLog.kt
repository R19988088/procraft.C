package io.github.artcraftandroid.procraft

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import android.util.Log
import androidx.annotation.RequiresApi
import io.github.artcraftandroid.procraft.core.LogLevel
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream

/**
 * The app's log, kept in a file as well so that it can be shared (the core keeps it:
 * procraft.log in the app's folder, the run before's beside it): a header about the app and the
 * device, how the app's last runs ended, the core's log, the interface's warnings and errors,
 * which go through [w] and [e], and the stack of an exception nothing caught. Help's Share log
 * hands it on ([toShare]).
 */
object AppLog {
    private const val TAG = "ProCraft"

    /** How many of the last runs' ends are told. */
    private const val EXITS = 5

    /** The most of a trace that is read, bytes (the core tells at most 48 KB of it). */
    private const val TRACE_READ_MOST = 4 shl 20

    /** Where the newest end told is kept (its time), so that each trace is told once. */
    private const val PREFS = "log"
    private const val EXIT_TOLD = "exitTold"

    /** Whether the file has begun, so lines can go into it. */
    @Volatile
    private var kept = false

    /**
     * Begins the log's file with its header; once a process. Called by the app's own screen, not
     * on every start of the process (a widget's), so that a run that crashed stays the previous log
     * until the app is opened again.
     */
    @Synchronized
    fun start(context: Context) {
        if (kept) return
        val app = context.applicationContext
        runCatching { io.github.artcraftandroid.procraft.core.startLogFile(app.filesDir.path, header(app)) }
            .onSuccess { kept = true }
            .onFailure { Log.e(TAG, "the log's file", it) }
        if (!kept) return
        catchCrashes()
        // off the main thread: a trace is read and made into text
        kotlin.concurrent.thread(name = "last runs") { lastRuns(app) }
    }

    /** An exception nothing caught: its stack into the file, before the system's handler ends the app. */
    private fun catchCrashes() {
        val before = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching { toFile(LogLevel.ERROR, "crash", "uncaught on thread \"${thread.name}\"", error) }
            before?.uncaughtException(thread, error)
        }
    }

    /**
     * The log to share, as one text file among the files the app hands to others (see the core's
     * write_log_to_share); none if it couldn't be written.
     */
    fun toShare(context: Context): File? {
        val file = File(File(context.cacheDir, "exports").apply { mkdirs() }, "procraft-log.txt")
        val written = runCatching { io.github.artcraftandroid.procraft.core.writeLogToShare(context.filesDir.path, file.path) }
        return file.takeIf { written.getOrDefault(false) }
    }

    /** A warning, to the system's log and the file. */
    fun w(tag: String, message: String, error: Throwable? = null) {
        Log.w(tag, message, error)
        toFile(LogLevel.WARN, tag, message, error)
    }

    /** An error, to the system's log and the file. */
    fun e(tag: String, message: String, error: Throwable? = null) {
        Log.e(tag, message, error)
        toFile(LogLevel.ERROR, tag, message, error)
    }

    /** A line into the file only (its stack after it, for an error). */
    private fun toFile(level: LogLevel, tag: String, message: String, error: Throwable? = null) {
        if (!kept) return
        val text = if (error == null) message else "$message\n${error.stackTraceToString().trimEnd()}"
        runCatching { io.github.artcraftandroid.procraft.core.logLine(level, tag, text) }
    }

    /**
     * How the app's last runs ended, as the system keeps it (Android 11 on), newest first: why, when,
     * how much the app mattered then and the memory it held; with the system's trace (a crash in
     * native code, the app not responding) of each not told in an earlier log.
     */
    private fun lastRuns(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            toFile(LogLevel.INFO, "last runs", "how the last runs ended is kept from Android 11 on")
            return
        }
        val exits = runCatching {
            context.getSystemService(ActivityManager::class.java)?.getHistoricalProcessExitReasons(context.packageName, 0, EXITS).orEmpty()
        }.getOrElse {
            toFile(LogLevel.WARN, "last runs", "how the last runs ended couldn't be read", it)
            return
        }
        if (exits.isEmpty()) {
            toFile(LogLevel.INFO, "last runs", "no earlier run is known")
            return
        }
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val told = prefs.getLong(EXIT_TOLD, 0L)
        toFile(LogLevel.INFO, "last runs", "how the last ${exits.size} runs ended, the newest first:")
        for (exit in exits) {
            val trace = if (exit.timestamp > told) trace(exit) else null
            toFile(LogLevel.INFO, "last runs", exitLine(exit) + (trace?.let { "\n" + it.trimEnd().prependIndent("    ") } ?: ""))
        }
        prefs.edit().putLong(EXIT_TOLD, exits.maxOf { it.timestamp }).apply()
    }

    @RequiresApi(Build.VERSION_CODES.R)
    private fun exitLine(exit: ApplicationExitInfo): String = buildString {
        append("${java.time.Instant.ofEpochMilli(exit.timestamp)}: ${reasonWords(exit.reason)} (reason ${exit.reason}, status ${exit.status}), ")
        append("${importanceWords(exit.importance)} (importance ${exit.importance}), ")
        append("holding ${exit.pss shr 10} MB (${exit.rss shr 10} MB resident)")
        exit.description?.takeIf { it.isNotBlank() }?.let { append(": $it") }
    }

    /** The system's trace of how a run ended, as text (see the core's trace_text); none if it kept none. */
    @RequiresApi(Build.VERSION_CODES.R)
    private fun trace(exit: ApplicationExitInfo): String? = runCatching {
        exit.traceInputStream?.use { readAtMost(it, TRACE_READ_MOST) }?.let { io.github.artcraftandroid.procraft.core.traceText(it) }
    }.getOrNull()

    private fun readAtMost(input: InputStream, most: Int): ByteArray {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(64 shl 10)
        while (out.size() < most) {
            val n = input.read(buffer, 0, minOf(buffer.size, most - out.size()))
            if (n < 0) break
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }

    @RequiresApi(Build.VERSION_CODES.R)
    private fun reasonWords(reason: Int): String = when (reason) {
        ApplicationExitInfo.REASON_EXIT_SELF -> "it ended itself"
        ApplicationExitInfo.REASON_SIGNALED -> "a signal ended it"
        ApplicationExitInfo.REASON_LOW_MEMORY -> "the system ended it, short of memory"
        ApplicationExitInfo.REASON_CRASH -> "it crashed (in Kotlin or Java)"
        ApplicationExitInfo.REASON_CRASH_NATIVE -> "it crashed (in native code)"
        ApplicationExitInfo.REASON_ANR -> "it stopped responding (ANR)"
        ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "it failed to start"
        ApplicationExitInfo.REASON_PERMISSION_CHANGE -> "a permission changed"
        ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE -> "the system ended it for using too much"
        ApplicationExitInfo.REASON_USER_REQUESTED -> "the user ended it"
        ApplicationExitInfo.REASON_USER_STOPPED -> "the user stopped it (force stop)"
        ApplicationExitInfo.REASON_DEPENDENCY_DIED -> "something it depends on ended"
        ApplicationExitInfo.REASON_OTHER -> "the system ended it (another reason)"
        14 -> "the system froze it" // REASON_FREEZER, Android 13
        15 -> "the app was turned off or on" // REASON_PACKAGE_STATE_CHANGE, Android 14
        16 -> "the app was updated" // REASON_PACKAGE_UPDATED, Android 14
        else -> "unknown"
    }

    private fun importanceWords(importance: Int): String = when (importance) {
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND -> "in front"
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND_SERVICE -> "a foreground service"
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE -> "visible"
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_PERCEPTIBLE -> "perceptible"
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_SERVICE -> "a service"
        325 -> "in front, the screen off" // IMPORTANCE_TOP_SLEEPING
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_CANT_SAVE_STATE -> "unable to save its state"
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_CACHED -> "in the background"
        ActivityManager.RunningAppProcessInfo.IMPORTANCE_GONE -> "gone"
        else -> "in between"
    }

    /**
     * What the log says first: the app's version, the device, its system and its memory, and the
     * screen. (The GPU follows, as the core makes its device.)
     */
    private fun header(context: Context): String {
        val info = runCatching { context.packageManager.getPackageInfo(context.packageName, 0) }.getOrNull()
        val memory = ActivityManager.MemoryInfo()
        runCatching { context.getSystemService(ActivityManager::class.java)?.getMemoryInfo(memory) }
        val screen = context.resources.displayMetrics
        val abi = File(context.applicationInfo.nativeLibraryDir ?: "").name
        fun gb(bytes: Long) = "%.1f GB".format(java.util.Locale.ROOT, bytes / 1e9)
        return buildString {
            appendLine("ProCraft ${info?.versionName ?: "?"} (version code ${info?.let { androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(it) } ?: "?"}), started ${java.time.Instant.now()}")
            appendLine("device: ${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE}), build ${Build.FINGERPRINT}")
            appendLine("Android ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT}), ABIs ${Build.SUPPORTED_ABIS.joinToString()}; the app's library: $abi")
            appendLine(
                "memory: ${gb(memory.totalMem)}, ${gb(memory.availMem)} free${if (memory.lowMemory) " (low)" else ""}; " +
                    "the app's heap up to ${Runtime.getRuntime().maxMemory() shr 20} MB",
            )
            appendLine("screen: ${screen.widthPixels}×${screen.heightPixels}, ${screen.densityDpi} dpi")
            appendLine(display(context))
        }
    }

    /**
     * What a report of the screen showing the app wrongly needs: the chip (Android 12 on), whether
     * the screen takes wide colour and HDR, the colour space a wide-colour window is drawn in there,
     * and whether Wide Color is on.
     */
    private fun display(context: Context): String {
        val config = context.resources.configuration
        val chip = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) "${Build.SOC_MANUFACTURER} ${Build.SOC_MODEL}" else "not told before Android 12"
        val wideSpace = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            runCatching {
                context.getSystemService(android.hardware.display.DisplayManager::class.java)
                    ?.getDisplay(android.view.Display.DEFAULT_DISPLAY)?.preferredWideGamutColorSpace?.name
            }.getOrNull() ?: "none"
        } else {
            "not told before Android 10"
        }
        val wideColor = runCatching {
            context.getSharedPreferences("interface", Context.MODE_PRIVATE).getBoolean(
                io.github.artcraftandroid.procraft.canvas.CanvasViewModel.WIDE_COLOR,
                io.github.artcraftandroid.procraft.canvas.wideColorByDefault(),
            )
        }.getOrNull()
        return "chip: $chip; screen: wide colour ${if (config.isScreenWideColorGamut) "yes" else "no"}, " +
            "HDR ${if (config.isScreenHdr) "yes" else "no"}, a wide window drawn in $wideSpace; " +
            "Wide Color ${when (wideColor) { true -> "on"; false -> "off"; null -> "unknown" }}"
    }
}

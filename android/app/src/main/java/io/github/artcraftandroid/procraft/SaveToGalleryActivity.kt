package io.github.artcraftandroid.procraft

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.MediaStore
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File
import java.util.concurrent.Executors

/**
 * The share sheet's "Save to Gallery": exported pictures and videos written into the device's
 * shared Pictures/ProCraft and Movies/ProCraft, where gallery apps show them. Shows nothing of its
 * own but a short note of how it went.
 */
class SaveToGalleryActivity : ComponentActivity() {
    private val files by lazy { intent.getStringArrayExtra(EXTRA_PATHS).orEmpty().map(::File).filter { it.isFile } }
    private val mime by lazy { intent.getStringExtra(EXTRA_MIME) ?: "image/png" }

    // Android 8 and 9 let an app into the shared folders only with leave to write there
    private val askToWrite = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) {
            save()
        } else {
            Toast.makeText(applicationContext, "Couldn't save to Gallery without storage access", Toast.LENGTH_LONG).show()
            finish()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        when {
            files.isEmpty() -> finish()
            Build.VERSION.SDK_INT < Build.VERSION_CODES.Q &&
                checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED ->
                askToWrite.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
            else -> save()
        }
    }

    /** Writes every file off the main thread, then says how it went once and closes. */
    private fun save() {
        val app = applicationContext
        val video = mime.startsWith("video/")
        Executors.newSingleThreadExecutor().execute {
            val saved = files.count { f ->
                val put = { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) insert(f, video) else copyShared(f, video) }
                runCatching(put).getOrDefault(false)
            }
            runOnUiThread {
                val text = when {
                    saved == files.size && saved == 1 -> "Saved to Gallery"
                    saved == files.size -> "$saved saved to Gallery"
                    saved == 0 -> "Couldn't save to Gallery"
                    else -> "Saved $saved of ${files.size} to Gallery"
                }
                Toast.makeText(app, text, if (saved == files.size) Toast.LENGTH_SHORT else Toast.LENGTH_LONG).show()
                finish()
            }
        }
    }

    /**
     * [file] put in the shared Pictures/ProCraft (Movies/ProCraft if a [video]): kept pending, so no
     * gallery shows it part written, until it is whole; taken out again if it can't be.
     */
    @androidx.annotation.RequiresApi(Build.VERSION_CODES.Q)
    private fun insert(file: File, video: Boolean): Boolean {
        val collection =
            if (video) MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            else MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, file.name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${sharedFolder(video)}/$FOLDER")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = contentResolver.insert(collection, values) ?: return false
        val written = runCatching { write(file, uri) }.getOrDefault(false)
        if (!written) {
            runCatching { contentResolver.delete(uri, null, null) }
            return false
        }
        contentResolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        return true
    }

    /** [file] copied whole into [uri]; false if it couldn't be. */
    private fun write(file: File, uri: Uri): Boolean {
        val out = contentResolver.openOutputStream(uri, "w") ?: return false
        out.use { o -> file.inputStream().use { it.copyTo(o) } }
        return true
    }

    /**
     * Android 8 and 9: [file] copied into the shared folder under a name of its own (written beside it
     * first, then put in place whole), then told to the media store so galleries list it.
     */
    @Suppress("DEPRECATION")
    private fun copyShared(file: File, video: Boolean): Boolean {
        val dir = File(Environment.getExternalStoragePublicDirectory(sharedFolder(video)), FOLDER)
        if (!dir.isDirectory && !dir.mkdirs()) return false
        val target = freeName(dir, file.name)
        val part = File(dir, ".${target.name}.part")
        try {
            file.inputStream().use { input -> part.outputStream().use { input.copyTo(it) } }
            if (!part.renameTo(target)) return false
        } finally {
            part.delete()
        }
        MediaScannerConnection.scanFile(applicationContext, arrayOf(target.absolutePath), arrayOf(mime), null)
        return true
    }

    companion object {
        private const val EXTRA_PATHS = "paths"
        private const val EXTRA_MIME = "mime"
        private const val FOLDER = "ProCraft"

        @Suppress("DEPRECATION")
        private fun sharedFolder(video: Boolean) = if (video) Environment.DIRECTORY_MOVIES else Environment.DIRECTORY_PICTURES

        /** What the share sheet starts to save [files] (of type [mime]) to the gallery. */
        fun intent(context: Context, files: List<File>, mime: String): Intent =
            Intent(context, SaveToGalleryActivity::class.java)
                .putExtra(EXTRA_PATHS, files.map { it.absolutePath }.toTypedArray())
                .putExtra(EXTRA_MIME, mime)
    }
}

/** Whether files of type [mime] can go to the gallery: the pictures and videos gallery apps show. */
internal fun savesToGallery(mime: String): Boolean = mime in GALLERY_PICTURES || mime.startsWith("video/")

private val GALLERY_PICTURES = setOf("image/png", "image/jpeg", "image/gif", "image/webp")

/** A name in [dir] no file has yet: [name] itself, else with " (2)", " (3)" … before its extension. */
internal fun freeName(dir: File, name: String): File {
    val first = File(dir, name)
    if (!first.exists()) return first
    val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
    val (stem, ext) = name.substring(0, dot) to name.substring(dot)
    return generateSequence(2) { it + 1 }.map { File(dir, "$stem ($it)$ext") }.first { !it.exists() }
}

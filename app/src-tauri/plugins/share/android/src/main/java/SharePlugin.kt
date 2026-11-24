package io.felixbennett.paperloom.share

import android.app.Activity
import android.content.ContentResolver
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.util.Base64
import android.util.Log
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.chinese.ChineseTextRecognizerOptions
import java.io.File
import java.util.UUID

@InvokeArg
class SetEventHandlerArgs {
    lateinit var handler: Channel
}

@InvokeArg
class RecognizeTextArgs {
    /** A picture of a page: a PNG, in base64. */
    lateinit var png: String
}

/**
 * Keeps what other apps share to Paperloom (the share sheet's SEND and
 * SEND_MULTIPLE) until the app takes it. Files are copied into the cache
 * when taken, while the share's read permission still holds.
 */
@TauriPlugin
class SharePlugin(private val activity: Activity) : Plugin(activity) {
    private val pending = mutableListOf<Intent>()
    private var channel: Channel? = null
    // ML Kit's Chinese model, kept in the app (phones without Google's
    // services have it too); it reads Latin letters as well.
    private val recognizer by lazy { TextRecognition.getClient(ChineseTextRecognizerOptions.Builder().build()) }

    override fun load(webView: WebView) {
        super.load(webView)
        SystemBars.keepClear(webView)
        // The share that started the app. Reopened from Recents, Android
        // hands the same intent again: that one was taken already.
        val intent = activity.intent
        if (intent != null && (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) == 0) {
            // The page may already have asked and found nothing: say so.
            if (keep(intent)) channel?.send(JSObject())
        }
    }

    override fun onNewIntent(intent: Intent) {
        if (keep(intent)) {
            channel?.send(JSObject())
        }
    }

    // Only the Rust side calls this.
    @Command
    fun setEventHandler(invoke: Invoke) {
        channel = invoke.parseArgs(SetEventHandlerArgs::class.java).handler
        invoke.resolve()
        // A share kept before anyone listened (the one that started the app,
        // when the page asked before it was kept) is announced now.
        if (synchronized(pending) { pending.isNotEmpty() }) channel?.send(JSObject())
    }

    // The text in a picture of a scanned page, read on the phone.
    @Command
    fun recognizeText(invoke: Invoke) {
        val png = Base64.decode(invoke.parseArgs(RecognizeTextArgs::class.java).png, Base64.DEFAULT)
        val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size)
        if (bitmap == null) {
            Log.w("PaperloomText", "not a picture (${png.size} bytes)")
            invoke.resolve(JSObject())
            return
        }
        recognizer.process(InputImage.fromBitmap(bitmap, 0))
            .addOnSuccessListener { read ->
                // Only the length: the page's words stay private.
                Log.i("PaperloomText", "read ${read.text.length} characters from ${bitmap.width}x${bitmap.height}")
                // Each line with its box, as shares of the picture's size.
                val lines = JSArray()
                for (block in read.textBlocks) {
                    for (line in block.lines) {
                        val box = line.boundingBox ?: continue
                        val entry = JSObject()
                        entry.put("text", line.text)
                        entry.put("x", box.left.toDouble() / bitmap.width)
                        entry.put("y", box.top.toDouble() / bitmap.height)
                        entry.put("width", box.width().toDouble() / bitmap.width)
                        entry.put("height", box.height().toDouble() / bitmap.height)
                        lines.put(entry)
                    }
                }
                val result = JSObject()
                result.put("lines", lines)
                invoke.resolve(result)
            }
            .addOnFailureListener { e ->
                Log.w("PaperloomText", "recognition failed", e)
                invoke.reject(e.message ?: e.toString())
            }
    }

    @Command
    fun takeShared(invoke: Invoke) {
        val intents = synchronized(pending) { pending.toList().also { pending.clear() } }
        Thread {
            try {
                val folder = File(activity.cacheDir, "shared")
                removeOld(folder)
                val texts = JSArray()
                val files = JSArray()
                for (intent in intents) {
                    read(intent, folder, texts, files)
                }
                val result = JSObject()
                result.put("texts", texts)
                result.put("files", files)
                invoke.resolve(result)
            } catch (e: Exception) {
                invoke.reject(e.message ?: e.toString())
            }
        }.start()
    }

    private fun keep(intent: Intent): Boolean {
        if (intent.action != Intent.ACTION_SEND && intent.action != Intent.ACTION_SEND_MULTIPLE) {
            return false
        }
        synchronized(pending) { pending.add(intent) }
        return true
    }

    private fun read(intent: Intent, folder: File, texts: JSArray, files: JSArray) {
        val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
        if (!text.isNullOrBlank()) {
            val shared = JSObject()
            shared.put("text", text)
            shared.put("subject", intent.getCharSequenceExtra(Intent.EXTRA_SUBJECT)?.toString())
            texts.put(shared)
        }
        for (uri in streams(intent)) {
            val copied = copy(uri, intent.type, folder) ?: continue
            files.put(copied)
        }
    }

    /** The files a share carries, from its extras and its clip data. */
    private fun streams(intent: Intent): List<Uri> {
        val found = mutableListOf<Uri>()
        if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
            val list = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
            }
            list?.let { found.addAll(it) }
        } else {
            val one = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
            }
            one?.let { found.add(it) }
        }
        intent.clipData?.let { clip ->
            for (i in 0 until clip.itemCount) {
                clip.getItemAt(i).uri?.let { found.add(it) }
            }
        }
        // Only what another app offers through a content provider.
        return found.distinct().filter { it.scheme == ContentResolver.SCHEME_CONTENT }
    }

    /** Copies a shared file into its own folder in the cache. */
    private fun copy(uri: Uri, type: String?, folder: File): JSObject? {
        val resolver = activity.contentResolver
        val name = displayName(uri)
        val target = File(File(folder, UUID.randomUUID().toString()).apply { mkdirs() }, name)
        val input = resolver.openInputStream(uri) ?: return null
        input.use { source -> target.outputStream().use { source.copyTo(it) } }
        val file = JSObject()
        file.put("path", target.absolutePath)
        file.put("name", name)
        file.put("mime", resolver.getType(uri) ?: type)
        return file
    }

    /** The file's name as the sharing app gives it, safe as a file name. */
    private fun displayName(uri: Uri): String {
        var name: String? = null
        try {
            activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst() && !it.isNull(0)) name = it.getString(0)
            }
        } catch (e: Exception) {
            // No name to be had: fall back on the address.
        }
        val base = (name ?: uri.lastPathSegment ?: "shared").substringAfterLast('/').substringAfterLast('\\')
        val clean = base.replace(Regex("[\\u0000-\\u001f:*?\"<>|]"), "_").trim().trimStart('.')
        return clean.ifEmpty { "shared" }.take(120)
    }

    /** Copies from earlier shares, a day old or more, have been imported. */
    private fun removeOld(folder: File) {
        val dayAgo = System.currentTimeMillis() - 24 * 60 * 60 * 1000
        folder.listFiles()?.filter { it.lastModified() < dayAgo }?.forEach { it.deleteRecursively() }
    }
}

package io.github.joelcave42.hey

import android.app.Activity
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.Toast
import kotlin.concurrent.thread

const val TAG = "Hey"

/** Sends text to the paired PC in the background and says how it went in a toast. */
fun sendToPc(context: Context, text: String, onDone: (Boolean) -> Unit = {}) {
    val app = context.applicationContext
    val main = Handler(Looper.getMainLooper())
    val pc = Pc.load(app)
    if (pc == null) {
        Toast.makeText(app, "Open Hey and pair it with your PC first.", Toast.LENGTH_LONG).show()
        app.startActivity(Intent(app, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        onDone(false)
        return
    }
    if (text.isBlank()) {
        Toast.makeText(app, "Nothing to send.", Toast.LENGTH_SHORT).show()
        onDone(false)
        return
    }
    thread {
        val error = try {
            HeyClient(app).sendText(pc, text)
            null
        } catch (e: HeyClient.HeyException) {
            e.message
        } catch (e: Exception) {
            Log.w(TAG, "Sending text failed", e)
            "Couldn't send to ${pc.name}: ${e.message}"
        }
        Log.i(TAG, if (error == null) "Sent text (${text.length} chars)" else "Text not sent: $error")
        main.post {
            Toast.makeText(app, error ?: "Sent to ${pc.name}", if (error == null) Toast.LENGTH_SHORT else Toast.LENGTH_LONG).show()
            onDone(error == null)
        }
    }
}

/**
 * Sends text from a screen that closes straight away (the share sheet, the
 * clipboard tile), handing the network part to [SendService].
 */
fun sendFromBackground(context: Context, text: String) {
    when {
        Pc.load(context) == null -> sendToPc(context, text) // explains pairing
        text.isBlank() -> Toast.makeText(context, "Nothing to send.", Toast.LENGTH_SHORT).show()
        else -> SendService.startText(context, text)
    }
}

/** The "Hey" target in Android's share sheet, and in the menu for selected text. Invisible. */
class ShareActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val files = sharedFiles()
        Log.i(TAG, "Share received: ${intent.action}, ${files.size} file(s), text=${intent.hasExtra(Intent.EXTRA_TEXT)}")
        if (files.isNotEmpty()) {
            try {
                if (Pc.load(this) == null) sendToPc(this, "") else SendService.start(this, files)
            } catch (_: SecurityException) {
                // The sharing app didn't let us read what it shared.
                Toast.makeText(this, "That app didn't allow Hey to read the file.", Toast.LENGTH_LONG).show()
            }
            finish()
            return
        }
        val text = when (intent.action) {
            Intent.ACTION_PROCESS_TEXT -> intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()
            else -> intent.getStringExtra(Intent.EXTRA_TEXT)
        }
        sendFromBackground(this, text.orEmpty())
        finish()
    }

    /** Photos and files in a share, whether one (SEND) or many (SEND_MULTIPLE). */
    @Suppress("DEPRECATION")
    private fun sharedFiles(): List<Uri> = when (intent.action) {
        Intent.ACTION_SEND -> listOfNotNull(intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM))
        Intent.ACTION_SEND_MULTIPLE -> intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM).orEmpty()
        else -> emptyList()
    }
}

/**
 * Sends whatever is on the clipboard. Android only lets the app in front read
 * the clipboard, so the quick settings tile opens this invisible activity and
 * it reads the clipboard as soon as it has focus.
 */
class ClipboardActivity : Activity() {
    private var sent = false

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || sent) return
        sent = true
        if (!sendClipboard(this)) {
            Toast.makeText(this, "Your clipboard is empty.", Toast.LENGTH_SHORT).show()
        }
        finish()
    }
}

/**
 * Sends whatever is on the phone's clipboard: a copied picture or file goes
 * as a file, anything else as text. Only works while Hey is in front, which
 * is Android's rule for reading the clipboard. Returns false if it's empty.
 */
fun sendClipboard(context: Context): Boolean {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    val clip = clipboard.primaryClip?.takeIf { it.itemCount > 0 } ?: return false
    val uris = (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
    if (uris.isNotEmpty() && Pc.load(context) != null) {
        try {
            SendService.start(context, uris)
            return true
        } catch (_: SecurityException) {
            // The app that copied it didn't let others read it; fall back to its text.
        }
    }
    val text = clip.getItemAt(0).coerceToText(context)?.toString().orEmpty()
    if (text.isBlank()) return false
    sendFromBackground(context, text)
    return true
}

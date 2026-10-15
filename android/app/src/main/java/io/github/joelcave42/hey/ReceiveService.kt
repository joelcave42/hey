package io.github.joelcave42.hey

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.Network
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.IBinder
import android.provider.MediaStore
import android.util.Log
import android.webkit.MimeTypeMap
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.json.JSONObject
import java.io.IOException
import java.io.InputStream
import kotlin.concurrent.thread

/**
 * "Ready to receive": keeps a connection open to the paired PC so it can send
 * links, text and files to the phone at any time. Runs as a foreground
 * service with a quiet notification, reconnects on its own when the PC or the
 * Wi-Fi comes back, and starts again after the phone restarts.
 */
class ReceiveService : Service() {

    companion object {
        private const val STATUS_CHANNEL = "ready"
        private const val INCOMING_CHANNEL = "incoming"
        private const val STATUS_ID = 2
        private const val PREFS = "receive"

        private val _connected = MutableStateFlow(false)
        /** Whether the PC can reach the phone right now, for the app's status. */
        val connected: StateFlow<Boolean> = _connected

        fun isEnabled(context: Context) =
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean("enabled", true)

        fun setEnabled(context: Context, on: Boolean) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("enabled", on).apply()
            if (on) start(context) else context.stopService(Intent(context, ReceiveService::class.java))
        }

        /** Starts the service if the phone is paired and receiving is on. */
        fun start(context: Context) {
            if (Pc.load(context) == null || !isEnabled(context)) return
            try {
                context.startForegroundService(Intent(context, ReceiveService::class.java))
            } catch (e: Exception) {
                // Android sometimes refuses background starts; the app starts it when opened.
                Log.w(TAG, "Couldn't start the receiver", e)
            }
        }
    }

    @Volatile private var running = true
    private val wake = Object()
    private var current: HeyClient.Listener? = null
    private lateinit var connectivity: ConnectivityManager

    // Reconnect straight away when a network comes up, rather than waiting out the backoff.
    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = synchronized(wake) { wake.notifyAll() }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createChannels()
        startInForeground(statusNotification(false))
        connectivity = getSystemService(ConnectivityManager::class.java)
        connectivity.registerDefaultNetworkCallback(networkCallback)
        thread(name = "hey-receive") { loop() }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int) = START_STICKY

    override fun onDestroy() {
        running = false
        runCatching { connectivity.unregisterNetworkCallback(networkCallback) }
        runCatching { current?.close() }
        synchronized(wake) { wake.notifyAll() }
        _connected.value = false
        super.onDestroy()
    }

    private fun loop() {
        var backoff = 2_000L
        while (running) {
            val pc = Pc.load(this)
            if (pc == null || !isEnabled(this)) {
                stopSelf()
                return
            }
            try {
                HeyClient(this).listen(pc).use { listener ->
                    current = listener
                    setConnected(true, pc)
                    backoff = 2_000L
                    serve(listener)
                }
            } catch (e: Exception) {
                Log.i(TAG, "Receiver disconnected: ${e.message}")
            } finally {
                current = null
                setConnected(false, pc)
            }
            if (!running) return
            synchronized(wake) { wake.wait(backoff) }
            backoff = (backoff * 2).coerceAtMost(30_000L)
        }
    }

    private fun serve(listener: HeyClient.Listener) {
        while (running) {
            val line = listener.readLine() ?: throw IOException("PC closed the connection")
            val message = JSONObject(line)
            when (message.optString("t")) {
                "ping" -> listener.send(JSONObject().put("t", "pong"))
                "text" -> {
                    receivedText(message.getString("text"))
                    listener.send(JSONObject().put("ok", true))
                }
                "file" -> {
                    val name = message.optString("name", "file")
                    val size = message.getLong("size")
                    val reply = try {
                        receivedFile(name, size, listener.input)
                        JSONObject().put("ok", true)
                    } catch (e: IOException) {
                        throw e // the stream is out of step now; reconnect
                    } catch (e: Exception) {
                        Log.w(TAG, "Couldn't save $name", e)
                        JSONObject().put("ok", false).put("error", "The phone couldn't save it: ${e.message}")
                    }
                    listener.send(reply)
                }
                else -> listener.send(JSONObject().put("ok", false).put("error", "Update Hey on your phone."))
            }
        }
    }

    private fun receivedText(text: String) {
        getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("From your PC", text))
        History.addText(this, text, incoming = true)
        val link = Regex("""https?://\S+""").find(text)?.value
        val open = if (link != null) Intent(Intent.ACTION_VIEW, Uri.parse(link)) else launchApp()
        val notification = Notification.Builder(this, INCOMING_CHANNEL)
            .setSmallIcon(R.drawable.ic_tile)
            .setContentTitle(if (link != null) "Link from your PC" else "From your PC")
            .setContentText(text.take(200))
            .setStyle(Notification.BigTextStyle().bigText("${text.take(600)}\n\nCopied to your clipboard."))
            .setContentIntent(activity(open))
            .setAutoCancel(true)
            .apply { if (link != null) addAction(Notification.Action.Builder(null, "Open link", activity(open)).build()) }
            .addAction(Notification.Action.Builder(null, "Copy", CopyReceiver.intent(this, text)).build())
            .build()
        notify(notification)
    }

    /** Streams a file into Download/Hey on the phone. */
    private fun receivedFile(name: String, size: Long, input: InputStream) {
        val mime = MimeTypeMap.getSingleton()
            .getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase()) ?: "application/octet-stream"
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, name)
            put(MediaStore.Downloads.MIME_TYPE, mime)
            put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Hey")
            put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val uri = contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            ?: throw IllegalStateException("No room in Downloads")
        try {
            contentResolver.openOutputStream(uri)!!.use { out ->
                val buffer = ByteArray(64 * 1024)
                var left = size
                while (left > 0) {
                    val n = input.read(buffer, 0, minOf(buffer.size.toLong(), left).toInt())
                    if (n < 0) throw IOException("PC stopped sending partway through")
                    out.write(buffer, 0, n)
                    left -= n
                }
            }
        } catch (e: Exception) {
            contentResolver.delete(uri, null, null)
            throw e
        }
        contentResolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
        History.addFiles(this, listOf(name), incoming = true)

        val open = Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val kind = when {
            mime.startsWith("image/") -> "Photo"
            mime.startsWith("video/") -> "Video"
            else -> "File"
        }
        val builder = Notification.Builder(this, INCOMING_CHANNEL)
            .setSmallIcon(R.drawable.ic_tile)
            .setContentTitle("$kind from your PC")
            .setContentText("$name · saved to Download/Hey")
            .setContentIntent(activity(Intent.createChooser(open, null)))
            .setAutoCancel(true)
            .addAction(Notification.Action.Builder(null, "Open", activity(Intent.createChooser(open, null))).build())
        if (mime.startsWith("image/")) preview(uri)?.let { builder.setStyle(Notification.BigPictureStyle().bigPicture(it)).setLargeIcon(it) }
        notify(builder.build())
    }

    private fun preview(uri: Uri) = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= 720) sample *= 2
        contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample })
        }
    }.getOrNull()

    private fun setConnected(on: Boolean, pc: Pc?) {
        if (_connected.value == on) return
        _connected.value = on
        if (running) notify(statusNotification(on, pc), STATUS_ID)
    }

    private fun statusNotification(on: Boolean, pc: Pc? = Pc.load(this)): Notification {
        val name = pc?.label ?: "your PC"
        return Notification.Builder(this, STATUS_CHANNEL)
            .setSmallIcon(R.drawable.ic_tile)
            .setContentTitle(if (on) "Ready to receive from $name" else "Waiting for $name")
            .setContentText(if (on) "Send from the Hey window on your PC" else "Hey connects when your PC is on the same Wi-Fi")
            .setContentIntent(activity(launchApp()))
            .setOngoing(true)
            .setShowWhen(false)
            .build()
    }

    private fun startInForeground(notification: Notification) {
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(STATUS_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(STATUS_ID, notification)
        }
    }

    private fun createChannels() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(STATUS_CHANNEL, "Ready to receive", NotificationManager.IMPORTANCE_MIN).apply {
                description = "Shows while Hey can receive from your PC"
                setShowBadge(false)
            }
        )
        manager.createNotificationChannel(
            NotificationChannel(INCOMING_CHANNEL, "From your PC", NotificationManager.IMPORTANCE_HIGH)
        )
    }

    private fun notify(notification: Notification, id: Int = (System.currentTimeMillis() and 0x7fffffff).toInt()) =
        getSystemService(NotificationManager::class.java).notify(id, notification)

    private fun launchApp() = Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    private fun activity(intent: Intent): PendingIntent = PendingIntent.getActivity(
        this, intent.hashCode(), intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
}

/** The Copy button on a "From your PC" notification. */
class CopyReceiver : BroadcastReceiver() {
    companion object {
        fun intent(context: Context, text: String): PendingIntent = PendingIntent.getBroadcast(
            context, text.hashCode(),
            Intent(context, CopyReceiver::class.java).putExtra("text", text),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    override fun onReceive(context: Context, intent: Intent) {
        val text = intent.getStringExtra("text") ?: return
        context.getSystemService(ClipboardManager::class.java)
            .setPrimaryClip(ClipData.newPlainText("From your PC", text))
    }
}

/** Starts receiving again after the phone restarts or Hey updates. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED || intent.action == Intent.ACTION_MY_PACKAGE_REPLACED) {
            ReceiveService.start(context)
        }
    }
}

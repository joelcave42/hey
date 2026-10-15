package io.github.joelcave42.hey

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.provider.OpenableColumns
import android.util.Log
import android.widget.Toast
import java.io.File
import java.util.concurrent.Executors

/**
 * Sends photos and files to the PC. It runs as a foreground service with a
 * progress notification, so a big video keeps going after the share sheet
 * closes. Batches queue up and go one after another.
 */
class SendService : Service() {

    companion object {
        private const val CHANNEL = "sending"
        private const val NOTIFICATION_ID = 1

        fun start(context: Context, uris: List<Uri>) {
            val intent = Intent(context, SendService::class.java)
                .putParcelableArrayListExtra("uris", ArrayList(uris))
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            // ClipData carries the read permission the sharing app gave us
            // over to the service.
            intent.clipData = ClipData.newRawUri("", uris[0]).apply {
                uris.drop(1).forEach { addItem(ClipData.Item(it)) }
            }
            context.startForegroundService(intent)
        }

        /**
         * Sends text from the share sheet or the clipboard tile. Those screens
         * close straight away, and with battery saving on Android cuts the
         * network for apps in the background, so text goes through this
         * service too.
         */
        fun startText(context: Context, text: String) {
            context.startForegroundService(Intent(context, SendService::class.java).putExtra("text", text))
        }
    }

    private val worker = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private var pending = 0

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startInForeground(progressNotification("Sending to your PC…", 0, 0))
        @Suppress("DEPRECATION")
        val uris = intent?.getParcelableArrayListExtra<Uri>("uris").orEmpty()
        val text = intent?.getStringExtra("text")
        if (uris.isEmpty() && text == null) {
            finishIfIdle()
            return START_NOT_STICKY
        }
        pending++
        worker.execute {
            val message = try {
                if (text != null) sendText(text) else send(uris)
            } catch (e: HeyClient.HeyException) {
                e.message
            } catch (e: Exception) {
                Log.w(TAG, "Sending failed", e)
                "Couldn't send: ${e.message}"
            }
            Log.i(TAG, "Send finished: $message")
            main.post {
                Toast.makeText(this, message, Toast.LENGTH_LONG).show()
                pending--
                finishIfIdle()
            }
        }
        return START_NOT_STICKY
    }

    private data class Item(val uri: Uri, val name: String, val size: Long)

    private fun sendText(text: String): String {
        val pc = Pc.load(this) ?: return "Open Hey and pair it with your PC first."
        HeyClient(this).sendText(pc, text)
        return "Sent to ${pc.name}"
    }

    /** Sends a batch and returns what to tell the user. */
    private fun send(uris: List<Uri>): String {
        val pc = Pc.load(this) ?: return "Open Hey and pair it with your PC first."
        val client = HeyClient(this)
        val items = uris.map { describe(it) }
        val total = items.sumOf { it.size }.coerceAtLeast(1)
        var done = 0L
        var lastUpdate = 0L

        items.forEachIndexed { index, item ->
            val label = if (items.size == 1) item.name else "${index + 1} of ${items.size}: ${item.name}"
            client.sendFile(
                pc, item.name, item.size, index, items.size,
                open = { openStream(item) },
                progress = { sent ->
                    val now = SystemClock.elapsedRealtime()
                    if (now - lastUpdate > 250) {
                        lastUpdate = now
                        val percent = ((done + sent) * 100 / total).toInt()
                        notify(progressNotification("Sending to ${pc.name}", percent, 100, label))
                    }
                },
            )
            done += item.size
        }
        return when (items.size) {
            1 -> "Sent ${items[0].name} to ${pc.name}"
            else -> "Sent ${items.size} files to ${pc.name}"
        }
    }

    /** Name and size of a shared file. Copies it first if the app sharing it won't say how big it is. */
    private fun describe(uri: Uri): Item {
        var name: String? = null
        var size = -1L
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)
            ?.use { c ->
                if (c.moveToFirst()) {
                    name = c.getString(0)
                    if (!c.isNull(1)) size = c.getLong(1)
                }
            }
        val finalName = name ?: uri.lastPathSegment?.substringAfterLast('/') ?: "file"
        if (size >= 0) return Item(uri, finalName, size)

        val copy = File(cacheDir, "send-${System.nanoTime()}")
        contentResolver.openInputStream(uri)!!.use { input -> copy.outputStream().use { input.copyTo(it) } }
        return Item(Uri.fromFile(copy), finalName, copy.length())
    }

    private fun openStream(item: Item) =
        if (item.uri.scheme == "file") File(item.uri.path!!).inputStream()
        else contentResolver.openInputStream(item.uri) ?: throw HeyClient.HeyException("Couldn't open ${item.name}.")

    private fun finishIfIdle() {
        if (pending > 0) return
        cacheDir.listFiles { f -> f.name.startsWith("send-") }?.forEach { it.delete() }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        worker.shutdown()
        super.onDestroy()
    }

    private fun startInForeground(notification: Notification) {
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun notify(notification: Notification) {
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
    }

    private fun progressNotification(title: String, progress: Int, max: Int, text: String? = null): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Sending to your PC", NotificationManager.IMPORTANCE_LOW)
            )
        }
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_tile)
            .setContentTitle(title)
            .setContentText(text)
            .setProgress(max, progress, max == 0)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()
    }
}

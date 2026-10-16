package io.github.joelcave42.hey

import android.content.Context
import android.content.Intent
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import io.github.joelcave42.hey.ui.Accent
import io.github.joelcave42.hey.ui.HeyTheme
import org.json.JSONObject
import java.io.DataInputStream
import java.io.EOFException
import kotlin.concurrent.thread

/**
 * Watches a window on the PC live. The PC sends H.264 video (see view.rs);
 * the phone's hardware decoder draws it straight to the screen, set up for
 * low latency.
 */
class ViewerActivity : ComponentActivity() {

    companion object {
        fun start(context: Context, target: String, title: String) {
            context.startActivity(
                Intent(context, ViewerActivity::class.java)
                    .putExtra("target", target)
                    .putExtra("title", title)
            )
        }
    }

    private var size by mutableStateOf<Pair<Int, Int>?>(null)
    private var problem by mutableStateOf<String?>(null)
    private var fps by mutableStateOf(0)

    @Volatile private var running = false
    private var listener: HeyClient.Listener? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val title = intent.getStringExtra("title").orEmpty()
        setContent { HeyTheme { Viewer(title) } }
    }

    @Composable
    private fun Viewer(title: String) {
        Box(Modifier.fillMaxSize().background(Color.Black), contentAlignment = Alignment.Center) {
            val current = size
            AndroidView(
                factory = { context ->
                    SurfaceView(context).apply {
                        holder.addCallback(object : SurfaceHolder.Callback {
                            override fun surfaceCreated(holder: SurfaceHolder) = startStream(holder.surface)
                            override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, ht: Int) {}
                            override fun surfaceDestroyed(holder: SurfaceHolder) = stopStream()
                        })
                    }
                },
                modifier = if (current != null) {
                    Modifier.fillMaxWidth().aspectRatio(current.first.toFloat() / current.second, matchHeightConstraintsFirst = false)
                } else {
                    Modifier.size(1.dp)
                },
            )
            if (current == null && problem == null) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                    CircularProgressIndicator(color = Color.White)
                    Spacer(Modifier.size(16.dp))
                    Text("Connecting to your PC…", color = Color.White.copy(alpha = 0.8f))
                }
            }
            problem?.let {
                Text(
                    it,
                    color = Color.White,
                    fontSize = 16.sp,
                    modifier = Modifier.padding(32.dp),
                )
            }
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .fillMaxWidth()
                    .statusBarsPadding()
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                IconButton(
                    onClick = { finish() },
                    modifier = Modifier.clip(CircleShape).background(Color.Black.copy(alpha = 0.5f)),
                ) {
                    Icon(Icons.Rounded.Close, "Close", tint = Color.White)
                }
                Spacer(Modifier.width(10.dp))
                // The title takes whatever room the live badge leaves.
                Box(Modifier.weight(1f)) {
                    Text(
                        title,
                        color = Color.White,
                        fontSize = 15.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier
                            .clip(RoundedCornerShape(50))
                            .background(Color.Black.copy(alpha = 0.5f))
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                    )
                }
                Spacer(Modifier.width(10.dp))
                if (current != null) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier
                            .clip(RoundedCornerShape(50))
                            .background(Color.Black.copy(alpha = 0.5f))
                            .padding(horizontal = 10.dp, vertical = 6.dp),
                    ) {
                        Box(Modifier.size(8.dp).clip(CircleShape).background(Accent.Bad))
                        Spacer(Modifier.width(6.dp))
                        Text("LIVE · $fps fps", color = Color.White, fontSize = 12.sp)
                    }
                }
            }
        }
    }

    private fun startStream(surface: Surface) {
        if (running) return
        running = true
        val target = intent.getStringExtra("target") ?: "screen"
        thread(name = "hey-viewer") {
            val pc = Pc.load(this)
            if (pc == null) {
                problem = "Pair Hey with your PC first."
                return@thread
            }
            var decoder: MediaCodec? = null
            try {
                val stream = HeyClient(this).view(pc, target).also { listener = it }
                val input = DataInputStream(stream.input)
                var frames = 0
                var frameCount = 0L
                var second = System.nanoTime()
                val info = MediaCodec.BufferInfo()
                while (running) {
                    val length = input.readInt()
                    val kind = input.readUnsignedByte()
                    val payload = ByteArray(length - 1)
                    input.readFully(payload)
                    when (kind) {
                        1 -> {
                            // The window changed size: start a fresh decoder for it.
                            val json = JSONObject(String(payload))
                            val w = json.getInt("width")
                            val h = json.getInt("height")
                            decoder?.let { runCatching { it.stop(); it.release() } }
                            decoder = MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_VIDEO_AVC).apply {
                                val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, w, h)
                                if (Build.VERSION.SDK_INT >= 30) format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
                                format.setInteger(MediaFormat.KEY_PRIORITY, 0)
                                configure(format, surface, null, 0)
                                start()
                            }
                            runOnUiThread { size = w to h }
                        }
                        2 -> {
                            val codec = decoder ?: continue
                            feed(codec, payload, frameCount++ * 33_333)
                            drain(codec, info)
                            frames++
                        }
                    }
                    val now = System.nanoTime()
                    if (now - second >= 1_000_000_000L) {
                        val shown = frames
                        runOnUiThread { fps = shown }
                        frames = 0
                        second = now
                    }
                }
            } catch (e: HeyClient.HeyException) {
                problem = e.message
            } catch (_: EOFException) {
                if (running) problem = "The window closed, or Hey on your PC stopped."
            } catch (e: Exception) {
                Log.w(TAG, "Viewer stopped", e)
                if (running) problem = "Lost the connection to your PC."
            } finally {
                decoder?.let { runCatching { it.stop(); it.release() } }
            }
        }
    }

    /** Hands one frame to the decoder, waiting briefly for room. Frames can't be skipped here. */
    private fun feed(codec: MediaCodec, data: ByteArray, timeUs: Long) {
        val info = MediaCodec.BufferInfo()
        while (running) {
            val index = codec.dequeueInputBuffer(10_000)
            if (index >= 0) {
                codec.getInputBuffer(index)!!.apply { clear(); put(data) }
                codec.queueInputBuffer(index, 0, data.size, timeUs, 0)
                return
            }
            drain(codec, info)
        }
    }

    /** Shows every decoded frame straight away. */
    private fun drain(codec: MediaCodec, info: MediaCodec.BufferInfo) {
        while (true) {
            val index = codec.dequeueOutputBuffer(info, 0)
            if (index < 0) return
            codec.releaseOutputBuffer(index, true)
        }
    }

    private fun stopStream() {
        running = false
        runCatching { listener?.close() }
    }

    override fun onDestroy() {
        stopStream()
        super.onDestroy()
    }
}

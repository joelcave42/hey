package io.github.joelcave42.hey

import android.content.Context
import android.content.Intent
import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.Surface
import android.view.TextureView
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.FitScreen
import androidx.compose.material.icons.rounded.Keyboard
import androidx.compose.material.icons.rounded.SwapHoriz
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import io.github.joelcave42.hey.ui.Accent
import io.github.joelcave42.hey.ui.HeyTheme
import io.github.joelcave42.hey.ui.WindowChoice
import io.github.joelcave42.hey.ui.WindowPicker
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.DataInputStream
import java.io.EOFException
import kotlin.concurrent.thread
import kotlin.math.roundToInt

/**
 * Watches (and controls) a window on the PC. The PC sends H.264 video (see
 * view.rs); the phone's hardware decoder draws it into a TextureView, set
 * up for low latency. Touches go back through [RemoteControl].
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

    private var target by mutableStateOf("screen")
    private var title by mutableStateOf("")
    private var size by mutableStateOf<Pair<Int, Int>?>(null)
    private var problem by mutableStateOf<String?>(null)
    private var fps by mutableStateOf(0)
    private var controllable by mutableStateOf(false)
    private var scale by mutableStateOf(1f)
    private var pan by mutableStateOf(Offset.Zero)

    @Volatile private var running = false
    private var surface: Surface? = null
    private var listener: HeyClient.Listener? = null
    private var control: RemoteControl? = null
    private var streamThread: Thread? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        target = intent.getStringExtra("target") ?: "screen"
        title = intent.getStringExtra("title").orEmpty()
        setContent { HeyTheme { Viewer() } }
    }

    @Composable
    private fun Viewer() {
        val view = LocalView.current
        val density = LocalDensity.current
        val scope = rememberCoroutineScope()
        var typing by remember { mutableStateOf(false) }
        var switching by remember { mutableStateOf(false) }
        var choices by remember { mutableStateOf<List<WindowChoice>?>(null) }
        var transform by remember { mutableStateOf<VideoTransform?>(null) }

        BoxWithConstraints(Modifier.fillMaxSize().background(Color.Black)) {
            val boxW = constraints.maxWidth.toFloat()
            val boxH = constraints.maxHeight.toFloat()
            val current = size

            // Fit the picture to the screen, in portrait or landscape.
            if (current != null) {
                val ratio = current.first.toFloat() / current.second
                val (w, h) = if (boxW / boxH > ratio) boxH * ratio to boxH else boxW to boxW / ratio
                val left = (boxW - w) / 2
                val top = (boxH - h) / 2
                transform = VideoTransform(left, top, w, h, scale, pan, Offset(boxW / 2, boxH / 2))
            }

            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        scaleX = scale
                        scaleY = scale
                        translationX = pan.x
                        translationY = pan.y
                    }
            ) {
                val t = transform
                AndroidView(
                    factory = { context ->
                        TextureView(context).apply {
                            surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                                override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                                    surface = Surface(st)
                                    startStream()
                                }
                                override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {}
                                override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean {
                                    stopStream()
                                    surface?.release()
                                    surface = null
                                    return true
                                }
                                override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
                            }
                        }
                    },
                    modifier = if (t != null) {
                        with(density) {
                            Modifier
                                .offset { IntOffset(t.left.roundToInt(), t.top.roundToInt()) }
                                .size(t.width.toDp(), t.height.toDp())
                        }
                    } else {
                        Modifier.size(1.dp)
                    },
                )
            }

            // Touches over the whole screen: control the PC, or zoom with two fingers.
            Box(
                Modifier
                    .fillMaxSize()
                    .remoteGestures(
                        view = view,
                        transform = { transform },
                        send = { event -> control?.send(event) },
                        onZoom = { centroid, delta, zoom ->
                            val center = Offset(boxW / 2, boxH / 2)
                            val newScale = (scale * zoom).coerceIn(1f, 5f)
                            val k = newScale / scale
                            val newPan = (pan - (centroid - center)) * k + (centroid - center) + delta
                            scale = newScale
                            pan = if (newScale <= 1.01f) Offset.Zero else newPan
                        },
                    )
            )

            if (current == null && problem == null) {
                Column(
                    Modifier.align(Alignment.Center),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    CircularProgressIndicator(color = Color.White)
                    Spacer(Modifier.size(16.dp))
                    Text("Connecting to your PC…", color = Color.White.copy(alpha = 0.8f))
                }
            }
            problem?.let {
                Text(it, color = Color.White, fontSize = 16.sp, modifier = Modifier.align(Alignment.Center).padding(32.dp))
            }

            TopBar(
                onClose = { finish() },
                onKeyboard = { typing = !typing }.takeIf { controllable },
                onFit = { scale = 1f; pan = Offset.Zero }.takeIf { scale > 1.01f },
                onSwitch = {
                    switching = true
                    choices = null
                    scope.launch {
                        val pc = Pc.load(this@ViewerActivity) ?: return@launch
                        val reply = withContext(Dispatchers.IO) {
                            runCatching { HeyClient(this@ViewerActivity).windows(pc) }.getOrNull()
                        }
                        choices = reply?.let(::windowChoices).orEmpty()
                    }
                },
                live = current != null,
            )

            if (typing) Typing(onDone = { typing = false })
        }

        if (switching) {
            WindowPicker(
                choices = choices,
                problem = null,
                onPick = { choice ->
                    switching = false
                    switchTo(choice)
                },
                onDismiss = { switching = false },
            )
        }
    }

    @Composable
    private fun TopBar(
        onClose: () -> Unit,
        onKeyboard: (() -> Unit)?,
        onFit: (() -> Unit)?,
        onSwitch: () -> Unit,
        live: Boolean,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .statusBarsPadding()
                .padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            RoundButton(Icons.Rounded.Close, "Close", onClose)
            Spacer(Modifier.width(8.dp))
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
            if (onFit != null) RoundButton(Icons.Rounded.FitScreen, "Fit to screen", onFit)
            RoundButton(Icons.Rounded.SwapHoriz, "Watch something else", onSwitch)
            if (onKeyboard != null) RoundButton(Icons.Rounded.Keyboard, "Keyboard", onKeyboard)
            if (live) {
                Spacer(Modifier.width(6.dp))
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier
                        .clip(RoundedCornerShape(50))
                        .background(Color.Black.copy(alpha = 0.5f))
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                ) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(Accent.Bad))
                    Spacer(Modifier.width(6.dp))
                    Text(
                        if (controllable) "$fps fps" else "View only · $fps fps",
                        color = Color.White,
                        fontSize = 12.sp,
                    )
                }
            }
        }
    }

    @Composable
    private fun RoundButton(icon: ImageVector, label: String, onClick: () -> Unit) {
        IconButton(
            onClick = onClick,
            modifier = Modifier
                .padding(start = 4.dp)
                .clip(CircleShape)
                .background(Color.Black.copy(alpha = 0.5f)),
        ) {
            Icon(icon, label, tint = Color.White)
        }
    }

    /**
     * An invisible text box that brings up the phone's keyboard and sends
     * what's typed to the PC. It always holds one space, so Backspace on an
     * "empty" box still registers.
     */
    @Composable
    private fun Typing(onDone: () -> Unit) {
        val focus = remember { FocusRequester() }
        val keyboard = LocalSoftwareKeyboardController.current
        var value by remember { mutableStateOf(TextFieldValue(" ", TextRange(1))) }
        BasicTextField(
            value = value,
            onValueChange = { new ->
                val text = new.text
                when {
                    text.isEmpty() -> control?.key("Backspace")
                    text.length > 1 -> control?.text(text.substring(1))
                }
                value = TextFieldValue(" ", TextRange(1))
            },
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
            keyboardActions = KeyboardActions(onSend = { control?.key("Enter") }),
            modifier = Modifier
                .size(1.dp)
                .alpha(0f)
                .focusRequester(focus),
        )
        androidx.compose.runtime.LaunchedEffect(Unit) {
            focus.requestFocus()
            keyboard?.show()
        }
        androidx.compose.runtime.DisposableEffect(Unit) { onDispose { keyboard?.hide(); onDone() } }
    }

    private fun windowChoices(reply: JSONObject): List<WindowChoice> {
        val all = mutableListOf<WindowChoice>()
        for (key in listOf("screens", "windows")) {
            val list = reply.optJSONArray(key) ?: continue
            for (i in 0 until list.length()) {
                val w = list.getJSONObject(i)
                all += WindowChoice(w.getString("id"), w.getString("title"), w.optString("app"))
            }
        }
        return all
    }

    private fun switchTo(choice: WindowChoice) {
        stopStream()
        streamThread?.join(2000)
        target = choice.id
        title = choice.title
        size = null
        problem = null
        scale = 1f
        pan = Offset.Zero
        startStream()
    }

    private fun startStream() {
        val surface = surface ?: return
        if (running) return
        running = true
        streamThread = thread(name = "hey-viewer") {
            val pc = Pc.load(this)
            if (pc == null) {
                problem = "Pair Hey with your PC first."
                return@thread
            }
            var decoder: MediaCodec? = null
            try {
                val stream = HeyClient(this).view(pc, target).also { listener = it }
                val session = stream.reply.optString("session")
                val allowed = stream.reply.optBoolean("control") && session.isNotEmpty()
                controllable = allowed
                if (allowed) control = RemoteControl(this, pc, session)

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
                            // A new picture size: start a fresh decoder for it.
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
                            feed(codec, payload, frameCount++ * 16_666)
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
                if (running) {
                    Log.w(TAG, "Viewer stopped", e)
                    problem = "Lost the connection to your PC."
                }
            } finally {
                decoder?.let { runCatching { it.stop(); it.release() } }
                control?.close()
                control = null
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
        control?.close()
    }

    override fun onDestroy() {
        stopStream()
        super.onDestroy()
    }
}

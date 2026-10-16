package io.github.joelcave42.hey

import android.content.Context
import android.util.Log
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.unit.dp
import org.json.JSONObject
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.sign

/**
 * Sends taps, scrolls and typing to the PC while watching it (input.rs on
 * the PC). Actions queue up and go out in order on their own connection.
 */
class RemoteControl(context: Context, pc: Pc, session: String) {
    private val queue = LinkedBlockingQueue<JSONObject>()
    @Volatile private var closed = false
    @Volatile private var connection: HeyClient.Listener? = null

    init {
        val app = context.applicationContext
        thread(name = "hey-control") {
            try {
                val listener = HeyClient(app).control(pc, session)
                connection = listener
                while (!closed) {
                    val event = queue.poll(1, TimeUnit.SECONDS) ?: continue
                    listener.send(event)
                }
            } catch (e: Exception) {
                if (!closed) Log.w(TAG, "Control connection ended", e)
            }
        }
    }

    fun send(event: JSONObject) {
        if (!closed) queue.offer(event)
    }

    fun text(text: String) = send(JSONObject().put("e", "text").put("text", text))

    fun key(name: String) = send(JSONObject().put("e", "key").put("key", name))

    fun close() {
        closed = true
        runCatching { connection?.close() }
    }
}

/** Where the video sits on screen, and the zoom applied to it. */
data class VideoTransform(
    val left: Float,
    val top: Float,
    val width: Float,
    val height: Float,
    val scale: Float,
    val offset: Offset,
    val center: Offset,
) {
    /** A touch on screen as a fraction (0 to 1) of the PC's picture, or null if it's off the picture. */
    fun toPicture(touch: Offset): Pair<Double, Double>? {
        val content = (touch - center - offset) / scale + center
        val x = (content.x - left) / width
        val y = (content.y - top) / height
        if (x !in -0.02f..1.02f || y !in -0.02f..1.02f) return null
        return x.coerceIn(0f, 1f).toDouble() to y.coerceIn(0f, 1f).toDouble()
    }
}

/**
 * Touch gestures on the watched picture:
 *   tap = click (two quick taps double-click), one-finger drag = scroll,
 *   hold = right click, hold then drag = drag with the left button,
 *   two fingers = zoom and move around the picture (handled by [onZoom]).
 */
fun Modifier.remoteGestures(
    view: View,
    transform: () -> VideoTransform?,
    send: (JSONObject) -> Unit,
    onZoom: (centroid: Offset, pan: Offset, zoom: Float) -> Unit,
) = pointerInput(Unit) {
    val longPress = 450L
    val scrollStep = 28.dp.toPx()
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false)
        val start = down.position
        val startedAt = System.nanoTime()
        fun at(name: String, p: Offset): JSONObject? {
            val (x, y) = transform()?.toPicture(p) ?: return null
            return JSONObject().put("e", name).put("x", x).put("y", y)
        }

        var mode = "undecided" // then "scroll", "drag" or "zoom"
        var held = false
        var last = start
        var scrolled = 0f
        var previous: List<Offset>? = null
        while (true) {
            val waitMs = longPress - (System.nanoTime() - startedAt) / 1_000_000
            val event = if (mode == "undecided" && !held && waitMs > 0) {
                withTimeoutOrNull(waitMs) { awaitPointerEvent() }
            } else {
                awaitPointerEvent()
            }
            if (event == null) {
                held = true
                view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                continue
            }
            val pressed = event.changes.filter { it.pressed }
            if (pressed.size >= 2) {
                // Two fingers: zoom and pan the picture on the phone.
                if (mode == "drag") at("up", last)?.let(send)
                mode = "zoom"
                val points = pressed.take(2).map { it.position }
                val before = previous
                if (before != null && before.size == 2) {
                    val centroid = (points[0] + points[1]) / 2f
                    val oldCentroid = (before[0] + before[1]) / 2f
                    val distance = hypot(points[0].x - points[1].x, points[0].y - points[1].y)
                    val oldDistance = hypot(before[0].x - before[1].x, before[0].y - before[1].y)
                    if (oldDistance > 0f) onZoom(centroid, centroid - oldCentroid, distance / oldDistance)
                }
                previous = points
                event.changes.forEach { it.consume() }
                continue
            }
            previous = null
            val change = event.changes.first()
            if (pressed.isEmpty()) {
                when {
                    mode == "drag" -> at("up", change.position)?.let(send)
                    mode == "undecided" && held -> at("right", start)?.let(send)
                    mode == "undecided" -> at("tap", start)?.let(send)
                }
                return@awaitEachGesture
            }
            if (mode == "zoom") continue
            val position = change.position
            if (mode == "undecided" && (position - start).getDistance() > viewConfiguration.touchSlop) {
                mode = if (held) "drag" else "scroll"
                if (mode == "drag") at("down", start)?.let(send)
            }
            when (mode) {
                "scroll" -> {
                    // Finger down moves the page down, like scrolling on a phone.
                    scrolled += position.y - last.y
                    while (abs(scrolled) >= scrollStep) {
                        val notch = sign(scrolled)
                        at("scroll", position)?.put("dy", notch.toDouble())?.let(send)
                        scrolled -= notch * scrollStep
                    }
                }
                "drag" -> if (change.positionChanged()) at("move", position)?.let(send)
            }
            last = position
            change.consume()
        }
    }
}

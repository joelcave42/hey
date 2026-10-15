package io.github.joelcave42.hey

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.json.JSONArray
import org.json.JSONObject

/** What the phone has sent, newest first, for the Recent list. Kept on the phone only. */
object History {

    enum class Kind { Link, Text, Code, Photo, Video, File }

    data class Entry(val kind: Kind, val label: String, val text: String?, val time: Long)

    private const val PREFS = "history"
    private const val LIMIT = 30

    private val entries = MutableStateFlow<List<Entry>?>(null)

    fun entries(context: Context): StateFlow<List<Entry>?> {
        if (entries.value == null) entries.value = load(context)
        return entries
    }

    fun addText(context: Context, text: String) {
        val trimmed = text.trim()
        val code = Regex("""\b\d{4,8}\b""").find(trimmed)?.value
        val isCode = code != null && (trimmed == code || Regex("(?i)code|otp|verif|pin|sign.?in|login").containsMatchIn(trimmed))
        val link = Regex("""https?://\S+""").find(trimmed)?.value
        val entry = when {
            isCode -> Entry(Kind.Code, "Code $code", trimmed, now())
            link != null -> Entry(Kind.Link, link.removePrefix("https://").removePrefix("http://"), trimmed, now())
            else -> Entry(Kind.Text, trimmed.lines().first().take(80), trimmed, now())
        }
        add(context, entry)
    }

    fun addFiles(context: Context, names: List<String>) {
        if (names.isEmpty()) return
        val kinds = names.map(::kindOf)
        val kind = if (kinds.all { it == kinds[0] }) kinds[0] else Kind.File
        val label = if (names.size == 1) names[0] else when (kind) {
            Kind.Photo -> "${names.size} photos"
            Kind.Video -> "${names.size} videos"
            else -> "${names.size} files"
        }
        add(context, Entry(kind, label, null, now()))
    }

    fun clear(context: Context) {
        entries.value = emptyList()
        save(context, emptyList())
    }

    private fun add(context: Context, entry: Entry) {
        val list = (listOf(entry) + (entries(context).value ?: emptyList())).take(LIMIT)
        entries.value = list
        save(context, list)
    }

    private fun kindOf(name: String) = when (name.substringAfterLast('.', "").lowercase()) {
        "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "bmp" -> Kind.Photo
        "mp4", "mov", "mkv", "webm", "3gp" -> Kind.Video
        else -> Kind.File
    }

    private fun now() = System.currentTimeMillis()

    private fun load(context: Context): List<Entry> {
        val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("entries", null) ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            (0 until array.length()).map { i ->
                val o = array.getJSONObject(i)
                Entry(
                    Kind.valueOf(o.getString("kind")),
                    o.getString("label"),
                    o.optString("text").ifEmpty { null },
                    o.getLong("time"),
                )
            }
        }.getOrDefault(emptyList())
    }

    private fun save(context: Context, list: List<Entry>) {
        val array = JSONArray()
        list.forEach {
            array.put(
                JSONObject().put("kind", it.kind.name).put("label", it.label)
                    .put("text", it.text ?: "").put("time", it.time)
            )
        }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString("entries", array.toString()).apply()
    }
}

package io.github.joelcave42.hey

import android.content.Context
import android.net.Uri

/** The PC this phone is paired with, as learned from its pairing QR code. */
data class Pc(
    val id: String,
    val name: String,
    val port: Int,
    /** SHA-256 of the PC's TLS certificate. Hey only talks to a PC that presents exactly this one. */
    val fingerprint: String,
    /** Addresses to try first, most recent first. Found again over the network if they go stale. */
    val addresses: List<String>,
    val token: String = "",
) {
    companion object {
        private const val PREFS = "pc"

        /** Reads a `hey://pair?...` link. Returns the PC and its one-time pairing key. */
        fun fromPairingLink(link: String): Pair<Pc, String>? {
            val uri = Uri.parse(link.trim())
            if (uri.scheme != "hey" || uri.host != "pair") return null
            val q = { name: String -> uri.getQueryParameter(name).orEmpty() }
            val port = q("port").toIntOrNull() ?: return null
            if (q("id").isEmpty() || q("fp").isEmpty() || q("key").isEmpty()) return null
            val pc = Pc(
                id = q("id"),
                name = q("name").ifEmpty { "your PC" },
                port = port,
                fingerprint = q("fp"),
                addresses = q("ip").split(',').filter { it.isNotBlank() },
            )
            return pc to q("key")
        }

        fun load(context: Context): Pc? {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val token = p.getString("token", null) ?: return null
            return Pc(
                id = p.getString("id", "")!!,
                name = p.getString("name", "your PC")!!,
                port = p.getInt("port", 47820),
                fingerprint = p.getString("fp", "")!!,
                addresses = p.getString("ips", "")!!.split(',').filter { it.isNotBlank() },
                token = token,
            )
        }

        fun forget(context: Context) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().apply()
        }
    }

    fun save(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString("id", id)
            .putString("name", name)
            .putInt("port", port)
            .putString("fp", fingerprint)
            .putString("ips", addresses.joinToString(","))
            .putString("token", token)
            .apply()
    }
}

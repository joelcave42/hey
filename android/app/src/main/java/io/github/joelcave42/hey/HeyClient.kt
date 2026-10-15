package io.github.joelcave42.hey

import android.annotation.SuppressLint
import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.provider.Settings
import android.util.Base64
import org.json.JSONObject
import java.io.BufferedOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509TrustManager

/** Talks to Hey for Windows. Every call blocks, so run it off the main thread. */
class HeyClient(private val context: Context) {

    class HeyException(message: String) : Exception(message)

    /** Pairs with a PC from its QR code and saves it. Returns the saved PC. */
    fun pair(pc: Pc, key: String): Pc {
        val reply = request(pc, JSONObject().put("t", "pair").put("key", key).put("device", deviceName()))
        val paired = pc.copy(name = reply.optString("name", pc.name), token = reply.getString("token"))
        paired.save(context)
        return paired
    }

    fun sendText(pc: Pc, text: String) {
        request(pc, JSONObject().put("t", "text").put("token", pc.token).put("text", text))
    }

    /**
     * Sends one file. [index] and [count] place it in a batch so the PC shows
     * one notification for the lot. [open] is called once per attempt.
     */
    fun sendFile(
        pc: Pc,
        name: String,
        size: Long,
        index: Int,
        count: Int,
        open: () -> InputStream,
        progress: (sent: Long) -> Unit,
    ) {
        val body = JSONObject().put("t", "file").put("token", pc.token)
            .put("name", name).put("size", size).put("index", index).put("count", count)
        request(pc, body) { out ->
            open().use { input ->
                val buffer = ByteArray(64 * 1024)
                var sent = 0L
                while (sent < size) {
                    val n = input.read(buffer, 0, minOf(buffer.size.toLong(), size - sent).toInt())
                    if (n < 0) throw HeyException("$name got shorter while it was being sent.")
                    out.write(buffer, 0, n)
                    sent += n
                    progress(sent)
                }
            }
        }
    }

    fun ping(pc: Pc) {
        request(pc, JSONObject().put("t", "ping").put("token", pc.token))
    }

    /** Tries the PC's known addresses, then looks for it on the network if they've changed. */
    private fun request(pc: Pc, body: JSONObject, payload: ((OutputStream) -> Unit)? = null): JSONObject {
        for (address in pc.addresses) {
            try {
                return exchange(address, pc.port, pc.fingerprint, body, payload)
            } catch (e: HeyException) {
                throw e
            } catch (_: IOException) {
                // Try the next address.
            }
        }
        val found = discover(pc) ?: throw HeyException(
            "Can't find ${pc.name}. Make sure Hey is running on it and you're on the same Wi-Fi."
        )
        val reply = try {
            exchange(found.first, found.second, pc.fingerprint, body, payload)
        } catch (e: HeyException) {
            throw e
        } catch (_: IOException) {
            throw HeyException("Found ${pc.name} but couldn't connect. Check Windows Firewall allows Hey.")
        }
        if (pc.token.isNotEmpty()) {
            pc.copy(addresses = listOf(found.first) + (pc.addresses - found.first), port = found.second).save(context)
        }
        return reply
    }

    private fun exchange(
        host: String,
        port: Int,
        fingerprint: String,
        body: JSONObject,
        payload: ((OutputStream) -> Unit)?,
    ): JSONObject {
        val raw = Socket()
        raw.connect(InetSocketAddress(host, port), 1500)
        raw.soTimeout = 8000
        val tls = pinnedContext(fingerprint).socketFactory.createSocket(raw, host, port, true) as SSLSocket
        tls.use { socket ->
            // A different machine at this address fails the pin check here, and
            // request() moves on to the next address.
            socket.startHandshake()
            val out = BufferedOutputStream(socket.outputStream, 64 * 1024)
            out.write((body.toString() + "\n").toByteArray())
            if (payload != null) {
                // Once bytes are flowing, a dropped connection is a real failure,
                // not a reason to try another address.
                try {
                    payload(out)
                } catch (_: IOException) {
                    throw HeyException("Lost the connection to your PC while sending. Try again.")
                }
            }
            out.flush()
            val line = socket.inputStream.bufferedReader().readLine() ?: throw IOException("No reply")
            val reply = JSONObject(line)
            if (!reply.optBoolean("ok")) {
                throw HeyException(reply.optString("error", "Your PC said no."))
            }
            return reply
        }
    }

    /** Trusts exactly one certificate: the one whose hash was in the QR code. */
    @SuppressLint("CustomX509TrustManager", "TrustAllX509TrustManager")
    private fun pinnedContext(fingerprint: String): SSLContext {
        val pin = object : X509TrustManager {
            override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) =
                throw CertificateException("Not used")

            override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
                val hash = MessageDigest.getInstance("SHA-256").digest(chain[0].encoded)
                val actual = Base64.encodeToString(hash, Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)
                if (actual != fingerprint) throw CertificateException("Certificate doesn't match the pairing code")
            }

            override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
        }
        return SSLContext.getInstance("TLS").apply { init(null, arrayOf(pin), null) }
    }

    /** Finds the PC on the local network by the ID it advertises over mDNS. */
    private fun discover(pc: Pc): Pair<String, Int>? {
        val nsd = context.getSystemService(Context.NSD_SERVICE) as NsdManager
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        val lock = wifi.createMulticastLock("hey").apply { setReferenceCounted(false); acquire() }
        val done = CountDownLatch(1)
        var result: Pair<String, Int>? = null

        val listener = object : NsdManager.DiscoveryListener {
            override fun onServiceFound(info: NsdServiceInfo) {
                if (info.serviceName != "hey-${pc.id}") return
                @Suppress("DEPRECATION")
                nsd.resolveService(info, object : NsdManager.ResolveListener {
                    override fun onServiceResolved(resolved: NsdServiceInfo) {
                        @Suppress("DEPRECATION")
                        val host = resolved.host?.hostAddress
                        if (host != null && result == null) {
                            result = host to resolved.port
                            done.countDown()
                        }
                    }

                    override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {}
                })
            }

            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onServiceLost(info: NsdServiceInfo) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) = done.countDown()
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
        }

        try {
            nsd.discoverServices("_hey._tcp", NsdManager.PROTOCOL_DNS_SD, listener)
            done.await(5, TimeUnit.SECONDS)
        } finally {
            runCatching { nsd.stopServiceDiscovery(listener) }
            lock.release()
        }
        return result
    }

    private fun deviceName(): String =
        Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
            ?.takeIf { it.isNotBlank() } ?: Build.MODEL
}

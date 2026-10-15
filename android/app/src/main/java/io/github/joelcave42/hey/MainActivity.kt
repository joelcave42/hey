package io.github.joelcave42.hey

import android.Manifest
import android.app.StatusBarManager
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.drawable.Icon
import android.os.Build
import android.os.Bundle
import android.view.View
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.button.MaterialButton
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import kotlin.concurrent.thread

class MainActivity : AppCompatActivity() {

    private lateinit var pairedView: View
    private lateinit var unpairedView: View
    private lateinit var status: TextView
    private lateinit var statusDot: View
    private lateinit var message: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        pairedView = findViewById(R.id.paired)
        unpairedView = findViewById(R.id.unpaired)
        status = findViewById(R.id.status)
        statusDot = findViewById(R.id.status_dot)
        message = findViewById(R.id.message)

        findViewById<MaterialButton>(R.id.scan).setOnClickListener { scan() }
        findViewById<MaterialButton>(R.id.paste_link).setOnClickListener { pasteLink() }
        findViewById<MaterialButton>(R.id.send).setOnClickListener {
            sendToPc(this, message.text.toString()) { ok -> if (ok) message.text.clear() }
        }
        findViewById<MaterialButton>(R.id.send_clipboard).setOnClickListener { sendClipboard() }
        findViewById<MaterialButton>(R.id.add_tile).apply {
            visibility = if (Build.VERSION.SDK_INT >= 33) View.VISIBLE else View.GONE
            setOnClickListener { addTile() }
        }
        findViewById<MaterialButton>(R.id.unpair).setOnClickListener { unpair() }

        // For the progress bar while photos and files are sending.
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
    }

    override fun onResume() {
        super.onResume()
        refresh()
    }

    private fun refresh() {
        val pc = Pc.load(this)
        pairedView.visibility = if (pc != null) View.VISIBLE else View.GONE
        unpairedView.visibility = if (pc == null) View.VISIBLE else View.GONE
        if (pc == null) return

        findViewById<TextView>(R.id.pc_name).text = pc.name
        setStatus(null, "Checking…")
        thread {
            val error = try {
                HeyClient(this).ping(pc)
                null
            } catch (e: HeyClient.HeyException) {
                e.message
            } catch (e: Exception) {
                e.message ?: "Can't reach ${pc.name}"
            }
            runOnUiThread { setStatus(error == null, error ?: "Ready") }
        }
    }

    private fun setStatus(ok: Boolean?, text: String) {
        status.text = text
        statusDot.setBackgroundResource(
            when (ok) {
                true -> R.drawable.dot_ok
                false -> R.drawable.dot_error
                null -> R.drawable.dot_idle
            }
        )
    }

    private fun scan() {
        val options = GmsBarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .build()
        GmsBarcodeScanning.getClient(this, options).startScan()
            .addOnSuccessListener { barcode -> pairWith(barcode.rawValue.orEmpty()) }
            .addOnFailureListener { e -> toast("Couldn't open the scanner: ${e.message}") }
    }

    private fun pasteLink() {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val text = clipboard.primaryClip?.takeIf { it.itemCount > 0 }
            ?.getItemAt(0)?.coerceToText(this)?.toString().orEmpty()
        if (!text.trim().startsWith("hey://pair")) {
            toast("Copy the pairing link from your PC first. It's under \"Can't scan?\" on the pairing page.")
            return
        }
        pairWith(text)
    }

    private fun pairWith(link: String) {
        val (pc, key) = Pc.fromPairingLink(link) ?: run {
            toast("That isn't a Hey pairing code.")
            return
        }
        setStatus(null, "Pairing with ${pc.name}…")
        thread {
            val error = try {
                HeyClient(this).pair(pc, key)
                null
            } catch (e: HeyClient.HeyException) {
                e.message
            } catch (e: Exception) {
                "Couldn't pair: ${e.message}"
            }
            runOnUiThread {
                if (error != null) toast(error) else toast("Paired with ${pc.name}")
                refresh()
            }
        }
    }

    private fun sendClipboard() {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val text = clipboard.primaryClip?.takeIf { it.itemCount > 0 }
            ?.getItemAt(0)?.coerceToText(this)?.toString().orEmpty()
        if (text.isBlank()) toast("Your clipboard is empty.") else sendToPc(this, text)
    }

    private fun addTile() {
        if (Build.VERSION.SDK_INT < 33) return
        getSystemService(StatusBarManager::class.java).requestAddTileService(
            ComponentName(this, HeyTileService::class.java),
            getString(R.string.tile_label),
            Icon.createWithResource(this, R.drawable.ic_tile),
            mainExecutor,
        ) { }
    }

    private fun unpair() {
        val pc = Pc.load(this) ?: return
        MaterialAlertDialogBuilder(this)
            .setTitle("Unpair from ${pc.name}?")
            .setMessage("You'll need to scan a new pairing code to send to it again.")
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Unpair") { _, _ ->
                Pc.forget(this)
                refresh()
            }
            .show()
    }

    private fun toast(text: String) = Toast.makeText(this, text, Toast.LENGTH_LONG).show()
}

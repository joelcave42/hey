package io.github.joelcave42.hey

import android.Manifest
import android.app.StatusBarManager
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LifecycleResumeEffect
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import io.github.joelcave42.hey.ui.HeyTheme
import io.github.joelcave42.hey.ui.HomeActions
import io.github.joelcave42.hey.ui.HomeScreen
import io.github.joelcave42.hey.ui.PairScreen
import io.github.joelcave42.hey.ui.Status
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        // For the progress bar while photos and files are sending.
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }

        setContent { HeyTheme { HeyApp(this) } }
    }
}

@Composable
private fun HeyApp(activity: ComponentActivity) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var pc by remember { mutableStateOf(Pc.load(context)) }
    var status by remember { mutableStateOf<Status>(Status.Checking) }
    var pairing by remember { mutableStateOf(false) }
    val history by History.entries(context).collectAsState()
    val receiveConnected by ReceiveService.connected.collectAsState()
    var receiving by remember { mutableStateOf(ReceiveService.isEnabled(context)) }

    fun say(message: String) = scope.launch { snackbar.showSnackbar(message) }

    fun check() {
        val current = pc ?: return
        status = Status.Checking
        scope.launch {
            status = withContext(Dispatchers.IO) {
                try {
                    HeyClient(context).ping(current)
                    Status.Ready
                } catch (e: Exception) {
                    Status.Problem(e.message ?: "Can't reach ${current.label}")
                }
            }
            (status as? Status.Problem)?.let { say(it.message) }
        }
    }

    fun pairWith(link: String) {
        val (newPc, key) = Pc.fromPairingLink(link) ?: run {
            say("That isn't a Hey pairing code.")
            return
        }
        pairing = true
        scope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { HeyClient(context).pair(newPc, key) } }
            pairing = false
            result.onSuccess {
                pc = it
                say("Paired with ${it.label}")
            }.onFailure { say(it.message ?: "Couldn't pair.") }
        }
    }

    LifecycleResumeEffect(pc?.token) {
        check()
        ReceiveService.start(context)
        onPauseOrDispose { }
    }

    val pickPhotos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(50)) { uris ->
        sendFiles(context, uris)
    }
    val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        sendFiles(context, uris)
    }

    val current = pc
    if (current == null) {
        PairScreen(
            pairing = pairing,
            snackbar = snackbar,
            onScan = {
                val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
                GmsBarcodeScanning.getClient(activity, options).startScan()
                    .addOnSuccessListener { pairWith(it.rawValue.orEmpty()) }
                    .addOnFailureListener { say("Couldn't open the scanner: ${it.message}") }
            },
            onPasteLink = {
                val text = clipboardText(context)
                if (text.trim().startsWith("hey://pair")) pairWith(text)
                else say("Copy the pairing link on your PC first. It's under \"Can't scan?\".")
            },
        )
        return
    }

    HomeScreen(
        pc = current,
        status = status,
        history = history.orEmpty(),
        receiving = receiving,
        receiveConnected = receiveConnected,
        snackbar = snackbar,
        actions = HomeActions(
            retry = ::check,
            sendClipboard = {
                val text = clipboardText(context)
                if (text.isBlank()) say("Your clipboard is empty. Copy something first.")
                else SendService.startText(context, text)
            },
            pickPhotos = {
                pickPhotos.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo))
            },
            pickFiles = { pickFiles.launch(arrayOf("*/*")) },
            sendText = { SendService.startText(context, it) },
            resend = { entry -> entry.text?.let { SendService.startText(context, it) } },
            rename = { name ->
                pc = current.copy(nickname = name).also { it.save(context) }
            },
            addTile = if (Build.VERSION.SDK_INT >= 33) {
                { addTile(activity) }
            } else null,
            clearHistory = { History.clear(context) },
            unpair = {
                ReceiveService.setEnabled(context, false)
                Pc.forget(context)
                ReceiveService.setEnabled(context, true)
                pc = null
            },
            setReceiving = { on ->
                receiving = on
                ReceiveService.setEnabled(context, on)
            },
        ),
    )
}

private fun sendFiles(context: Context, uris: List<Uri>) {
    if (uris.isNotEmpty()) SendService.start(context, uris)
}

private fun clipboardText(context: Context): String {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    return clipboard.primaryClip?.takeIf { it.itemCount > 0 }
        ?.getItemAt(0)?.coerceToText(context)?.toString().orEmpty()
}

private fun addTile(activity: ComponentActivity) {
    if (Build.VERSION.SDK_INT < 33) return
    activity.getSystemService(StatusBarManager::class.java).requestAddTileService(
        ComponentName(activity, HeyTileService::class.java),
        activity.getString(R.string.tile_label),
        Icon.createWithResource(activity, R.drawable.ic_tile),
        activity.mainExecutor,
    ) { }
}

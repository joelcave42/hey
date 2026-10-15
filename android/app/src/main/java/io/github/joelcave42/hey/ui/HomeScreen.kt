package io.github.joelcave42.hey.ui

import android.text.format.DateUtils
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.automirrored.rounded.AddToHomeScreen
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.rounded.Description
import androidx.compose.material.icons.rounded.DesktopWindows
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material.icons.rounded.Image
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.LinkOff
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Movie
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.automirrored.rounded.Notes
import androidx.compose.material.icons.rounded.PhotoLibrary
import androidx.compose.material.icons.rounded.Replay
import androidx.compose.material.icons.rounded.DeleteSweep
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.joelcave42.hey.History
import io.github.joelcave42.hey.Pc
import io.github.joelcave42.hey.R

sealed interface Status {
    data object Checking : Status
    data object Ready : Status
    data class Problem(val message: String) : Status
}

class HomeActions(
    val retry: () -> Unit,
    val sendClipboard: () -> Unit,
    val pickPhotos: () -> Unit,
    val pickFiles: () -> Unit,
    val sendText: (String) -> Unit,
    val resend: (History.Entry) -> Unit,
    val rename: (String) -> Unit,
    val addTile: (() -> Unit)?,
    val clearHistory: () -> Unit,
    val unpair: () -> Unit,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    pc: Pc,
    status: Status,
    history: List<History.Entry>,
    snackbar: SnackbarHostState,
    actions: HomeActions,
) {
    var typing by rememberSaveable { mutableStateOf(false) }
    var renaming by rememberSaveable { mutableStateOf(false) }
    var confirmUnpair by rememberSaveable { mutableStateOf(false) }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopBar(
                onRename = { renaming = true },
                addTile = actions.addTile,
                clearHistory = actions.clearHistory.takeIf { history.isNotEmpty() },
                onUnpair = { confirmUnpair = true },
            )
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(
                start = 20.dp,
                end = 20.dp,
                top = padding.calculateTopPadding() + 4.dp,
                bottom = padding.calculateBottomPadding() + 24.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { PcCard(pc, status, onClick = actions.retry) }
            item { Spacer(Modifier.height(4.dp)) }
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    ActionTile(Icons.Rounded.ContentPaste, "Clipboard", "What you copied", Accent.Clipboard, Modifier.weight(1f), actions.sendClipboard)
                    ActionTile(Icons.Rounded.PhotoLibrary, "Photos", "From your gallery", Accent.Photos, Modifier.weight(1f), actions.pickPhotos)
                }
            }
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    ActionTile(Icons.Rounded.UploadFile, "Files", "Any file, any size", Accent.Files, Modifier.weight(1f), actions.pickFiles)
                    ActionTile(Icons.Rounded.EditNote, "Type", "A quick note", Accent.Type, Modifier.weight(1f)) { typing = true }
                }
            }
            item {
                Text(
                    "Recent",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(top = 16.dp, bottom = 2.dp, start = 4.dp),
                )
            }
            if (history.isEmpty()) {
                item { EmptyRecent() }
            } else {
                items(history, key = { it.time }) { entry -> RecentRow(entry, actions.resend) }
            }
        }
    }

    if (typing) TypeSheet(onDismiss = { typing = false }, onSend = { actions.sendText(it); typing = false })
    if (renaming) RenameDialog(pc.label, onDismiss = { renaming = false }, onRename = { actions.rename(it); renaming = false })
    if (confirmUnpair) {
        AlertDialog(
            onDismissRequest = { confirmUnpair = false },
            title = { Text("Unpair from ${pc.label}?") },
            text = { Text("You'll need to scan a new pairing code to send to it again.") },
            confirmButton = {
                TextButton(onClick = { confirmUnpair = false; actions.unpair() }) {
                    Text("Unpair", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { confirmUnpair = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun TopBar(onRename: () -> Unit, addTile: (() -> Unit)?, clearHistory: (() -> Unit)?, onUnpair: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(start = 20.dp, end = 8.dp, top = 12.dp, bottom = 12.dp),
    ) {
        Icon(
            painterResource(R.drawable.logo),
            contentDescription = null,
            tint = Color.Unspecified,
            modifier = Modifier.size(34.dp),
        )
        Spacer(Modifier.width(12.dp))
        Text("Hey", fontSize = 28.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
        Box {
            IconButton(onClick = { menu = true }) { Icon(Icons.Rounded.MoreVert, "More") }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(
                    text = { Text("Rename PC") },
                    leadingIcon = { Icon(Icons.Rounded.Edit, null) },
                    onClick = { menu = false; onRename() },
                )
                if (addTile != null) {
                    DropdownMenuItem(
                        text = { Text("Add quick settings tile") },
                        leadingIcon = { Icon(Icons.AutoMirrored.Rounded.AddToHomeScreen, null) },
                        onClick = { menu = false; addTile() },
                    )
                }
                if (clearHistory != null) {
                    DropdownMenuItem(
                        text = { Text("Clear recent") },
                        leadingIcon = { Icon(Icons.Rounded.DeleteSweep, null) },
                        onClick = { menu = false; clearHistory() },
                    )
                }
                DropdownMenuItem(
                    text = { Text("Unpair", color = MaterialTheme.colorScheme.error) },
                    leadingIcon = { Icon(Icons.Rounded.LinkOff, null, tint = MaterialTheme.colorScheme.error) },
                    onClick = { menu = false; onUnpair() },
                )
            }
        }
    }
}

@Composable
private fun PcCard(pc: Pc, status: Status, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(28.dp))
            .background(BrandGradient)
            .clickable(onClick = onClick)
            .padding(20.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .size(56.dp)
                    .clip(RoundedCornerShape(18.dp))
                    .background(Color.White.copy(alpha = 0.18f)),
            ) {
                Icon(Icons.Rounded.DesktopWindows, null, tint = Color.White, modifier = Modifier.size(30.dp))
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    pc.label,
                    color = Color.White,
                    fontSize = 21.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(4.dp))
                AnimatedContent(targetState = status, label = "status") { s -> StatusLine(s) }
            }
        }
    }
}

@Composable
private fun StatusLine(status: Status) {
    val (color, text) = when (status) {
        Status.Checking -> Accent.Warn to "Looking for your PC…"
        Status.Ready -> Accent.Good to "Connected"
        is Status.Problem -> Accent.Bad to "Can't reach it · tap to retry"
    }
    val pulse = rememberInfiniteTransition(label = "pulse")
    val glow by pulse.animateFloat(
        initialValue = 1f,
        targetValue = if (status == Status.Ready) 0.35f else 1f,
        animationSpec = infiniteRepeatable(tween(1100), RepeatMode.Reverse),
        label = "glow",
    )
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .clip(CircleShape)
            .background(Color.Black.copy(alpha = 0.22f))
            .padding(start = 10.dp, end = 12.dp, top = 5.dp, bottom = 5.dp),
    ) {
        Box(contentAlignment = Alignment.Center) {
            // A soft halo that breathes behind a solid dot while connected.
            Box(
                Modifier
                    .size(16.dp)
                    .alpha(1f - glow)
                    .clip(CircleShape)
                    .background(color.copy(alpha = 0.5f)),
            )
            Box(
                Modifier
                    .size(9.dp)
                    .clip(CircleShape)
                    .background(color),
            )
        }
        Spacer(Modifier.width(6.dp))
        Text(text, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun ActionTile(
    icon: ImageVector,
    label: String,
    hint: String,
    color: Color,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) 0.95f else 1f, label = "press")
    Surface(
        onClick = onClick,
        interactionSource = interaction,
        shape = RoundedCornerShape(26.dp),
        color = MaterialTheme.colorScheme.surfaceContainer,
        modifier = modifier
            .scale(scale)
            .aspectRatio(1.08f),
    ) {
        Column(Modifier.padding(18.dp)) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .size(48.dp)
                    .clip(RoundedCornerShape(16.dp))
                    .background(color.copy(alpha = 0.18f)),
            ) {
                Icon(icon, null, tint = color, modifier = Modifier.size(26.dp))
            }
            Spacer(Modifier.weight(1f))
            Text(label, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Text(
                hint,
                fontSize = 13.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun RecentRow(entry: History.Entry, onResend: (History.Entry) -> Unit) {
    val (icon, color) = when (entry.kind) {
        History.Kind.Link -> Icons.Rounded.Link to Accent.Clipboard
        History.Kind.Text -> Icons.AutoMirrored.Rounded.Notes to Accent.Type
        History.Kind.Code -> Icons.Rounded.Key to Accent.Good
        History.Kind.Photo -> Icons.Rounded.Image to Accent.Photos
        History.Kind.Video -> Icons.Rounded.Movie to Accent.Photos
        History.Kind.File -> Icons.Rounded.Description to Accent.Files
    }
    Surface(shape = RoundedCornerShape(20.dp), color = MaterialTheme.colorScheme.surfaceContainerLow) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, end = 4.dp, top = 10.dp, bottom = 10.dp),
        ) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .size(40.dp)
                    .clip(RoundedCornerShape(13.dp))
                    .background(color.copy(alpha = 0.16f)),
            ) {
                Icon(icon, null, tint = color, modifier = Modifier.size(21.dp))
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(entry.label, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    timeAgo(entry.time),
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (entry.text != null) {
                IconButton(onClick = { onResend(entry) }) {
                    Icon(Icons.Rounded.Replay, "Send again", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

@Composable
private fun EmptyRecent() {
    Surface(shape = RoundedCornerShape(20.dp), color = MaterialTheme.colorScheme.surfaceContainerLow) {
        Column(Modifier.fillMaxWidth().padding(20.dp)) {
            Text("Nothing sent yet", fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(4.dp))
            Text(
                "Tap a button above, or in any app tap Share → Hey.",
                fontSize = 14.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TypeSheet(onDismiss: () -> Unit, onSend: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(
            Modifier
                .padding(horizontal = 20.dp)
                .padding(bottom = 16.dp)
                .navigationBarsPadding()
                .imePadding(),
        ) {
            Text("Send to your PC", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                placeholder = { Text("Type or paste a link, a note, anything") },
                shape = RoundedCornerShape(18.dp),
                minLines = 3,
                maxLines = 8,
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(focus),
            )
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = { onSend(text) },
                enabled = text.isNotBlank(),
                shape = RoundedCornerShape(18.dp),
                modifier = Modifier
                    .fillMaxWidth()
                    .height(56.dp),
            ) {
                Icon(Icons.AutoMirrored.Rounded.Send, null)
                Spacer(Modifier.width(10.dp))
                Text("Send", fontSize = 16.sp)
            }
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
}

@Composable
private fun RenameDialog(current: String, onDismiss: () -> Unit, onRename: (String) -> Unit) {
    var name by rememberSaveable { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Rename PC") },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(40) },
                singleLine = true,
                placeholder = { Text("Like \"Desk PC\"") },
                shape = RoundedCornerShape(14.dp),
            )
        },
        confirmButton = { TextButton(onClick = { onRename(name.trim()) }) { Text("Save") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

private fun timeAgo(time: Long): String {
    val now = System.currentTimeMillis()
    if (now - time < DateUtils.MINUTE_IN_MILLIS) return "Just now"
    return DateUtils.getRelativeTimeSpanString(time, now, DateUtils.MINUTE_IN_MILLIS).toString()
}

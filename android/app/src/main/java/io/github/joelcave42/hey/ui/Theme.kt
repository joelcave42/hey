package io.github.joelcave42.hey.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

val BrandStart = Color(0xFF6D5DFC)
val BrandEnd = Color(0xFF3B82F6)
val BrandGradient = Brush.linearGradient(listOf(BrandStart, BrandEnd))

/** One colour per action tile, so each is easy to find at a glance. */
object Accent {
    val Clipboard = Color(0xFF8B7DFF)
    val Photos = Color(0xFFFF6B9A)
    val Files = Color(0xFF2EC4B6)
    val Type = Color(0xFFFFB547)
    val Good = Color(0xFF3DDC84)
    val Warn = Color(0xFFFFC145)
    val Bad = Color(0xFFFF5C5C)
}

private val Dark = darkColorScheme(
    primary = Color(0xFF8B7DFF),
    onPrimary = Color.White,
    background = Color(0xFF0E0F14),
    onBackground = Color(0xFFF2F2F6),
    surface = Color(0xFF0E0F14),
    onSurface = Color(0xFFF2F2F6),
    surfaceContainerLow = Color(0xFF14151C),
    surfaceContainer = Color(0xFF181922),
    surfaceContainerHigh = Color(0xFF20212C),
    surfaceContainerHighest = Color(0xFF282A37),
    onSurfaceVariant = Color(0xFF9A9CAB),
    outlineVariant = Color(0xFF2A2B36),
    error = Color(0xFFFF6B6B),
)

private val Light = lightColorScheme(
    primary = Color(0xFF5B5BF6),
    onPrimary = Color.White,
    background = Color(0xFFF5F5FA),
    onBackground = Color(0xFF14151A),
    surface = Color(0xFFF5F5FA),
    onSurface = Color(0xFF14151A),
    surfaceContainerLow = Color(0xFFFAFAFD),
    surfaceContainer = Color(0xFFFFFFFF),
    surfaceContainerHigh = Color(0xFFEDEDF5),
    surfaceContainerHighest = Color(0xFFE4E4EE),
    onSurfaceVariant = Color(0xFF5D6070),
    outlineVariant = Color(0xFFE4E4EC),
    error = Color(0xFFD93636),
)

@Composable
fun HeyTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}

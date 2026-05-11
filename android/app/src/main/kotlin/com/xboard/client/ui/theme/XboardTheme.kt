package com.xboard.client.ui.theme

import android.os.Build
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

// Brand seeds — kept in lockstep with res/values/colors.xml's
// brand_primary / brand_secondary / brand_tertiary so XML-rendered
// icons match the Compose theme.

private val BrandPrimary = Color(0xFF7C5CFF)
private val BrandSecondary = Color(0xFF00C48C)
private val BrandTertiary = Color(0xFFFF6380)
private val ErrorColor = Color(0xFFFF4057)

private val LightColors = lightColorScheme(
    primary = BrandPrimary,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFE0E7FF),
    onPrimaryContainer = Color(0xFF1E1B4B),
    secondary = BrandSecondary,
    onSecondary = Color.White,
    tertiary = BrandTertiary,
    onTertiary = Color.White,
    background = Color(0xFFFAFAFA),
    onBackground = Color(0xFF111827),
    surface = Color.White,
    onSurface = Color(0xFF111827),
    surfaceVariant = Color(0xFFF3F4F6),
    onSurfaceVariant = Color(0xFF4B5563),
    error = ErrorColor,
    onError = Color.White,
)

private val DarkColors = darkColorScheme(
    primary = BrandPrimary,
    onPrimary = Color.White,
    primaryContainer = Color(0xFF34265E),
    onPrimaryContainer = Color(0xFFF0E9FF),
    secondary = BrandSecondary,
    onSecondary = Color(0xFF041B16),
    tertiary = BrandTertiary,
    onTertiary = Color.White,
    background = Color(0xFF15121F),
    onBackground = Color(0xFFF8F7FF),
    surface = Color(0xFF211C2D),
    onSurface = Color(0xFFF8F7FF),
    surfaceVariant = Color(0xFF2D273B),
    onSurfaceVariant = Color(0xFFBDB6CC),
    error = ErrorColor,
    onError = Color.White,
)

/**
 * Theme entry point. Honours system dark mode by default; when the
 * device is on Android 12+ (API 31+) dynamic colors are pulled from the
 * wallpaper instead of the brand palette — this is an OS standard so
 * we follow it.
 */
@Composable
fun XboardTheme(
    darkTheme: Boolean = true,
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit,
) {
    val colors = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val ctx = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        }
        darkTheme -> DarkColors
        else -> LightColors
    }
    MaterialTheme(
        colorScheme = colors,
        typography = MaterialTheme.typography,
        content = content,
    )
}

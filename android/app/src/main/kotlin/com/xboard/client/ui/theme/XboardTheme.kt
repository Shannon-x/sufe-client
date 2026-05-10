package com.xboard.client.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
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

private val BrandPrimary = Color(0xFF4F46E5)
private val BrandSecondary = Color(0xFF06B6D4)
private val BrandTertiary = Color(0xFFF59E0B)
private val ErrorColor = Color(0xFFDC2626)

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
    primary = Color(0xFFA5B4FC),
    onPrimary = Color(0xFF1E1B4B),
    primaryContainer = Color(0xFF312E81),
    onPrimaryContainer = Color(0xFFE0E7FF),
    secondary = Color(0xFF67E8F9),
    onSecondary = Color(0xFF083344),
    tertiary = Color(0xFFFBBF24),
    onTertiary = Color(0xFF422006),
    background = Color(0xFF0B1020),
    onBackground = Color(0xFFE5E7EB),
    surface = Color(0xFF111827),
    onSurface = Color(0xFFE5E7EB),
    surfaceVariant = Color(0xFF1F2937),
    onSurfaceVariant = Color(0xFFD1D5DB),
    error = Color(0xFFFCA5A5),
    onError = Color(0xFF7F1D1D),
)

/**
 * Theme entry point. Honours system dark mode by default; when the
 * device is on Android 12+ (API 31+) dynamic colors are pulled from the
 * wallpaper instead of the brand palette — this is an OS standard so
 * we follow it.
 */
@Composable
fun XboardTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = true,
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

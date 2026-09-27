package ru.teivrim.anime.ui.theme

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import ru.teivrim.anime.data.ThemeMode

/**
 * Colours and type shared with the web frontend, so the app and the site look
 * like the same product.
 *
 * The accent is deliberately the same red as the web build. Material 3 wants a
 * full tonal palette; rather than hand-authoring one, the accent is used for
 * both the primary role and the surfaces it sits on, which keeps the number of
 * near-identical reds down to something a person can reason about.
 */

private val Accent = Color(0xFFFF4757)
private val AccentDark = Color(0xFFE03847)
private val Link = Color(0xFF6EA8FF)

private val DarkColors = darkColorScheme(
    primary = Accent,
    onPrimary = Color.White,
    primaryContainer = Color(0xFF3A1016),
    onPrimaryContainer = Color(0xFFFFD9DD),
    secondary = Link,
    onSecondary = Color(0xFF00203F),
    background = Color(0xFF0D0D10),
    onBackground = Color(0xFFECECF1),
    surface = Color(0xFF0D0D10),
    onSurface = Color(0xFFECECF1),
    surfaceVariant = Color(0xFF1E1E25),
    onSurfaceVariant = Color(0xFF9A9AAB),
    surfaceContainer = Color(0xFF16161B),
    surfaceContainerHigh = Color(0xFF1E1E25),
    surfaceContainerHighest = Color(0xFF26262E),
    outline = Color(0xFF2A2A33),
    outlineVariant = Color(0xFF3A3A46),
    error = Accent,
    onError = Color.White,
)

private val LightColors = lightColorScheme(
    primary = AccentDark,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFFFD9DD),
    onPrimaryContainer = Color(0xFF3A0010),
    secondary = Color(0xFF2563EB),
    onSecondary = Color.White,
    background = Color(0xFFF6F6F9),
    onBackground = Color(0xFF16161D),
    surface = Color(0xFFF6F6F9),
    onSurface = Color(0xFF16161D),
    surfaceVariant = Color(0xFFEDEDF2),
    onSurfaceVariant = Color(0xFF5C5C6E),
    surfaceContainer = Color(0xFFFFFFFF),
    surfaceContainerHigh = Color(0xFFFFFFFF),
    surfaceContainerHighest = Color(0xFFF0F0F5),
    outline = Color(0xFFE0E0E8),
    outlineVariant = Color(0xFFC8C8D4),
    error = AccentDark,
    onError = Color.White,
)

private val AppTypography = Typography(
    titleLarge = TextStyle(fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.Bold),
    titleMedium = TextStyle(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold),
    titleSmall = TextStyle(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold),
    bodyLarge = TextStyle(fontSize = 15.sp, lineHeight = 22.sp),
    bodyMedium = TextStyle(fontSize = 14.sp, lineHeight = 20.sp),
    bodySmall = TextStyle(fontSize = 12.5.sp, lineHeight = 17.sp),
    labelLarge = TextStyle(fontSize = 14.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold),
    labelMedium = TextStyle(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium),
    labelSmall = TextStyle(fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Medium),
)

/** 2:3 poster, the aspect every source uses. */
object Dimens {
    val cardCorner = 12.dp
    val gridGap = 12.dp
    val screenPadding = 16.dp
    val cardMinWidth = 148.dp
}

@Composable
fun AnimeTheme(
    themeMode: ThemeMode,
    content: @Composable () -> Unit,
) {
    val dark = when (themeMode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Dark -> true
        ThemeMode.Light -> false
    }
    val colors = if (dark) DarkColors else LightColors

    val view = LocalView.current
    if (!view.isInEditMode) {
        val context = LocalContext.current
        SideEffect {
            (context as? Activity)?.window?.let { window ->
                WindowCompat.getInsetsController(window, view).apply {
                    isAppearanceLightStatusBars = !dark
                    isAppearanceLightNavigationBars = !dark
                }
            }
        }
    }

    MaterialTheme(
        colorScheme = colors,
        typography = AppTypography,
        content = content,
    )
}

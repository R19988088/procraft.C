package io.github.artcraftandroid.procraft.ui

import android.content.res.Configuration
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsIgnoringVisibility
import androidx.compose.foundation.layout.union
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.em
import io.github.artcraftandroid.procraft.core.Plugins

/** Touches leave no ripple: the controls show presses in their own ways, or not at all. */
private object NoIndication : androidx.compose.foundation.IndicationNodeFactory {
    override fun create(interactionSource: androidx.compose.foundation.interaction.InteractionSource): androidx.compose.ui.node.DelegatableNode =
        object : androidx.compose.ui.Modifier.Node() {}

    override fun hashCode(): Int = -1

    override fun equals(other: Any?): Boolean = other === this
}

/** Letter spacing that brings the bundled face to the measured text widths (over 18–24 pt labels). */
private const val TRACKING = -0.045f

/**
 * The app's theme: Inter (bundled in `assets/fonts`, OFL) for all text, with natural line heights
 * and no font padding, so text sits where the measured labels do. Interface text keeps its size
 * whatever the system font scale: sp equal dp inside the theme.
 */
@Composable
fun ProCraftTheme(interfaceSize: Float = 1f, clear: Clearance = Clearance(), content: @Composable () -> Unit) {
    val assets = LocalContext.current.assets
    val text = remember(assets) {
        val family = FontFamily(
            Font("fonts/Inter-Regular.ttf", assets, FontWeight.Normal),
            Font("fonts/Inter-Medium.ttf", assets, FontWeight.Medium),
            Font("fonts/Inter-SemiBold.ttf", assets, FontWeight.SemiBold),
            Font("fonts/Inter-Bold.ttf", assets, FontWeight.Bold),
        )
        TextStyle(fontFamily = family, letterSpacing = TRACKING.em, platformStyle = PlatformTextStyle(includeFontPadding = false))
    }
    val typography = remember(text) {
        val t = Typography()
        Typography(
            displayLarge = t.displayLarge.merge(text),
            displayMedium = t.displayMedium.merge(text),
            displaySmall = t.displaySmall.merge(text),
            headlineLarge = t.headlineLarge.merge(text),
            headlineMedium = t.headlineMedium.merge(text),
            headlineSmall = t.headlineSmall.merge(text),
            titleLarge = t.titleLarge.merge(text),
            titleMedium = t.titleMedium.merge(text),
            titleSmall = t.titleSmall.merge(text),
            bodyLarge = t.bodyLarge.merge(text),
            bodyMedium = t.bodyMedium.merge(text),
            bodySmall = t.bodySmall.merge(text),
            labelLarge = t.labelLarge.merge(text),
            labelMedium = t.labelMedium.merge(text),
            labelSmall = t.labelSmall.merge(text),
        )
    }
    MaterialTheme(colorScheme = darkColorScheme(primary = Pc.Accent), typography = typography) {
        val density = LocalDensity.current
        val band = remember { BandColour() }
        CompositionLocalProvider(
            LocalDensity provides Density(density.density * interfaceSize, 1f),
            LocalConfiguration provides sizedScreen(interfaceSize, clear, density.density),
            LocalSizeSettled provides sizeSettled(interfaceSize),
            LocalTextStyle provides text,
            LocalClearance provides clear,
            LocalBandColour provides band,
            androidx.compose.foundation.LocalIndication provides NoIndication,
        ) {
            // the interface inside what it keeps clear of, the rest in the colour of what is under it
            // (the top bar's, or a screen's own: see BandColour); with nothing to keep
            // clear of, the whole window, each part of it measured to it as the window's root does
            val inside = with(LocalDensity.current) {
                androidx.compose.foundation.layout.PaddingValues(clear.left.toDp(), clear.top.toDp(), clear.right.toDp(), clear.bottom.toDp())
            }
            androidx.compose.foundation.layout.Box(
                androidx.compose.ui.Modifier.fillMaxSize()
                    .then(if (clear.none) androidx.compose.ui.Modifier else androidx.compose.ui.Modifier.background(band.colour ?: Pc.Bar.copy(alpha = 1f)))
                    .padding(inside),
                propagateMinConstraints = true,
            ) { content() }
        }
    }
}

/** Moves a frame after the interface's size does: see [sizeSettled]. */
private val LocalSizeSettled = staticCompositionLocalOf { 0 }

/**
 * Counts the sizes the interface has settled at. A BoxWithConstraints gives its content the size
 * in dp through the density of its last measuring, and as the window stays as it is nothing
 * composes it again after that: it would go on with the old size until the next turn of the
 * screen. Changing a static local a frame on, measured by then, has all of it read the new one.
 */
@Composable
private fun sizeSettled(interfaceSize: Float): Int {
    var settled by remember { mutableIntStateOf(0) }
    var last by remember { mutableFloatStateOf(interfaceSize) }
    LaunchedEffect(interfaceSize) {
        if (interfaceSize != last) {
            last = interfaceSize
            withFrameNanos { }
            settled++
        }
    }
    return settled
}

/**
 * How far in from the window's edges the interface is laid out (pixels), to keep clear of the status
 * bar and the screen's cutouts, such as a camera's, while a plugin asks it to. Everything goes
 * inside, the canvas too, so a place in the window is one in the interface less its [corner].
 */
@androidx.compose.runtime.Immutable
data class Clearance(val left: Int = 0, val top: Int = 0, val right: Int = 0, val bottom: Int = 0) {
    /** Where the interface's top left is in the window. */
    val corner: androidx.compose.ui.geometry.Offset get() = androidx.compose.ui.geometry.Offset(left.toFloat(), top.toFloat())

    /** Whether the interface takes the whole window. */
    val none: Boolean get() = left == 0 && top == 0 && right == 0 && bottom == 0
}

/** What the interface keeps clear of, inside the theme: see [Clearance]. */
val LocalClearance = staticCompositionLocalOf { Clearance() }

/**
 * The colour of the band the interface keeps clear of (see [Clearance]): the top bar's own while
 * the canvas shows, or the colour a screen of its own gives with [ClearBandShows] (the gallery's
 * background), so the band and what is under it read as one.
 */
class BandColour {
    var colour by androidx.compose.runtime.mutableStateOf<androidx.compose.ui.graphics.Color?>(null)
}

/** The band's colour, inside the theme: see [BandColour]. */
val LocalBandColour = staticCompositionLocalOf { BandColour() }

/** While this is composed, the band kept clear of shows [colour] (a screen's own background). */
@Composable
fun ClearBandShows(colour: androidx.compose.ui.graphics.Color) {
    val band = LocalBandColour.current
    androidx.compose.runtime.DisposableEffect(band, colour) {
        band.colour = colour
        onDispose { if (band.colour == colour) band.colour = null }
    }
}

/**
 * What the interface keeps clear of with [plugins] as they are ([changes] when they change): the
 * status bar's band, kept whether the bar shows or not (so a swipe showing it for a moment moves
 * nothing), and the screen's cutouts, while a plugin asks; nothing otherwise.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun clearance(plugins: Plugins, changes: Any? = null): Clearance {
    val clear = remember(plugins, changes) { plugins.keepClear() }
    if (!clear) return Clearance()
    val density = LocalDensity.current
    val ltr = androidx.compose.ui.unit.LayoutDirection.Ltr
    val insets = WindowInsets.statusBarsIgnoringVisibility.union(WindowInsets.displayCutout)
    return Clearance(insets.getLeft(density, ltr), insets.getTop(density), insets.getRight(density, ltr), insets.getBottom(density))
}

/** How big the interface shows with [chosen] set: as far as the window, less what it keeps [clear] of, has room. */
@Composable
fun fittedInterfaceSize(plugins: Plugins, changes: Any? = null, clear: Clearance = Clearance()): Float {
    val screen = LocalConfiguration.current
    val density = LocalDensity.current.density
    return remember(plugins, changes, screen.screenWidthDp, screen.screenHeightDp, clear, density) {
        plugins.interfaceScale(screen.screenWidthDp - (clear.left + clear.right) / density, screen.screenHeightDp - (clear.top + clear.bottom) / density)
    }
}

/**
 * The screen as the interface measures it at [interfaceSize]: as many of its dp as fit across it,
 * less what it keeps [clear] of ([density]: the screen's own).
 */
@Composable
private fun sizedScreen(interfaceSize: Float, clear: Clearance, density: Float): Configuration {
    val screen = LocalConfiguration.current
    return remember(screen, interfaceSize, clear, density) {
        if (interfaceSize == 1f && clear.none) {
            screen
        } else {
            val across = (clear.left + clear.right) / density
            val down = (clear.top + clear.bottom) / density
            Configuration(screen).apply {
                screenWidthDp = ((screenWidthDp - across) / interfaceSize).toInt()
                screenHeightDp = ((screenHeightDp - down) / interfaceSize).toInt()
                smallestScreenWidthDp = (smallestScreenWidthDp / interfaceSize).toInt()
                if (!clear.none) smallestScreenWidthDp = minOf(smallestScreenWidthDp, screenWidthDp, screenHeightDp)
            }
        }
    }
}

/**
 * The text scale the popups made before [PcPopup] were matched at: they ran at
 * the Legion's system font scale (0.85) then, so their sizes are set for it. Kept for them, now
 * on any device.
 */
const val MATCHED_POPUP_TEXT = 0.85f

/** How big the part of the interface being composed shows next to the rest, as [PartScaled] sized it. */
val LocalPartScale = androidx.compose.runtime.staticCompositionLocalOf { 1f }

/**
 * [content] as [part] of the interface, at the size a plugin gives it next to the rest
 * ([Pc.partScale]): its dp and sp are that much bigger or smaller, whatever part it is in.
 */
@Composable
fun PartScaled(part: io.github.artcraftandroid.procraft.core.InterfacePart, content: @Composable () -> Unit) {
    val scale = Pc.partScale(part)
    val was = LocalPartScale.current
    val density = LocalDensity.current
    CompositionLocalProvider(
        LocalDensity provides Density(density.density / was * scale, density.fontScale),
        LocalPartScale provides scale,
        content = content,
    )
}

/** The density of a popup, its text scaled by [textScale]: the interface's, at the menus' size. */
@Composable
private fun popupDensity(textScale: Float): Density {
    val menus = Pc.partScale(io.github.artcraftandroid.procraft.core.InterfacePart.MENUS)
    return Density(LocalDensity.current.density / LocalPartScale.current * menus, textScale)
}

/**
 * A popup whose text keeps one size whatever the system font scale. A popup is a window of its
 * own, composed with the system's font scale, so its text followed it where the rest of the
 * interface doesn't; this hands it the theme's density, its text scaled by [textScale], and the
 * theme's screen. It shows at the size a plugin gives popovers and menus.
 */
@Composable
fun PcPopup(
    alignment: androidx.compose.ui.Alignment = androidx.compose.ui.Alignment.TopStart,
    offset: androidx.compose.ui.unit.IntOffset = androidx.compose.ui.unit.IntOffset(0, 0),
    onDismissRequest: (() -> Unit)? = null,
    properties: androidx.compose.ui.window.PopupProperties = androidx.compose.ui.window.PopupProperties(),
    textScale: Float = 1f,
    content: @Composable () -> Unit,
) {
    val density = popupDensity(textScale)
    val menus = Pc.partScale(io.github.artcraftandroid.procraft.core.InterfacePart.MENUS)
    val screen = LocalConfiguration.current
    androidx.compose.ui.window.Popup(alignment, offset, onDismissRequest, properties) {
        CompositionLocalProvider(LocalDensity provides density, LocalPartScale provides menus, LocalConfiguration provides screen, content = content)
    }
}

/** [PcPopup] placed by a [popupPositionProvider]. */
@Composable
fun PcPopup(
    popupPositionProvider: androidx.compose.ui.window.PopupPositionProvider,
    onDismissRequest: (() -> Unit)? = null,
    properties: androidx.compose.ui.window.PopupProperties = androidx.compose.ui.window.PopupProperties(),
    textScale: Float = 1f,
    content: @Composable () -> Unit,
) {
    val density = popupDensity(textScale)
    val menus = Pc.partScale(io.github.artcraftandroid.procraft.core.InterfacePart.MENUS)
    val screen = LocalConfiguration.current
    androidx.compose.ui.window.Popup(popupPositionProvider, onDismissRequest, properties) {
        CompositionLocalProvider(LocalDensity provides density, LocalPartScale provides menus, LocalConfiguration provides screen, content = content)
    }
}

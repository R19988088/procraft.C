package io.github.artcraftandroid.procraft

import android.content.Context
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.keptPlugins
import io.github.artcraftandroid.procraft.ui.Pc
import io.github.artcraftandroid.procraft.ui.Switch
import io.github.artcraftandroid.procraft.ui.fittedInterfaceSize

/**
 * The app's own settings, apart from the canvas: what Android's page for the app links to as its
 * settings, and Help's Advanced settings opens. The canvas reads them again when it comes back.
 */
class SettingsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = getSharedPreferences("interface", Context.MODE_PRIVATE)
        val plugins = keptPlugins(this, prefs)
        Pc.accentGiven = Pc.accentOf(plugins)
        setContent {
            val size = fittedInterfaceSize(plugins)
            val density = LocalDensity.current
            // left to right in every language, as the app is
            CompositionLocalProvider(
                LocalDensity provides Density(density.density * size, density.fontScale),
                androidx.compose.ui.platform.LocalLayoutDirection provides androidx.compose.ui.unit.LayoutDirection.Ltr,
            ) {
                SettingsScreen(prefs) { finish() }
            }
        }
    }
}

private val Back = Color(0xFF000000)
private val Group = Color(0xFF1C1C1E)
private val Ink = Color(0xFFFFFFFF)
private val Dim = Color(0xFF8E8E93)
private val Line = Color(0xFF38383A)
private val Blue get() = Pc.tinted(Color(0xFF0A84FF))

private val Blue get() = Pc.tinted(Color(0xFF0A84FF))

private const val LANGUAGE_KEY = "language"
private enum class AppLanguage(val code: String, val displayName: String) {
    Chinese("zh", "中文"), English("en", "English");

    companion object {
        fun from(code: String?) = entries.firstOrNull { it.code == code } ?: Chinese
    }
}

private fun localized(text: String, language: AppLanguage): String = if (language == AppLanguage.English) text else when (text) {
    "Canvas Orientation Memory" -> "画布方向记忆"
    "An artwork opens zoomed and turned as it was left." -> "打开作品时恢复上次的缩放和旋转状态。"
    "Fit Canvas Inside Interface" -> "将画布适配到界面内"
    "A fitted canvas keeps clear of the top bar and the sidebar." -> "适配后的画布会避开顶部栏和侧边栏。"
    "Disable Time-lapse" -> "禁用延时摄影"
    "New artworks don't record their time-lapse." -> "新作品不记录延时摄影。"
    "Simplified Undos" -> "合并撤销"
    "Strokes made in an adjustment undo together once it is left." -> "离开调整工具时，一次撤销其中的所有笔画。"
    "Maximum Speed Distance" -> "最大速度距离"
    "How far the pen moves before a brush's speed dynamics count it as fast." -> "画笔速度动态将笔迹视为快速移动前所需的距离。"
    "Drag and Drop export" -> "拖放导出"
    "Preferred file format" -> "首选文件格式"
    "Preferred image format" -> "首选图像格式"
    "Accessibility" -> "无障碍"
    "Single Touch Gestures Companion" -> "单指手势助手"
    "A floating bar of undo, redo, zoom, move and fit for one finger." -> "为单指操作提供撤销、重做、缩放、移动和适配浮动栏。"
    "Color Description Notifications" -> "颜色描述通知"
    "A message names the colour once it stops changing." -> "颜色停止变化后显示其名称。"
    "Feedback Sounds" -> "反馈音效"
    "Undo, redo, tools and panels click as they are used." -> "使用撤销、重做、工具和面板时播放提示音。"
    "Tap Assistance - Initial Touch Location Support" -> "点击辅助 - 支持初始触摸位置"
    "A tap lands where the finger first touched, even if it drifts before lifting." -> "即使手指抬起前发生偏移，点击仍落在最初触摸的位置。"
    else -> text
}

/** A row of the page. */
private sealed class Setting

/** A switch: its key in the preferences, its name, what it does, and where it starts. */
private class Toggle(val key: String, val label: String, val detail: String, val default: Boolean) : Setting()

/** A slider from [min] to [max]. */
private class Amount(val key: String, val detail: String, val min: Float, val max: Float, val default: Float) : Setting()

/** One of [options] (kept by its index). */
private class Choice(val key: String, val label: String, val options: List<String>, val default: Int) : Setting()

/** The groups of rows, each with its heading (none for "") */
private val Groups: List<Pair<String, List<Setting>>> = listOf(
    "" to listOf(
        Toggle(CanvasViewModel.ORIENTATION_MEMORY, "Canvas Orientation Memory", "An artwork opens zoomed and turned as it was left.", true),
        Toggle(CanvasViewModel.FIT_INSIDE, "Fit Canvas Inside Interface", "A fitted canvas keeps clear of the top bar and the sidebar.", false),
    ),
    "" to listOf(
        Toggle(CanvasViewModel.NO_TIME_LAPSE, "Disable Time-lapse", "New artworks don't record their time-lapse.", false),
        Toggle(CanvasViewModel.SIMPLIFIED_UNDOS, "Simplified Undos", "Strokes made in an adjustment undo together once it is left.", true),
    ),
    "Maximum Speed Distance" to listOf(
        Amount(CanvasViewModel.SPEED_DISTANCE, "How far the pen moves before a brush's speed dynamics count it as fast.", 0.25f, 1f, 1f),
    ),
    "Drag and Drop export" to listOf(
        Choice(CanvasViewModel.DRAG_FORMAT, "Preferred file format", CanvasViewModel.DRAG_FORMATS, 0),
        Choice(CanvasViewModel.DRAG_IMAGE_FORMAT, "Preferred image format", CanvasViewModel.DRAG_IMAGE_FORMATS, 0),
    ),
    "Accessibility" to listOf(
        Toggle(CanvasViewModel.SINGLE_TOUCH_COMPANION, "Single Touch Gestures Companion", "A floating bar of undo, redo, zoom, move and fit for one finger.", false),
        Toggle(CanvasViewModel.COLOR_NAMES, "Color Description Notifications", "A message names the colour once it stops changing.", false),
        Toggle(CanvasViewModel.FEEDBACK_SOUNDS, "Feedback Sounds", "Undo, redo, tools and panels click as they are used.", false),
        Toggle(
            CanvasViewModel.TAP_ASSISTANCE, "Tap Assistance - Initial Touch Location Support",
            "A tap lands where the finger first touched, even if it drifts before lifting.", false,
        ),
    ),
)

@Composable
private fun SettingsScreen(prefs: android.content.SharedPreferences, onDone: () -> Unit) {
    var licences by remember { mutableStateOf(false) }
    var language by remember { mutableStateOf(AppLanguage.from(prefs.getString(LANGUAGE_KEY, "zh"))) }
    if (licences) return Licences { licences = false }
    Box(Modifier.fillMaxSize().background(Back).safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
        Column(Modifier.widthIn(max = 640.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(24.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("ProCraft", color = Ink, fontSize = 30.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                Text(if (language == AppLanguage.Chinese) "完成" else "Done", color = Blue, fontSize = 17.sp, modifier = Modifier.clickable(onClick = onDone).padding(8.dp))
            }
            Text(if (language == AppLanguage.Chinese) "语言" else "Language", color = Dim, fontSize = 13.sp, modifier = Modifier.padding(start = 16.dp, top = 24.dp, bottom = 6.dp))
            var languageMenu by remember { mutableStateOf(false) }
            Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Group)) {
                Row(Modifier.fillMaxWidth().clickable { languageMenu = true }.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(if (language == AppLanguage.Chinese) "应用语言" else "App language", color = Ink, fontSize = 17.sp, modifier = Modifier.weight(1f))
                    Text(language.displayName, color = Blue, fontSize = 17.sp)
                }
                DropdownMenu(languageMenu, { languageMenu = false }, containerColor = Group) {
                    AppLanguage.entries.forEach { option -> DropdownMenuItem(
                        text = { Text(option.displayName, color = if (option == language) Blue else Ink, fontSize = 17.sp) },
                        onClick = {
                            language = option
                            prefs.edit().putString(LANGUAGE_KEY, option.code).apply()
                            languageMenu = false
                        },
                    ) }
                }
            }
            for ((heading, rows) in Groups) {
                Text(localized(heading, language).uppercase(), color = Dim, fontSize = 13.sp, modifier = Modifier.padding(start = 16.dp, top = 24.dp, bottom = 6.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Group)) {
                    rows.forEachIndexed { i, s ->
                        if (i > 0) Box(Modifier.padding(start = 16.dp).fillMaxWidth().height(1.dp).background(Line))
                        when (s) {
                            is Toggle -> ToggleRow(prefs, s, language)
                            is Amount -> AmountRow(prefs, s, language)
                            is Choice -> ChoiceRow(prefs, s, language)
                        }
                    }
                }
            }
            // the licences of what the app is made with
            Column(Modifier.padding(top = 24.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Group)) {
                Row(Modifier.fillMaxWidth().clickable { licences = true }.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Licenses", color = Ink, fontSize = 17.sp, modifier = Modifier.weight(1f))
                    Text("›", color = Dim, fontSize = 20.sp)
                }
            }
        }
    }
}

@Composable
private fun ToggleRow(prefs: android.content.SharedPreferences, s: Toggle, language: AppLanguage) {
    var on by remember { mutableStateOf(prefs.getBoolean(s.key, s.default)) }
    Row(
        Modifier.fillMaxWidth().clickable {
            on = !on
            prefs.edit().putBoolean(s.key, on).apply()
        }.padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(localized(s.label, language), color = Ink, fontSize = 17.sp)
            Text(localized(s.detail, language), color = Dim, fontSize = 13.sp)
        }
        Switch(on, enabled = true)
    }
}

@Composable
private fun AmountRow(prefs: android.content.SharedPreferences, s: Amount, language: AppLanguage) {
    var value by remember { mutableFloatStateOf(prefs.getFloat(s.key, s.default)) }
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
        // a thin track and a round knob; dragged anywhere along it
        val thumb = 14.dp
        Canvas(
            Modifier.fillMaxWidth().height(44.dp).pointerInput(s) {
                val r = thumb.toPx()
                fun at(x: Float) = s.min + ((x - r) / (size.width - 2 * r)).coerceIn(0f, 1f) * (s.max - s.min)
                awaitEachGesture {
                    val down = awaitFirstDown()
                    value = at(down.position.x)
                    drag(down.id) {
                        value = at(it.position.x)
                        it.consume()
                    }
                    prefs.edit().putFloat(s.key, value).apply()
                }
            },
        ) {
            val r = thumb.toPx()
            val y = size.height / 2
            val x = r + (size.width - 2 * r) * (value - s.min) / (s.max - s.min)
            drawLine(Line, Offset(r, y), Offset(size.width - r, y), 4.dp.toPx(), StrokeCap.Round)
            drawLine(Blue, Offset(r, y), Offset(x, y), 4.dp.toPx(), StrokeCap.Round)
            drawCircle(Ink, r, Offset(x, y))
        }
        Text(localized(s.detail, language), color = Dim, fontSize = 13.sp, modifier = Modifier.padding(bottom = 4.dp))
    }
}

@Composable
private fun ChoiceRow(prefs: android.content.SharedPreferences, s: Choice, language: AppLanguage) {
    var chosen by remember { mutableStateOf(prefs.getInt(s.key, s.default).coerceIn(s.options.indices)) }
    var open by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().clickable { open = true }.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(localized(s.label, language), color = Ink, fontSize = 17.sp, modifier = Modifier.weight(1f))
        // the menu opens under the value
        Box {
            Text(s.options[chosen], color = Dim, fontSize = 17.sp)
            DropdownMenu(open, { open = false }, containerColor = Group) {
                s.options.forEachIndexed { i, name ->
                    DropdownMenuItem(
                        text = { Text(name, color = if (i == chosen) Blue else Ink, fontSize = 17.sp) },
                        onClick = {
                            chosen = i
                            prefs.edit().putInt(s.key, i).apply()
                            open = false
                        },
                    )
                }
            }
        }
    }
}

/**
 * The licences of what the app is made with: the interface font's and icons', then the third-party
 * software's (assets/licenses.txt, written by tools/licenses.py), a paragraph a row.
 */
@Composable
private fun Licences(onBack: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val paragraphs = remember {
        fun asset(name: String) = runCatching { context.assets.open(name).bufferedReader().use { it.readText() } }.getOrDefault("")
        val gap = "\n\n"
        val text = "The interface font is Inter, by The Inter Project Authors, under the SIL Open Font License 1.1." + gap +
            asset("fonts/Inter-OFL.txt").trim() + gap +
            "The app keeps Lucide icons (lucide.dev), by Lucide Icons and Contributors, under the ISC License." + gap +
            asset("icons/LICENSE-lucide.txt").trim() + gap + asset("licenses.txt")
        text.split(gap).map { it.trimEnd() }.filter { it.isNotBlank() }
    }
    androidx.activity.compose.BackHandler(onBack = onBack)
    Box(Modifier.fillMaxSize().background(Back).safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
        androidx.compose.foundation.lazy.LazyColumn(Modifier.widthIn(max = 640.dp).fillMaxWidth().padding(horizontal = 24.dp)) {
            item {
                Text("‹ Settings", color = Blue, fontSize = 17.sp, modifier = Modifier.padding(top = 24.dp).clickable(onClick = onBack).padding(vertical = 8.dp))
                Text("Licenses", color = Ink, fontSize = 30.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(top = 8.dp, bottom = 16.dp))
            }
            items(paragraphs.size) { i -> Text(paragraphs[i], color = Dim, fontSize = 13.sp, modifier = Modifier.padding(bottom = 12.dp)) }
        }
    }
}

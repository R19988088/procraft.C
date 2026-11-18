package io.github.artcraftandroid.procraft.canvas

/**
 * The chips (as Android names them) whose tablets have shown a wide-colour canvas wrongly: the
 * display's compositor left only the canvas and its frosted glass, the bars and panels over it
 * gone to a blur until something else was drawn over the screen (Snapdragon 695). Wide Color
 * starts off on them; it can still be turned on.
 */
private val WIDE_COLOR_OFF_ON = listOf("SM6375")

/** Whether Wide Color starts on for a device with the chip [soc] (null where Android doesn't say). */
internal fun wideColorByDefault(soc: String?): Boolean {
    val chip = soc?.trim()?.uppercase() ?: return true
    return WIDE_COLOR_OFF_ON.none { chip.startsWith(it) }
}

/** [wideColorByDefault] for this device (Android says its chip from Android 12). */
internal fun wideColorByDefault(): Boolean =
    wideColorByDefault(if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) android.os.Build.SOC_MODEL else null)

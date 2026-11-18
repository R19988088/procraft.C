package io.github.artcraftandroid.procraft.canvas

import android.view.KeyEvent

/** The keys the shared shortcuts name by a word or a sign (crates/core keys.rs), by their key codes; letters and digits go by themselves. */
private val NAMED = listOf(
    KeyEvent.KEYCODE_LEFT_BRACKET to "[",
    KeyEvent.KEYCODE_RIGHT_BRACKET to "]",
    KeyEvent.KEYCODE_SEMICOLON to ";",
    KeyEvent.KEYCODE_COMMA to ",",
    KeyEvent.KEYCODE_PERIOD to ".",
    KeyEvent.KEYCODE_SLASH to "/",
    KeyEvent.KEYCODE_BACKSLASH to "\\",
    KeyEvent.KEYCODE_APOSTROPHE to "'",
    KeyEvent.KEYCODE_GRAVE to "`",
    KeyEvent.KEYCODE_MINUS to "-",
    KeyEvent.KEYCODE_EQUALS to "=",
    KeyEvent.KEYCODE_PLUS to "plus",
    KeyEvent.KEYCODE_SPACE to "space",
    KeyEvent.KEYCODE_DEL to "backspace",
    KeyEvent.KEYCODE_FORWARD_DEL to "delete",
    KeyEvent.KEYCODE_ESCAPE to "escape",
    KeyEvent.KEYCODE_DPAD_LEFT to "left",
    KeyEvent.KEYCODE_DPAD_RIGHT to "right",
    KeyEvent.KEYCODE_DPAD_UP to "up",
    KeyEvent.KEYCODE_DPAD_DOWN to "down",
) + (0 until 12).map { KeyEvent.KEYCODE_F1 + it to "f${it + 1}" }

/** The key [keyCode] as the shared shortcuts name it ("z", "[", "space", "f5"), as a plugin rebinding keys is told it; null for one they don't name. */
internal fun keyName(keyCode: Int): String? = when (keyCode) {
    in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> ('a' + (keyCode - KeyEvent.KEYCODE_A)).toString()
    in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> ('0' + (keyCode - KeyEvent.KEYCODE_0)).toString()
    else -> NAMED.firstOrNull { it.first == keyCode }?.second
}

/** The key code of a key as the shared shortcuts name it ([keyName] the other way). */
internal fun keyCodeNamed(name: String): Int? = when {
    name.length == 1 && name[0] in 'a'..'z' -> KeyEvent.KEYCODE_A + (name[0] - 'a')
    name.length == 1 && name[0] in '0'..'9' -> KeyEvent.KEYCODE_0 + (name[0] - '0')
    else -> NAMED.firstOrNull { it.second == name }?.first
}

/** The key the shared shortcuts' list names for Alt held by itself, the modify button: the app takes it, never as a press. */
internal const val MODIFY_KEY = "alt"

/**
 * A press of the shared shortcuts' list as the system's list of them takes it: its key code and
 * modifiers (Ctrl for the command key). Alt held by itself ([MODIFY_KEY]) is the modifier with no
 * key, which the system writes alone. Null for a key it doesn't name.
 */
internal fun systemShortcutKeys(key: String, command: Boolean, shift: Boolean, alt: Boolean): Pair<Int, Int>? {
    if (key == MODIFY_KEY) return KeyEvent.KEYCODE_UNKNOWN to KeyEvent.META_ALT_ON
    val code = keyCodeNamed(key) ?: return null
    val modifiers = (if (command) KeyEvent.META_CTRL_ON else 0) or
        (if (shift) KeyEvent.META_SHIFT_ON else 0) or
        (if (alt) KeyEvent.META_ALT_ON else 0)
    return code to modifiers
}

/** A key pressed with modifiers, as the shared shortcuts take it: the key's name and whether the command key, Shift and Alt are held. */
internal data class KeyPress(val key: String, val command: Boolean, val shift: Boolean, val alt: Boolean)

/** Ctrl and Meta: either is the command key (a Mac's ⌘), as a keyboard made for another system has one or the other. */
private const val COMMAND_ON = KeyEvent.META_CTRL_ON or KeyEvent.META_META_ON

/** The press [keyCode] is with a key event's [metaState], as the shared shortcuts take it; null for a key they don't name. */
internal fun keyPress(keyCode: Int, metaState: Int): KeyPress? {
    val key = keyName(keyCode) ?: return null
    val held = { on: Int -> (metaState and on) != 0 }
    return KeyPress(key, held(COMMAND_ON), held(KeyEvent.META_SHIFT_ON), held(KeyEvent.META_ALT_ON))
}

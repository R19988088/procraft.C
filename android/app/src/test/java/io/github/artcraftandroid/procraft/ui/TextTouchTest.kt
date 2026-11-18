package io.github.artcraftandroid.procraft.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TextTouchTest {
    @Test
    fun aTouchOnTheTextLeavesTheStyleSheetInTheKeyboardsPlace() {
        assertFalse(textTouchBringsKeyboard(styleOpen = true))
        assertTrue(textTouchBringsKeyboard(styleOpen = false))
    }
}

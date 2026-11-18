package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WideColorTest {
    @Test
    fun wideColorStartsOffOnASnapdragon695() {
        assertFalse(wideColorByDefault("SM6375"))
        assertFalse(wideColorByDefault(" sm6375 "))
    }

    @Test
    fun wideColorStartsOnElsewhereAndWhereTheChipIsntKnown() {
        assertTrue(wideColorByDefault("SM8750"))
        assertTrue(wideColorByDefault("MT6789"))
        assertTrue(wideColorByDefault(""))
        assertTrue(wideColorByDefault(null))
    }
}

package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertEquals
import org.junit.Test

class BrushFileNameTest {
    @Test
    fun aNameInAnyScriptIsKept() {
        assertEquals("水彩 笔.brush", brushFileName("水彩 笔", single = true))
        assertEquals("Кисть-2.brush", brushFileName("Кисть-2", single = true))
        assertEquals("Café Inks.brushset", brushFileName("Café Inks", single = false))
    }

    @Test
    fun anotherAppsFileWaitingForACanvasKeepsItsNameAndEnding() {
        val kept = pendingBrushFileName(42, "Charcoal_KA.bundle")
        assertEquals("42-Charcoal_KA.bundle", kept)
        // read back by the name after the stamp: the ending the brush engines go by
        assertEquals("Charcoal_KA.bundle", kept.substringAfter('-'))
        assertEquals("Basic-5 Size.kpp", pendingBrushFileName(7, "Basic-5 Size.kpp").substringAfter('-'))
        assertEquals("7-水彩 笔.abr", pendingBrushFileName(7, "水彩 笔.abr"))
        assertEquals("7-a_b.kpp", pendingBrushFileName(7, "a/b.kpp"))
        assertEquals("7-Imported", pendingBrushFileName(7, "  "))
    }

    @Test
    fun whatAFileNameCantHoldIsMadeAnUnderscore() {
        assertEquals("a_b_c_.brush", brushFileName("a/b:c?", single = true))
        assertEquals("Brush.brush", brushFileName("   ", single = true))
        assertEquals("Brush Set.brushset", brushFileName("", single = false))
    }
}

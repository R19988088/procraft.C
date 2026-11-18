package io.github.artcraftandroid.procraft.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class GallerySwipeTest {
    @Test
    fun aCellSwipedLeftOpensItsActionsAndRightPutsThemAway() {
        assertEquals(true, swipeOpens(-80f, 10f, selecting = false))
        assertEquals(false, swipeOpens(80f, -10f, selecting = false))
        // mostly down is a scroll; selecting, nothing swipes
        assertNull(swipeOpens(-30f, 60f, selecting = false))
        assertNull(swipeOpens(-80f, 0f, selecting = true))
    }

    @Test
    fun severalPickedAreOnePdfNamedAfterTheirStack() {
        assertEquals("Holiday", galleryShareName("Holiday"))
        assertEquals("Artworks", galleryShareName(null))
    }
}

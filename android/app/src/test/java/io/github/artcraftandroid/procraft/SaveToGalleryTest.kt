package io.github.artcraftandroid.procraft

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files

class SaveToGalleryTest {
    @Test
    fun picturesAndVideosGalleriesShowCanBeSavedThere() {
        for (mime in listOf("image/png", "image/jpeg", "image/gif", "image/webp", "video/mp4")) {
            assertTrue(mime, savesToGallery(mime))
        }
        // files a gallery doesn't show: the share sheet's Save to device only
        for (mime in listOf("image/vnd.adobe.photoshop", "image/tiff", "image/svg+xml", "application/pdf", "application/octet-stream", "text/plain")) {
            assertFalse(mime, savesToGallery(mime))
        }
    }

    @Test
    fun aNameTakenGetsTheNextNumberBeforeItsExtension() {
        val dir = Files.createTempDirectory("gallery").toFile()
        try {
            assertEquals("Untitled Artwork.png", freeName(dir, "Untitled Artwork.png").name)
            java.io.File(dir, "Untitled Artwork.png").writeText("")
            assertEquals("Untitled Artwork (2).png", freeName(dir, "Untitled Artwork.png").name)
            java.io.File(dir, "Untitled Artwork (2).png").writeText("")
            assertEquals("Untitled Artwork (3).png", freeName(dir, "Untitled Artwork.png").name)
            java.io.File(dir, "notes").writeText("")
            assertEquals("notes (2)", freeName(dir, "notes").name)
        } finally {
            dir.deleteRecursively()
        }
    }
}

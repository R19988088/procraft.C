package io.github.artcraftandroid.procraft.ui

import io.github.artcraftandroid.procraft.canvas.CanvasViewModel

/** What artworks picked in the stack named [stack] (none: the gallery itself) are named as one PDF. */
internal fun galleryShareName(stack: String?): String = stack ?: CanvasViewModel.GROUP_NAME

/**
 * A drag [dx] across and [dy] down on a gallery cell (an artwork's or a stack's), not while
 * selecting, swipes it: left opens its actions (true), right puts them away (false); null for a
 * drag mostly up or down.
 */
internal fun swipeOpens(dx: Float, dy: Float, selecting: Boolean): Boolean? =
    if (selecting || kotlin.math.abs(dx) <= kotlin.math.abs(dy)) null else dx < 0f

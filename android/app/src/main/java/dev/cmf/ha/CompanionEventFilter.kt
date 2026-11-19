package dev.cmf.ha

/**
 * Drops events that do not belong to the running app slot and stale repeats.
 * Event envelope from the firmware: [sequence:u8, slot:u8, payload...].
 */
internal class CompanionEventFilter {

    private var runningSlot: Int? = null
    private var lastSequence: Int? = null
    private val waitingForSlot = ArrayDeque<ByteArray>()

    @Synchronized
    fun selectSlot(slot: Int): List<ByteArray> {
        if (runningSlot == slot) return emptyList()
        if (runningSlot != null) waitingForSlot.clear()
        lastSequence = null
        runningSlot = slot
        val buffered = waitingForSlot.toList()
        waitingForSlot.clear()
        return buffered.mapNotNull(::acceptForKnownSlot)
    }

    @Synchronized
    fun accept(event: ByteArray): ByteArray? {
        if (event.size < 3) return null
        if (runningSlot == null) {
            if (waitingForSlot.size < 32) waitingForSlot.addLast(event.copyOf())
            return null
        }
        return acceptForKnownSlot(event)
    }

    private fun acceptForKnownSlot(event: ByteArray): ByteArray? {
        if (event.size < 3 || (event[1].toInt() and 255) != runningSlot) return null
        val sequence = event[0].toInt() and 255
        val previous = lastSequence ?: return event.copyOfRange(2, event.size).also { lastSequence = sequence }
        val distance = (sequence - previous) and 255
        if (distance !in 1..127) return null
        lastSequence = sequence
        return event.copyOfRange(2, event.size)
    }
}

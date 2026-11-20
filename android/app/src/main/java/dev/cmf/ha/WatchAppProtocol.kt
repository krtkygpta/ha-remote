package dev.cmf.ha

import java.io.ByteArrayOutputStream
import java.nio.charset.StandardCharsets

/**
 * The ha-remote protocol v3 on top of the channel: ASCII "H1", a type byte, fixed fields.
 * Must match apps/ha-remote/src/proto.rs exactly: ten slots, a kind per slot (toggle or
 * dimmer) and five favorites. Configuration is split because ten 15-byte names do not fit
 * one 128-byte message; the phone sends V, J, K, U in that order and U is the commit.
 */
internal object WatchAppProtocol {

    const val SLOTS = 10
    const val FAVORITES = 5
    const val HALF = 5
    const val TOGGLE = 0
    const val DIMMER = 1

    data class Slot(val enabled: Boolean, val label: String, val section: String = "", val kind: Int = TOGGLE)
    data class Toggle(val sequence: Int, val slot: Int, val on: Boolean)
    data class Brightness(val sequence: Int, val slot: Int, val value: Int)

    private fun head(message: ByteArray, type: Char, size: Int): Boolean =
        message.size == size &&
            message[0] == 'H'.code.toByte() && message[1] == '1'.code.toByte() &&
            message[2] == type.code.toByte()

    private fun utf8(text: String, max: Int): ByteArray {
        val raw = text.toByteArray(StandardCharsets.UTF_8)
        var count = minOf(max, raw.size)
        while (count > 0 && count < raw.size && (raw[count].toInt() and 0xC0) == 0x80) count--
        return raw.copyOf(count)
    }

    private fun header(type: Char, count: Int) =
        byteArrayOf('H'.code.toByte(), '1'.code.toByte(), type.code.toByte(), count.toByte())

    /** Slots 0..5 plus the five favorites: the commit frame the watch persists. */
    fun configA(slots: List<Slot>, favorites: List<Int>): ByteArray {
        require(slots.size <= HALF)
        val out = ByteArrayOutputStream()
        out.write(header('U', slots.size))
        writeRecords(out, slots)
        for (i in 0 until FAVORITES) {
            val slot = favorites.getOrNull(i) ?: -1
            out.write(if (slot in 0 until SLOTS) slot else 0xFF)
        }
        return out.toByteArray()
    }

    /** Slots 5..10. */
    fun configB(slots: List<Slot>): ByteArray {
        require(slots.size <= HALF)
        val out = ByteArrayOutputStream()
        out.write(header('V', slots.size))
        writeRecords(out, slots)
        return out.toByteArray()
    }

    private fun writeRecords(out: ByteArrayOutputStream, slots: List<Slot>) {
        for (slot in slots) {
            val name = utf8(slot.label, 15)
            out.write(byteArrayOf(if (slot.enabled) 1 else 0, if (slot.kind == DIMMER) 1 else 0, name.size.toByte()))
            out.write(name)
        }
    }

    /** Section titles for the first five slots. */
    fun sectionsA(slots: List<Slot>): ByteArray = sectionsFrame('J', slots)

    /** Section titles for slots 5..10. */
    fun sectionsB(slots: List<Slot>): ByteArray = sectionsFrame('K', slots)

    private fun sectionsFrame(type: Char, slots: List<Slot>): ByteArray {
        require(slots.size <= HALF)
        val out = ByteArrayOutputStream()
        out.write(header(type, slots.size))
        for (slot in slots) {
            val section = utf8(slot.section, 10)
            out.write(section.size)
            out.write(section)
        }
        return out.toByteArray()
    }

    fun parseToggle(message: ByteArray): Toggle? {
        if (!head(message, 'T', 6)) return null
        val slot = message[4].toInt() and 255
        val on = message[5].toInt() and 255
        if (slot !in 0 until SLOTS || on !in 0..1) return null
        return Toggle(message[3].toInt() and 255, slot, on == 1)
    }

    fun parseBrightness(message: ByteArray): Brightness? {
        if (!head(message, 'B', 6)) return null
        val slot = message[4].toInt() and 255
        val value = message[5].toInt() and 255
        if (slot !in 0 until SLOTS || value !in 0..100) return null
        return Brightness(message[3].toInt() and 255, slot, value)
    }

    fun heartbeat() = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'P'.code.toByte())
    fun widgetStatus(result: Int) = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'W'.code.toByte(), result.toByte())
    fun ack(sequence: Int) = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'A'.code.toByte(), sequence.toByte())
    fun error(sequence: Int, code: Int) =
        byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'E'.code.toByte(), sequence.toByte(), code.toByte())

    /** Authoritative state: on/off, brightness percent, result, and HA availability. */
    fun state(slot: Int, on: Boolean, value: Int, result: Int, available: Boolean) =
        byteArrayOf(
            'H'.code.toByte(), '1'.code.toByte(), 'S'.code.toByte(),
            slot.toByte(),
            if (on) 1 else 0,
            value.coerceIn(0, 100).toByte(),
            result.toByte(),
            if (available) 1 else 0
        )
}

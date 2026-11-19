package dev.cmf.ha

import java.io.ByteArrayOutputStream
import java.nio.charset.StandardCharsets

/**
 * The ha-remote protocol on top of the channel: ASCII "H1", a type byte, fixed fields.
 * Must match apps/ha-remote/src/proto.rs exactly.
 */
internal object WatchAppProtocol {

    data class Slot(val enabled: Boolean, val label: String, val section: String = "")
    data class Toggle(val sequence: Int, val slot: Int, val on: Boolean)

    fun config(slots: List<Slot>): ByteArray {
        require(slots.size <= 5)
        val out = ByteArrayOutputStream()
        out.write(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'C'.code.toByte(), slots.size.toByte()))
        for (slot in slots) {
            val raw = slot.label.toByteArray(StandardCharsets.UTF_8)
            var count = minOf(15, raw.size)
            while (count > 0 && count < raw.size && (raw[count].toInt() and 0xC0) == 0x80) count--
            out.write(byteArrayOf(if (slot.enabled) 1 else 0, count.toByte()))
            out.write(raw, 0, count)
        }
        return out.toByteArray()
    }

    fun parseToggle(message: ByteArray): Toggle? {
        if (message.size != 6) return null
        if (message[0] != 'H'.code.toByte() || message[1] != '1'.code.toByte() || message[2] != 'T'.code.toByte()) return null
        val slot = message[4].toInt() and 255
        val on = message[5].toInt() and 255
        if (slot !in 0..4 || on !in 0..1) return null
        return Toggle(message[3].toInt() and 255, slot, on == 1)
    }

    /** Section titles for the five slots, then the favorite slot index (or -1 for none). */
    fun groups(slots: List<Slot>, favorite: Int): ByteArray {
        require(slots.size <= 5)
        val out = ByteArrayOutputStream()
        out.write(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'G'.code.toByte(), slots.size.toByte()))
        for (slot in slots) {
            val raw = slot.section.toByteArray(StandardCharsets.UTF_8)
            var count = minOf(10, raw.size)
            while (count > 0 && count < raw.size && (raw[count].toInt() and 0xC0) == 0x80) count--
            out.write(count)
            out.write(raw, 0, count)
        }
        out.write(if (favorite in 0..4) favorite else 0xFF)
        return out.toByteArray()
    }

    fun heartbeat() = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'P'.code.toByte())
    fun widgetStatus(result: Int) = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'W'.code.toByte(), result.toByte())
    fun ack(sequence: Int) = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'A'.code.toByte(), sequence.toByte())
    fun error(sequence: Int, code: Int) = byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'E'.code.toByte(), sequence.toByte(), code.toByte())
    fun state(slot: Int, on: Boolean, result: Int) =
        byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'S'.code.toByte(), slot.toByte(), if (on) 1 else 0, result.toByte())
}

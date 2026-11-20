package dev.cmf.ha

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class CompanionProtocolTest {
    private val key = ByteArray(16) { it.toByte() }
    private fun hex(s: String) = ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

    @Test fun channelFramesMatchSdkVectors() {
        assertArrayEquals(hex("f50010ffff0001000190a7d7a03ccf08ff2d1d1569f9684f3d6223"),
            CompanionProtocol.frame(CompanionProtocol.PING, byteArrayOf(0), key))
        assertArrayEquals(hex("f50010ffff0001000190a56bc6b3689c06b5210bd489537178127f"),
            CompanionProtocol.frame(CompanionProtocol.MSG, byteArrayOf(0xFF.toByte(), 'n'.code.toByte(), '1'.code.toByte(), 'Q'.code.toByte()), key))
    }

    @Test fun parserReassemblesResponsesAndFiltersOtherKeys() {
        val pong = hex("f50010ffff00010001b0a799fa4e06d749eef057c9681ca82f52fc")
        val parser = CompanionProtocol.Parser()
        val packets = parser.feed(pong.copyOfRange(0, 13)) + parser.feed(pong.copyOfRange(13, pong.size))
        assertEquals(1, packets.size)
        assertEquals(0xB0A7, packets.single().c2)
        assertArrayEquals(byteArrayOf(0, 2, 1), CompanionProtocol.open(key, packets.single().body))
        assertNull(CompanionProtocol.open(ByteArray(16), packets.single().body))
        assertNotNull(CompanionProtocol.open(key, CompanionProtocol.seal(key, byteArrayOf(1, 2, 3))))
    }

    @Test fun eventFilterChecksSlotAndDeduplicatesWrappedSequences() {
        val filter = CompanionEventFilter()
        assertNull(filter.accept(byteArrayOf(254.toByte(), 3, 7)))
        assertArrayEquals(byteArrayOf(7), filter.selectSlot(3).single())
        assertArrayEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), '?'.code.toByte()),
            filter.accept(byteArrayOf(255.toByte(), 3, 'H'.code.toByte(), '1'.code.toByte(), '?'.code.toByte())))
        assertNull(filter.accept(byteArrayOf(255.toByte(), 3, 9)))
        assertNull(filter.accept(byteArrayOf(0, 2, 9)))
        assertArrayEquals(byteArrayOf(9), filter.accept(byteArrayOf(0, 3, 9)))
        assertNull(filter.accept(byteArrayOf(255.toByte(), 3, 9)))
        filter.selectSlot(4)
        assertArrayEquals(byteArrayOf(9), filter.accept(byteArrayOf(10, 4, 9)))
    }

    @Test fun watchAppFramesMatchTheRustProtocol() {
        val halfA = listOf(
            WatchAppProtocol.Slot(true, "Lamp", "Room", WatchAppProtocol.DIMMER),
            WatchAppProtocol.Slot(false, "Control 2")
        )
        val config = WatchAppProtocol.configA(halfA, listOf(0, -1, -1, -1, -1))
        assertArrayEquals(
            hex("48315502" + "0101044c616d70" + "000009436f6e74726f6c2032" + "00ffffffff"),
            config
        )
        val configB = WatchAppProtocol.configB(listOf(WatchAppProtocol.Slot(true, "Strip", "", WatchAppProtocol.DIMMER)))
        assertArrayEquals(hex("48315601" + "0101055374726970"), configB)
        assertArrayEquals(hex("48314a0204526f6f6d00"), WatchAppProtocol.sectionsA(halfA))
        assertEquals(WatchAppProtocol.Toggle(7, 2, true), WatchAppProtocol.parseToggle(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'T'.code.toByte(), 7, 2, 1)))
        assertNull(WatchAppProtocol.parseToggle(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'T'.code.toByte(), 7, 2, 2)))
        assertNotNull(WatchAppProtocol.parseToggle(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'T'.code.toByte(), 7, 9, 1)))
        assertNull(WatchAppProtocol.parseToggle(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'T'.code.toByte(), 7, 10, 1)))
        assertEquals(WatchAppProtocol.Brightness(5, 7, 80), WatchAppProtocol.parseBrightness(hex("483142050750")))
        assertNull(WatchAppProtocol.parseBrightness(hex("483142050765")))
        assertArrayEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'E'.code.toByte(), 7, 1), WatchAppProtocol.error(7, 1))
        assertArrayEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'S'.code.toByte(), 2, 1, 65, 0, 1), WatchAppProtocol.state(2, true, 65, 0, true))
        assertArrayEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'S'.code.toByte(), 1, 0, 0, 1, 0), WatchAppProtocol.state(1, false, 0, 1, false))
        assertArrayEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'P'.code.toByte()), WatchAppProtocol.heartbeat())
        assertArrayEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), 'W'.code.toByte(), (-22).toByte()), WatchAppProtocol.widgetStatus(-22))
    }

    @Test fun firmwareInfoReportsWidgetCompatibilityFromStubAndAbi() {
        val latest = CompanionProtocol.parseStubInfo(byteArrayOf(0, 5, 0, 0, 0, 3))
        assertEquals(5, latest?.version)
        assertEquals(true, latest?.supportsAppWidget)
        assertEquals(false, CompanionProtocol.parseStubInfo(byteArrayOf(0, 3, 0, 0, 0, 3))?.supportsAppWidget)
        assertEquals(false, CompanionProtocol.parseStubInfo(byteArrayOf(0, 5, 0, 0, 0, 2))?.supportsAppWidget)
        assertNull(CompanionProtocol.parseStubInfo(byteArrayOf(1)))
        assertNull(CompanionProtocol.parseStubInfo(null))
    }
}

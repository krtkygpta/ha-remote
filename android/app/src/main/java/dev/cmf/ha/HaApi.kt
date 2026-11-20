package dev.cmf.ha

import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/** Home Assistant REST calls. Blocking; call from a background dispatcher. */
class HaApi(private val base: String, private val token: String) {

    /**
     * on/off plus brightness percent (0..=100; for non-dimmable entities 0 or 100) and whether
     * Home Assistant currently has data for the entity. available = false is the normal state
     * of a device that lost power — not an error.
     */
    data class EntityState(val on: Boolean, val value: Int, val available: Boolean = true)

    private fun request(path: String, method: String = "GET", json: JSONObject? = null): String {
        val connection = URL(base.trimEnd('/') + path).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 7000
            connection.readTimeout = 7000
            connection.requestMethod = method
            connection.setRequestProperty("Authorization", "Bearer $token")
            connection.setRequestProperty("Content-Type", "application/json")
            if (json != null) {
                connection.doOutput = true
                connection.outputStream.use { it.write(json.toString().toByteArray()) }
            }
            val code = connection.responseCode
            val body = (if (code in 200..299) connection.inputStream else connection.errorStream)
                ?.bufferedReader()?.use { it.readText() }.orEmpty()
            if (code !in 200..299) error("Home Assistant request failed ($code)")
            return body
        } finally {
            connection.disconnect()
        }
    }

    fun validate() { request("/api/") }

    /** Every entity in Home Assistant, as (entity_id, friendly_name). */
    fun entities(): List<Pair<String, String>> = JSONArray(request("/api/states")).let { states ->
        (0 until states.length()).mapNotNull { i ->
            val state = states.optJSONObject(i) ?: return@mapNotNull null
            val id = state.optString("entity_id")
            if (id.isBlank()) return@mapNotNull null
            val name = state.optJSONObject("attributes")?.optString("friendly_name").orEmpty()
            id to name
        }
    }

    fun state(id: String): EntityState {
        val obj = JSONObject(request("/api/states/$id"))
        val raw = obj.optString("state")
        if (raw == "unavailable" || raw == "unknown") {
            return EntityState(false, 0, available = false)
        }
        val on = raw == "on" || raw == "true" || raw == "open"
        val brightness = obj.optJSONObject("attributes")?.optInt("brightness", -1) ?: -1
        val value = when {
            !on -> 0
            brightness in 0..255 -> (brightness * 100 + 127) / 255
            else -> 100
        }
        return EntityState(on, value.coerceIn(0, 100), available = true)
    }

    /** Generic on/off: `homeassistant.turn_on/off` maps to each domain's own service. */
    fun set(id: String, on: Boolean) {
        val service = if (on) "turn_on" else "turn_off"
        request("/api/services/homeassistant/$service", "POST", JSONObject().put("entity_id", id))
    }

    /** Brightness percent for a `light.*` entity; 0 turns it off. */
    fun setBrightness(id: String, percent: Int) {
        require(id.startsWith("light.")) { "Only light entities are dimmable" }
        val pct = percent.coerceIn(0, 100)
        if (pct == 0) {
            request("/api/services/light/turn_off", "POST", JSONObject().put("entity_id", id))
        } else {
            request(
                "/api/services/light/turn_on",
                "POST",
                JSONObject().put("entity_id", id).put("brightness_pct", pct)
            )
        }
    }

    /** Waits until the entity reaches [on]; false on timeout. */
    fun awaitState(id: String, on: Boolean): Boolean {
        val deadline = System.nanoTime() + 5_000_000_000L
        do {
            if (runCatching { state(id) }.getOrNull().let { it != null && it.available && it.on == on }) return true
            Thread.sleep(350)
        } while (System.nanoTime() < deadline)
        return false
    }

    /** Waits until the reported brightness is within a few percent of [percent]. */
    fun awaitBrightness(id: String, percent: Int): Boolean {
        val target = percent.coerceIn(0, 100)
        val deadline = System.nanoTime() + 5_000_000_000L
        do {
            val current = runCatching { state(id) }.getOrNull()
            if (current != null && current.available && current.on && target > 0 && kotlin.math.abs(current.value - target) <= 3) return true
            if (current != null && current.available && target == 0 && !current.on) return true
            Thread.sleep(350)
        } while (System.nanoTime() < deadline)
        return false
    }
}

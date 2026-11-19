package dev.cmf.ha

import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/** Home Assistant REST calls. Blocking; call from a background dispatcher. */
class HaApi(private val base: String, private val token: String) {

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

    /** All light entities and light groups, as (entity_id, friendly_name). */
    fun entities(): List<Pair<String, String>> = JSONArray(request("/api/states")).let { states ->
        (0 until states.length()).mapNotNull { i ->
            val state = states.optJSONObject(i) ?: return@mapNotNull null
            val id = state.optString("entity_id")
            val attributes = state.optJSONObject("attributes")
            val isLight = id.startsWith("light.")
            val members = attributes?.optJSONArray("entity_id")
            val isLightGroup = id.startsWith("group.") && members != null &&
                (0 until members.length()).any { members.optString(it).startsWith("light.") }
            if (isLight || isLightGroup) id to attributes?.optString("friendly_name").orEmpty() else null
        }
    }

    fun state(id: String): Boolean = when (val state = JSONObject(request("/api/states/$id")).optString("state")) {
        "on" -> true
        "off" -> false
        else -> error("Home Assistant entity is unavailable ($state)")
    }

    fun set(id: String, on: Boolean) {
        val service = if (on) "turn_on" else "turn_off"
        val domain = if (id.startsWith("group.")) "homeassistant" else "light"
        request("/api/services/$domain/$service", "POST", JSONObject().put("entity_id", id))
    }

    /** Waits until the entity reaches [on]; false on timeout. */
    fun awaitState(id: String, on: Boolean): Boolean {
        val deadline = System.nanoTime() + 5_000_000_000L
        do {
            if (runCatching { state(id) }.getOrNull() == on) return true
            Thread.sleep(350)
        } while (System.nanoTime() < deadline)
        return false
    }
}

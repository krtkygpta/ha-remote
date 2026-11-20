package dev.cmf.ha

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothManager
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.nio.charset.StandardCharsets
import java.util.LinkedHashMap
import java.util.concurrent.atomic.AtomicBoolean
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties

/**
 * Holds the watch link while the screen is off. Pushes the ten controls and their states to
 * the watch app, answers its toggle requests through Home Assistant, and reconnects with
 * bounded backoff after any loss. Home Assistant stays optional: the link keeps running and
 * the watch shows cached state until it recovers.
 */
class CompanionService : Service() {

    // An uncaught exception in any coroutine must never kill the process; report and go on.
    private val scope = CoroutineScope(
        SupervisorJob() + Dispatchers.IO +
            kotlinx.coroutines.CoroutineExceptionHandler { _, e ->
                runCatching { report("Watch connection retrying · " + (e.message ?: e.javaClass.simpleName)) }
            }
    )
    private var connectionJob: Job? = null
    private var restartJob: Job? = null
    private val recentRequests = LinkedHashMap<Int, Long>()
    private val busy = AtomicBoolean(false)
    private var widgetProbeResult: Int? = null

    @Volatile
    private var stoppedByUser = false

    @Volatile
    private var watchAddress: String? = null

    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(
            NotificationChannel(CHANNEL, "Watch connection", NotificationManager.IMPORTANCE_LOW)
        )
        runCatching {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(
                    NOTIFICATION_ID,
                    notification("Starting watch connection"),
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
                )
            } else {
                startForeground(NOTIFICATION_ID, notification("Starting watch connection"))
            }
        }.onFailure { stopSelf() }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        watchAddress = intent?.getStringExtra(EXTRA_ADDRESS) ?: watchAddress
            ?: getSharedPreferences("remote", MODE_PRIVATE).getString("watch", null)
        if (watchAddress.isNullOrBlank()) {
            report("Choose a bonded watch in the app")
            stopSelf()
            return START_NOT_STICKY
        }
        stoppedByUser = false
        val previous = connectionJob
        if (previous?.isActive == true) {
            if (restartJob?.isActive != true) {
                restartJob = scope.launch {
                    previous.cancelAndJoin()
                    if (isActive && !stoppedByUser) connectionJob = scope.launch { runConnectionLoop() }
                }
            }
        } else {
            connectionJob = scope.launch { runConnectionLoop() }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        stoppedByUser = true
        scope.cancel()
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    /** The user swiped the app away: bring the connection straight back up. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        val address = watchAddress
        if (address != null && !stoppedByUser) {
            val restart = Intent(applicationContext, CompanionService::class.java)
                .setAction(ACTION_START)
                .putExtra(EXTRA_ADDRESS, address)
            runCatching { applicationContext.startForegroundService(restart) }
        }
        super.onTaskRemoved(rootIntent)
    }

    /** Connect, feed, and on any loss connect again, backing off to one minute. */
    private suspend fun runConnectionLoop() {
        var backoff = 2_000L
        while (scope.isActive && !stoppedByUser) {
            var misses = 0
            var link: WatchLink? = null
            var heartbeatJob: Job? = null
            var receiverJob: Job? = null
            try {
                val address = watchAddress ?: error("No watch selected")
                val device = bondedDevice(address)
                val prefs = getSharedPreferences("remote", MODE_PRIVATE)
                val ha = buildHa(prefs)
                report("Connecting to watch · Nothing X stays paired")
                link = WatchLink(this, device)
                link.open()
                backoff = 2_000L

                val status = firmwareStatus(link)
                val haReady = ha != null
                report(if (haReady) "Connected · Home Assistant ready · $status" else "Watch connected · Home Assistant unavailable · $status")
                var haWasUnavailable = !haReady

                receiverJob = scope.launch { receiveEvents(link, ha) }
                heartbeatJob = scope.launch {
                    while (isActive && link.connected) {
                        delay(10_000)
                        runCatching {
                            if (link.ping() != null) link.send(WatchAppProtocol.heartbeat())
                        }
                    }
                }

                if (link.ping()?.let { it.first != 0xFF && it.second and 3 == 1 } == true) {
                    publishConfigAndStates(link, ha)
                }

                while (scope.isActive && link.connected) {
                    if (receiverJob?.isActive != true) error("Watch message handler stopped")
                    delay(5_000)
                    // A single missed ping is a transient radio hiccup; only give up after
                    // three in a row so the background link survives short interference.
                    val pong = link.ping()
                    if (pong == null) {
                        if (++misses >= 3) error("Watch channel stopped responding")
                        continue
                    }
                    misses = 0
                    if (pong.first != 0xFF && pong.second and 3 == 1) {
                        val unavailable = syncStates(link, ha)
                        if (unavailable > 0) {
                            report("Watch connected · $unavailable Home Assistant control(s) unavailable")
                        } else if (haWasUnavailable) {
                            haWasUnavailable = false
                            report("Connected · Home Assistant ready · $status")
                        }
                    }
                }
                error("Watch connection closed")
            } catch (e: CancellationException) {
                throw e
            } catch (e: SecurityException) {
                report("Bluetooth permission is required; open the app to grant it")
            } catch (e: Exception) {
                report("Reconnecting · " + (e.message ?: "watch or Home Assistant unavailable"))
            } finally {
                heartbeatJob?.cancel()
                receiverJob?.cancel()
                link?.close()
            }
            delay(backoff)
            backoff = (backoff * 2).coerceAtMost(60_000L)
        }
    }

    private fun firmwareStatus(link: WatchLink): String = when (val info = link.stubInfo) {
        null -> "stub version unknown"
        else -> if (info.supportsAppWidget) "stub " + info.version + " · widget-capable" else "stub " + info.version
    }

    /** Returns a ready HaApi, or null when Home Assistant is not configured or unreachable. */
    private suspend fun buildHa(prefs: android.content.SharedPreferences): HaApi? {
        val url = prefs.getString("url", "")!!.trim()
        val token = runCatching { decryptToken() }.getOrDefault("")
        if (!(url.startsWith("https://") || url.startsWith("http://")) || token.isBlank()) return null
        val ha = HaApi(url, token)
        return if (runCatching { ha.validate() }.isSuccess) ha else null
    }

    @Suppress("MissingPermission")
    private fun bondedDevice(address: String): android.bluetooth.BluetoothDevice {
        if (Build.VERSION.SDK_INT >= 31 &&
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_CONNECT) != android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            throw SecurityException("Bluetooth permission is required")
        }
        val adapter = getSystemService(BluetoothManager::class.java).adapter
            ?: error("Bluetooth unavailable")
        return adapter.bondedDevices.firstOrNull { it.address == address }
            ?: error("Watch is no longer bonded")
    }

    private suspend fun receiveEvents(link: WatchLink, ha: HaApi?) {
        for (message in link.events) {
            when {
                message.contentEquals(byteArrayOf('H'.code.toByte(), '1'.code.toByte(), '?'.code.toByte())) -> {
                    recentRequests.clear()
                    publishConfigAndStates(link, ha)
                }
                message.size == 4 && message[0] == 'H'.code.toByte() && message[1] == '1'.code.toByte() &&
                    message[2] == 'W'.code.toByte() -> {
                    val result = message[3].toInt()
                    if (widgetProbeResult != result) {
                        widgetProbeResult = result
                        when (result) {
                            0 -> report("Watch app widget API available")
                            -22 -> report("Watch app widget unsupported · update App Loader")
                            else -> report("Watch app widget returned error $result")
                        }
                    }
                }
                WatchAppProtocol.parseToggle(message) != null -> handleToggle(link, ha, WatchAppProtocol.parseToggle(message)!!)
                WatchAppProtocol.parseBrightness(message) != null -> handleBrightness(link, ha, WatchAppProtocol.parseBrightness(message)!!)
                else -> Unit
            }
        }
    }

    private suspend fun handleToggle(link: WatchLink, ha: HaApi?, toggle: WatchAppProtocol.Toggle) {
        val seq = toggle.sequence
        val slot = toggle.slot
        val prefs = getSharedPreferences("remote", MODE_PRIVATE)
        if (slot !in 0 until WatchAppProtocol.SLOTS || !prefs.getBoolean("enabled" + slot, false)) {
            sendError(link, seq, 1)
            return
        }
        val now = android.os.SystemClock.elapsedRealtime()
        synchronized(recentRequests) { recentRequests.entries.removeAll { now - it.value > 15_000 } }
        if (synchronized(recentRequests) { recentRequests.containsKey(seq) }) return
        if (!busy.compareAndSet(false, true)) {
            sendError(link, seq, 1)
            return
        }
        synchronized(recentRequests) { recentRequests[seq] = now }
        try {
            link.send(WatchAppProtocol.ack(seq))
            if (ha == null) {
                val cached = prefs.getBoolean("state_" + slot, false)
                val cachedValue = prefs.getInt("brightness_$slot", if (cached) 100 else 0)
                val cachedAvail = prefs.getBoolean("avail_$slot", true)
                runCatching { link.send(WatchAppProtocol.state(slot, cached, cachedValue, 1, cachedAvail)) }
                prefs.edit().putBoolean("state_" + slot, cached).apply()
                return
            }
            val entity = prefs.getString("entity" + slot, "")!!.trim()
            val confirmed = withContext(Dispatchers.IO) {
                ha.set(entity, toggle.on)
                ha.awaitState(entity, toggle.on)
            }
            val st = runCatching { ha.state(entity) }
                .getOrDefault(HaApi.EntityState(false, 0, available = false))
            prefs.edit()
                .putBoolean("state_$slot", st.on)
                .putInt("brightness_$slot", st.value)
                .putBoolean("avail_$slot", st.available)
                .apply()
            link.send(WatchAppProtocol.state(slot, st.on, st.value, if (confirmed) 0 else 2, st.available))
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            val cached = prefs.getBoolean("state_" + slot, false)
            val cachedValue = prefs.getInt("brightness_$slot", if (cached) 100 else 0)
            runCatching { link.send(WatchAppProtocol.state(slot, cached, cachedValue, 1, false)) }
            prefs.edit().putBoolean("avail_$slot", false).apply()
        } finally {
            busy.set(false)
        }
    }

    /** A brightness drag from the watch: set it, confirm the reported value, publish state. */
    private suspend fun handleBrightness(link: WatchLink, ha: HaApi?, request: WatchAppProtocol.Brightness) {
        val seq = request.sequence
        val slot = request.slot
        val prefs = getSharedPreferences("remote", MODE_PRIVATE)
        if (slot !in 0 until WatchAppProtocol.SLOTS || !prefs.getBoolean("enabled$slot", false)) {
            sendError(link, seq, 1)
            return
        }
        val now = android.os.SystemClock.elapsedRealtime()
        synchronized(recentRequests) { recentRequests.entries.removeAll { now - it.value > 15_000 } }
        if (synchronized(recentRequests) { recentRequests.containsKey(seq) }) return
        if (!busy.compareAndSet(false, true)) {
            sendError(link, seq, 1)
            return
        }
        synchronized(recentRequests) { recentRequests[seq] = now }
        try {
            link.send(WatchAppProtocol.ack(seq))
            if (ha == null) {
                val cachedOn = prefs.getBoolean("state_$slot", false)
                val cachedValue = prefs.getInt("brightness_$slot", if (cachedOn) 100 else 0)
                val cachedAvail = prefs.getBoolean("avail_$slot", true)
                runCatching { link.send(WatchAppProtocol.state(slot, cachedOn, cachedValue, 1, cachedAvail)) }
                return
            }
            val entity = prefs.getString("entity$slot", "")!!.trim()
            val confirmed = withContext(Dispatchers.IO) {
                ha.setBrightness(entity, request.value)
                ha.awaitBrightness(entity, request.value)
            }
            val st = runCatching { ha.state(entity) }
                .getOrDefault(HaApi.EntityState(false, 0, available = false))
            prefs.edit()
                .putBoolean("state_$slot", st.on)
                .putInt("brightness_$slot", st.value)
                .putBoolean("avail_$slot", st.available)
                .apply()
            link.send(WatchAppProtocol.state(slot, st.on, st.value, if (confirmed) 0 else 2, st.available))
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            val cachedOn = prefs.getBoolean("state_$slot", false)
            val cachedValue = prefs.getInt("brightness_$slot", if (cachedOn) 100 else 0)
            runCatching { link.send(WatchAppProtocol.state(slot, cachedOn, cachedValue, 1, false)) }
            prefs.edit().putBoolean("avail_$slot", false).apply()
        } finally {
            busy.set(false)
        }
    }

    private suspend fun publishConfigAndStates(link: WatchLink, ha: HaApi?) {
        val prefs = getSharedPreferences("remote", MODE_PRIVATE)
        val slots = (0 until WatchAppProtocol.SLOTS).map { i ->
            val entity = prefs.getString("entity$i", "")!!.trim()
            val kind = if (prefs.contains("kind$i")) {
                prefs.getInt("kind$i", WatchAppProtocol.TOGGLE)
            } else if (entity.startsWith("light.")) WatchAppProtocol.DIMMER else WatchAppProtocol.TOGGLE
            WatchAppProtocol.Slot(
                prefs.getBoolean("enabled$i", false),
                prefs.getString("name$i", "")!!.trim().ifBlank { "Control " + (i + 1) },
                prefs.getString("sec$i", "")!!.trim(),
                kind
            )
        }
        val halfA = slots.take(5)
        val halfB = slots.drop(5)
        // V, J, K configure; U is the commit frame the watch persists.
        if (link.send(WatchAppProtocol.configB(halfB)) != WatchLink.STATUS_OK) error("Could not send controls to watch")
        if (link.send(WatchAppProtocol.sectionsA(halfA)) != WatchLink.STATUS_OK) error("Could not send sections to watch")
        if (link.send(WatchAppProtocol.sectionsB(halfB)) != WatchLink.STATUS_OK) error("Could not send sections to watch")
        if (link.send(WatchAppProtocol.configA(halfA, favoriteSlots(prefs))) != WatchLink.STATUS_OK) error("Could not send controls to watch")
        if (ha != null) syncStates(link, ha, force = true)
    }

    private fun favoriteSlots(prefs: android.content.SharedPreferences): List<Int> {
        val stored = prefs.getStringSet("favs", null)
        if (stored != null) {
            return stored.mapNotNull { it.toIntOrNull() }
                .filter { it in 0 until WatchAppProtocol.SLOTS }
                .distinct().sorted()
                .take(WatchAppProtocol.FAVORITES)
        }
        // migration from releases with a single favorite
        val legacy = prefs.getInt("fav", -1)
        return if (legacy in 0 until WatchAppProtocol.SLOTS) listOf(legacy) else emptyList()
    }

    /** Pushes current Home Assistant states; returns how many entities were unreachable. */
    private suspend fun syncStates(link: WatchLink, ha: HaApi?, force: Boolean = false): Int {
        if (ha == null) return 0
        val prefs = getSharedPreferences("remote", MODE_PRIVATE)
        var unavailable = 0
        for (i in 0 until WatchAppProtocol.SLOTS) {
            if (!prefs.getBoolean("enabled$i", false)) continue
            val entity = prefs.getString("entity$i", "")!!.trim()
            val read = runCatching { withContext(Dispatchers.IO) { ha.state(entity) } }
            val state = read.getOrNull()
            if (state == null || !state.available) {
                // No data for this entity: the device lost power or was deleted. Mark the
                // watch row Inactive once, then leave it until HA reports it again.
                unavailable++
                val wasAvailable = prefs.getBoolean("avail_$i", true)
                if (force || wasAvailable) {
                    val cachedOn = prefs.getBoolean("state_$i", false)
                    val cachedValue = prefs.getInt("brightness_$i", if (cachedOn) 100 else 0)
                    val on = state?.on ?: cachedOn
                    val value = state?.value ?: cachedValue
                    val result = if (read.isSuccess) 0 else 1
                    runCatching { link.send(WatchAppProtocol.state(i, on, value, result, false)) }
                    prefs.edit()
                        .putBoolean("avail_$i", false)
                        .putBoolean("state_$i", on)
                        .putInt("brightness_$i", value)
                        .apply()
                }
                continue
            }
            val changed = prefs.getBoolean("state_$i", !state.on) != state.on ||
                prefs.getInt("brightness_$i", if (state.on) 100 else 0) != state.value ||
                !prefs.getBoolean("avail_$i", true)
            if (force || changed) {
                // A failed push is retried by the next sync; never let it escape and crash.
                runCatching { link.send(WatchAppProtocol.state(i, state.on, state.value, if (read.isSuccess) 0 else 1, true)) }
                if (read.isSuccess) {
                    prefs.edit().putBoolean("state_$i", state.on)
                        .putInt("brightness_$i", state.value)
                        .putBoolean("avail_$i", true)
                        .apply()
                }
            }
        }
        return unavailable
    }

    private suspend fun sendError(link: WatchLink, seq: Int, code: Int) {
        runCatching { link.send(WatchAppProtocol.error(seq, code)) }
    }

    private fun report(message: String) {
        getSharedPreferences("remote", MODE_PRIVATE).edit().putString("connection_status", message).apply()
        getSystemService(NotificationManager::class.java)?.notify(NOTIFICATION_ID, notification(message))
        sendBroadcast(Intent(ACTION_STATUS).setPackage(packageName).putExtra(EXTRA_STATUS, message))
    }

    private fun notification(message: String): Notification = NotificationCompat.Builder(this, CHANNEL)
        .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
        .setContentTitle("Home Assistant Watch Remote")
        .setContentText(message)
        .setOngoing(true)
        .setCategory(NotificationCompat.CATEGORY_SERVICE)
        .build()

    private fun key(): SecretKey {
        val store = java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey("ha-token", null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(
                KeyGenParameterSpec.Builder("ha-token", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build()
            )
        }.generateKey()
    }

    private fun decryptToken(): String {
        val prefs = getSharedPreferences("secure", MODE_PRIVATE)
        val iv = prefs.getString("iv", null) ?: return ""
        val data = prefs.getString("token", null) ?: return ""
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(
            Cipher.DECRYPT_MODE,
            key(),
            GCMParameterSpec(128, android.util.Base64.decode(iv, android.util.Base64.NO_WRAP))
        )
        return cipher.doFinal(android.util.Base64.decode(data, android.util.Base64.NO_WRAP))
            .toString(StandardCharsets.UTF_8)
    }

    companion object {
        const val ACTION_START = "dev.cmf.ha.START"
        const val ACTION_STATUS = "dev.cmf.ha.STATUS"
        const val EXTRA_ADDRESS = "watch_address"
        const val EXTRA_STATUS = "status"
        private const val CHANNEL = "watch_connection"
        private const val NOTIFICATION_ID = 41
    }
}

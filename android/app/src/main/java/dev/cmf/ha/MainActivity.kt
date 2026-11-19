package dev.cmf.ha

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.ArrayAdapter
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import com.google.android.material.button.MaterialButton
import com.google.android.material.card.MaterialCardView
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.nio.charset.StandardCharsets
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Material 3 setup screen, laid out as numbered steps: pick the watch, link Home Assistant,
 * name the five controls and choose a favorite. onCreate only builds views; every subsystem
 * call is guarded, so the app always opens.
 */
class MainActivity : Activity() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val prefs by lazy { getSharedPreferences("remote", MODE_PRIVATE) }
    private val securePrefs by lazy { getSharedPreferences("secure", MODE_PRIVATE) }

    private lateinit var status: TextView
    private lateinit var watchSummary: TextView
    private lateinit var haUrl: TextInputEditText
    private lateinit var haToken: TextInputEditText
    private val names by lazy { Array(5) { TextInputEditText(this) } }
    private val entities by lazy { Array(5) { TextInputEditText(this) } }
    private val sections by lazy { Array(5) { TextInputEditText(this) } }
    private val switches by lazy { Array(5) { MaterialSwitch(this) } }
    private lateinit var favoriteSpinner: Spinner
    private var devices: List<BluetoothDevice> = emptyList()
    private var selectedAddress: String? = null

    private val statusReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            intent?.getStringExtra(CompanionService.EXTRA_STATUS)?.let { status.text = it }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        buildUi()
        loadSettings()
    }

    override fun onResume() {
        super.onResume()
        runCatching {
            if (Build.VERSION.SDK_INT >= 33) {
                registerReceiver(statusReceiver, IntentFilter(CompanionService.ACTION_STATUS), Context.RECEIVER_NOT_EXPORTED)
            } else {
                @Suppress("DEPRECATION")
                registerReceiver(statusReceiver, IntentFilter(CompanionService.ACTION_STATUS))
            }
        }
        status.text = prefs.getString("connection_status", "Not connected") ?: "Not connected"
        askPermissions()
        refreshWatches()
    }

    override fun onPause() {
        runCatching { unregisterReceiver(statusReceiver) }
        super.onPause()
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    @Deprecated("Deprecated in Java")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        refreshWatches()
    }

    // ---------- small view helpers ----------

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    private fun label(text: String, size: Float = 15f, bold: Boolean = false) = TextView(this).apply {
        this.text = text
        textSize = size
        if (bold) setTypeface(typeface, Typeface.BOLD)
    }

    private fun field(hint: String, value: String = "", secret: Boolean = false): TextInputLayout {
        val layout = TextInputLayout(this)
        val edit = TextInputEditText(this).apply {
            this.hint = hint
            setSingleLine(true)
            setText(value)
            if (secret) inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
        }
        layout.addView(edit)
        return layout
    }

    /** Adds a Material text field to [box] and returns its edit text. */
    private fun fieldInto(box: LinearLayout, hint: String, secret: Boolean = false): TextInputEditText {
        val layout = field(hint, secret = secret)
        box.addView(layout)
        return layout.editText as TextInputEditText
    }

    private fun button(text: String, action: () -> Unit) = MaterialButton(this).apply {
        this.text = text
        setOnClickListener { runCatching(action).onFailure { status.text = "Error: " + (it.message ?: it.javaClass.simpleName) } }
    }

    private fun card(titleText: String): LinearLayout {
        val card = MaterialCardView(this)
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(12), dp(16), dp(16))
        }
        card.addView(box)
        box.addView(label(titleText, 16f, bold = true))
        return box
    }

    /** The Material card wrapping a section box; the box is already inside it. */
    private fun ViewGroup.addCard(box: LinearLayout) {
        addView(box.parent as ViewGroup)
    }

    private fun space(root: ViewGroup, height: Int) {
        root.addView(View(this), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(height)))
    }

    // ---------- screen ----------

    private fun buildUi() {
        val scroll = ScrollView(this)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(20), dp(16), dp(24))
        }
        scroll.addView(root)
        setContentView(scroll)

        val title = label("Home Control", 24f, bold = true)
        root.addView(title)
        status = label("Not connected")
        root.addView(status)
        space(root, 8)

        val watchBox = card("1 · Your watch")
        watchSummary = label("No watch selected")
        watchBox.addView(watchSummary)
        space(watchBox, 8)
        watchBox.addView(button("Choose watch") { chooseWatch() })
        root.addCard(watchBox)
        space(root, 12)

        val haBox = card("2 · Home Assistant")
        haUrl = fieldInto(haBox, "http://homeassistant.local:8123")
        haToken = fieldInto(haBox, "Long-lived access token", secret = true)
        space(haBox, 8)
        val haButtons = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        haButtons.addView(button("Test") { testHomeAssistant() }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        haButtons.addView(button("Load lights") { loadEntities() }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        haBox.addView(haButtons)
        haBox.addView(label("Plain HTTP works on your home network; the token is then not encrypted.", 12f))
        root.addCard(haBox)
        space(root, 12)

        val controlsBox = card("3 · Quick controls (up to five)")
        controlsBox.addView(label("Names appear on the watch. Keep them short and non-sensitive.", 12f))
        for (i in 0..4) {
            space(controlsBox, 8)
            switches[i] = MaterialSwitch(this).apply { text = "Control " + (i + 1) }
            controlsBox.addView(switches[i])
            names[i] = fieldInto(controlsBox, "Watch label (max 15 bytes)")
            entities[i] = fieldInto(controlsBox, "light.desk or group.downstairs")
            sections[i] = fieldInto(controlsBox, "Section (optional, max 10 bytes)")
        }
        space(controlsBox, 12)
        controlsBox.addView(label("Favorite — shown on the watch's widget card", 14f, bold = true))
        favoriteSpinner = Spinner(this)
        favoriteSpinner.adapter = ArrayAdapter(
            this,
            android.R.layout.simple_spinner_dropdown_item,
            arrayOf("None", "Control 1", "Control 2", "Control 3", "Control 4", "Control 5")
        )
        controlsBox.addView(favoriteSpinner)
        root.addCard(controlsBox)
        space(root, 16)

        root.addView(button("Save and connect") { connect() })
        space(root, 4)
        root.addView(button("Disconnect") { disconnect() })
        space(root, 4)
        root.addView(button("Keep running in background") { requestBatteryExemption() })
    }

    // ---------- permissions and devices ----------

    private fun askPermissions() {
        val wanted = buildList {
            if (Build.VERSION.SDK_INT >= 31 &&
                checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED
            ) add(Manifest.permission.BLUETOOTH_CONNECT)
            if (Build.VERSION.SDK_INT >= 33 &&
                checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            ) add(Manifest.permission.POST_NOTIFICATIONS)
        }
        if (wanted.isNotEmpty()) requestPermissions(wanted.toTypedArray(), 1)
    }

    @Suppress("MissingPermission")
    private fun deviceName(device: BluetoothDevice): String =
        runCatching { device.name }.getOrNull().orEmpty()

    @Suppress("MissingPermission")
    private fun refreshWatches() {
        try {
            val adapter = getSystemService(BluetoothManager::class.java)?.adapter
            if (adapter == null) {
                devices = emptyList()
                status.text = "Bluetooth is unavailable on this phone"
                return
            }
            if (!adapter.isEnabled) {
                devices = emptyList()
                status.text = "Turn on Bluetooth, then reopen the app"
                return
            }
            // Watches first: the old list put CMF Headphones ahead of the watch alphabetically.
            devices = adapter.bondedDevices.orEmpty()
                .sortedWith(
                    compareByDescending<BluetoothDevice> { deviceName(it).contains("watch", true) }
                        .thenBy { deviceName(it) }
                )
            selectedAddress = prefs.getString("watch", null)
            updateWatchSummary()
        } catch (e: SecurityException) {
            status.text = "Allow the Bluetooth permission, then reopen the app"
        } catch (e: Exception) {
            status.text = "Bluetooth error: " + (e.message ?: e.javaClass.simpleName)
        }
    }

    private fun updateWatchSummary() {
        val selected = devices.firstOrNull { it.address == selectedAddress }
        watchSummary.text = when {
            selected != null -> "Connected watch: " + (deviceName(selected).ifBlank { "Unknown" }) + " · " + selected.address
            selectedAddress != null -> "Saved watch is not bonded any more — choose it again"
            else -> "No watch selected. Pair it once in Nothing X first."
        }
    }

    @Suppress("MissingPermission")
    private fun chooseWatch() {
        if (devices.isEmpty()) {
            status.text = "No bonded devices. Pair the watch once through Nothing X first."
            return
        }
        val watchLike = devices.filter { deviceName(it).contains("watch", true) }
        val others = devices.filterNot { deviceName(it).contains("watch", true) }
        val ordered = watchLike + others
        val entries = ordered.mapIndexed { index, device ->
            val name = deviceName(device).ifBlank { "Unknown" }
            val tag = if (index < watchLike.size) "⌚ " else "   "
            tag + name + "  ·  " + device.address
        }.toTypedArray()
        val preselect = ordered.indexOfFirst { it.address == selectedAddress }
        MaterialAlertDialogBuilder(this)
            .setTitle("Choose your watch")
            .setSingleChoiceItems(entries, preselect) { dialog, which ->
                selectedAddress = ordered[which].address
                prefs.edit().putString("watch", selectedAddress).apply()
                updateWatchSummary()
                dialog.dismiss()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    // ---------- settings ----------

    private fun loadSettings() {
        haUrl.setText(prefs.getString("url", "http://homeassistant.local:8123"))
        haToken.setText(runCatching { decryptToken() }.getOrDefault(""))
        for (i in 0..4) {
            switches[i].isChecked = prefs.getBoolean("enabled$i", false)
            names[i].setText(prefs.getString("name$i", ""))
            entities[i].setText(prefs.getString("entity$i", ""))
            sections[i].setText(prefs.getString("sec$i", ""))
        }
        val fav = prefs.getInt("fav", -1)
        favoriteSpinner.setSelection((fav + 1).coerceIn(0, 5))
    }

    private fun saveSettings() {
        val editor = prefs.edit().putString("url", haUrl.text.toString().trim())
        for (i in 0..4) {
            editor.putBoolean("enabled$i", switches[i].isChecked)
                .putString("name$i", names[i].text.toString().trim())
                .putString("entity$i", entities[i].text.toString().trim())
                .putString("sec$i", sections[i].text.toString().trim())
        }
        editor.putInt("fav", favoriteSpinner.selectedItemPosition - 1)
        editor.apply()
        runCatching { encryptToken(haToken.text.toString().trim()) }
    }

    private fun testHomeAssistant() {
        val url = haUrl.text.toString().trim()
        val token = haToken.text.toString().trim()
        if (!isHttpUrl(url) || token.isEmpty()) {
            status.text = "Enter an http:// or https:// address and token first"
            return
        }
        scope.launch {
            try {
                status.text = "Testing Home Assistant connection…"
                val count = withContext(Dispatchers.IO) { HaApi(url, token).also { it.validate() }.entities().size }
                status.text = "Home Assistant connected · $count light entities and groups found"
            } catch (e: Exception) {
                status.text = "Home Assistant connection failed: " + (e.message ?: "check address and token")
            }
        }
    }

    private fun loadEntities() {
        val url = haUrl.text.toString().trim()
        val token = haToken.text.toString().trim()
        if (!isHttpUrl(url) || token.isEmpty()) {
            status.text = "Enter the http:// or https:// address and token first"
            return
        }
        scope.launch {
            try {
                status.text = "Loading Home Assistant lights…"
                val list = withContext(Dispatchers.IO) { HaApi(url, token).entities() }
                if (list.isEmpty()) {
                    status.text = "No light entities or light groups found"
                    return@launch
                }
                val choices = list.map { (id, name) -> if (name.isBlank()) id else "$name · $id" }.toTypedArray()
                MaterialAlertDialogBuilder(this@MainActivity)
                    .setTitle("Choose a control to configure")
                    .setItems(Array(5) { "Control " + (it + 1) }) { _, slot ->
                        MaterialAlertDialogBuilder(this@MainActivity)
                            .setTitle("Select a light or light group")
                            .setItems(choices) { _, which ->
                                entities[slot].setText(list[which].first)
                                if (names[slot].text.isNullOrBlank()) names[slot].setText("Control " + (slot + 1))
                            }
                            .setNegativeButton("Cancel", null)
                            .show()
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
                status.text = "Found " + list.size + " light entities and groups"
            } catch (e: Exception) {
                status.text = "Home Assistant error: " + (e.message ?: "request failed")
            }
        }
    }

    // ---------- connect ----------

    private fun isHttpUrl(url: String) = url.startsWith("https://") || url.startsWith("http://")

    private fun connect() {
        if (Build.VERSION.SDK_INT >= 31 &&
            checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED
        ) {
            status.text = "Allow the Bluetooth permission first"
            requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT), 1)
            return
        }
        val address = selectedAddress
        if (address == null) {
            status.text = "Choose your watch first"
            return
        }
        val url = haUrl.text.toString().trim()
        if (!isHttpUrl(url)) {
            status.text = "Use an http:// or https:// Home Assistant address"
            return
        }
        if (haToken.text.toString().trim().isEmpty()) {
            status.text = "Enter a Home Assistant token"
            return
        }
        for (i in 0..4) {
            if (!switches[i].isChecked) continue
            if (names[i].text.toString().trim().toByteArray(StandardCharsets.UTF_8).size > 15) {
                status.text = "Control " + (i + 1) + ": label must be 15 UTF-8 bytes or fewer"
                return
            }
            if (!entities[i].text.toString().trim().matches(Regex("(light|group)\\.[A-Za-z0-9_]+"))) {
                status.text = "Control " + (i + 1) + ": choose a Home Assistant light or light group"
                return
            }
            if (sections[i].text.toString().trim().toByteArray(StandardCharsets.UTF_8).size > 10) {
                status.text = "Control " + (i + 1) + ": section must be 10 UTF-8 bytes or fewer"
                return
            }
        }
        saveSettings()
        status.text = if (url.startsWith("http://")) {
            "Starting background watch connection… · plain HTTP: the token is not encrypted on your network"
        } else {
            "Starting background watch connection…"
        }
        val service = Intent(this, CompanionService::class.java)
            .setAction(CompanionService.ACTION_START)
            .putExtra(CompanionService.EXTRA_ADDRESS, address)
        if (Build.VERSION.SDK_INT >= 26) startForegroundService(service) else startService(service)
    }

    private fun disconnect() {
        prefs.edit().putString("connection_status", "Disconnected").apply()
        stopService(Intent(this, CompanionService::class.java))
        status.text = "Disconnected"
    }

    private fun requestBatteryExemption() {
        try {
            val pm = getSystemService(android.os.PowerManager::class.java)
            if (pm?.isIgnoringBatteryOptimizations(packageName) == true) {
                status.text = "Already exempt from battery optimization"
                return
            }
            @SuppressLint("BatteryLife")
            val intent = android.content.Intent(
                android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                android.net.Uri.parse("package:" + packageName)
            )
            startActivity(intent)
        } catch (e: Exception) {
            status.text = "Allow background use manually: Settings → Battery → HA Watch Remote"
        }
    }

    // ---------- token storage ----------

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

    private fun encryptToken(value: String) {
        if (value.isEmpty()) {
            securePrefs.edit().clear().apply()
            return
        }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        securePrefs.edit()
            .putString("iv", android.util.Base64.encodeToString(cipher.iv, android.util.Base64.NO_WRAP))
            .putString("token", android.util.Base64.encodeToString(cipher.doFinal(value.toByteArray()), android.util.Base64.NO_WRAP))
            .apply()
    }

    private fun decryptToken(): String {
        val iv = securePrefs.getString("iv", null) ?: return ""
        val data = securePrefs.getString("token", null) ?: return ""
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, android.util.Base64.decode(iv, android.util.Base64.NO_WRAP)))
        return cipher.doFinal(android.util.Base64.decode(data, android.util.Base64.NO_WRAP))
            .toString(StandardCharsets.UTF_8)
    }
}


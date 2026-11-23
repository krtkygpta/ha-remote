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
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import android.widget.ArrayAdapter
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import com.google.android.material.button.MaterialButton
import com.google.android.material.bottomnavigation.BottomNavigationView
import com.google.android.material.card.MaterialCardView
import com.google.android.material.checkbox.MaterialCheckBox
import com.google.android.material.color.MaterialColors
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.MaterialAutoCompleteTextView
import com.google.android.material.textfield.TextInputLayout
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CancellationException
import java.nio.charset.StandardCharsets
import org.json.JSONArray
import org.json.JSONObject
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Material 3 setup screen, laid out as numbered steps: pick the watch, link Home Assistant,
 * name up to ten controls and pick up to five favorites. onCreate only builds views; every subsystem
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
    private lateinit var installerSummary: TextView
    private lateinit var installerSlot: Spinner
    private val names by lazy { Array(10) { TextInputEditText(this) } }
    private val entities by lazy { Array(10) { MaterialAutoCompleteTextView(this) } }
    private val sections by lazy { Array(10) { TextInputEditText(this) } }
    private val switches by lazy { Array(10) { MaterialSwitch(this) } }
    private val kinds = arrayOfNulls<Spinner>(10)
    private val icons = arrayOfNulls<Spinner>(10)
    private val favoriteChecks = arrayOfNulls<MaterialCheckBox>(10)
    private var devices: List<BluetoothDevice> = emptyList()
    private var selectedAddress: String? = null
    private var packageBytes: ByteArray? = null
    private var packageHeader: Install.Header? = null
    private var entityChoices: List<Pair<String, String>> = emptyList()
    private var entitiesLoading = false

    private val statusReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            intent?.getStringExtra(CompanionService.EXTRA_STATUS)?.let { status.text = it }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        buildUi()
        loadSettings()
        showOnboardingIfNeeded()
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
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK) return
        if (requestCode == EXPORT_BACKUP) {
            val uri = data?.data ?: return
            runCatching { contentResolver.openOutputStream(uri)?.bufferedWriter()?.use { it.write(createBackup().toString(2)) } ?: error("Could not write backup") }
                .onSuccess { status.text = "Settings backup saved. Your Home Assistant token is stored separately and was not included." }
                .onFailure { status.text = "Backup failed: ${it.message ?: it.javaClass.simpleName}" }
            return
        }
        if (requestCode == IMPORT_BACKUP) {
            val uri = data?.data ?: return
            runCatching {
                val json = contentResolver.openInputStream(uri)?.bufferedReader()?.use { JSONObject(it.readText()) }
                    ?: error("Could not read backup")
                restoreBackup(json)
            }.onSuccess {
                loadSettings()
                status.text = "Settings restored. Enter your Home Assistant token, then connect."
            }.onFailure { status.text = "Restore failed: ${it.message ?: it.javaClass.simpleName}" }
            return
        }
        if (requestCode != PICK_PACKAGE) return
        try {
            val bytes = data?.data?.let { uri -> contentResolver.openInputStream(uri)?.use { it.readBytes() } }
                ?: error("Could not read the selected file")
            val header = Install.parsePackage(bytes)
            require(header.kind == Install.KIND_APP) { "Only .wp2app applications can be installed" }
            packageBytes = bytes
            packageHeader = header
            installerSummary.text = "${header.name} · v${Install.versionText(header.version)} · ${bytes.size / 1024} KB"
            status.text = "App package ready to install"
        } catch (e: Exception) {
            packageBytes = null
            packageHeader = null
            installerSummary.text = "No app package selected"
            status.text = "Cannot use this file: ${e.message ?: e.javaClass.simpleName}"
        }
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

    private fun entityFieldInto(box: LinearLayout, hint: String): MaterialAutoCompleteTextView {
        val layout = TextInputLayout(this).apply {
            this.hint = hint
            endIconMode = TextInputLayout.END_ICON_DROPDOWN_MENU
        }
        val input = MaterialAutoCompleteTextView(this).apply {
            setSingleLine(true)
            threshold = 1
            setAdapter(entityAdapter())
            setOnFocusChangeListener { _, focused -> if (focused && entityChoices.isEmpty()) loadEntities() }
            setOnItemClickListener { parent, _, position, _ ->
                val choice = parent.getItemAtPosition(position).toString()
                setText(choice.substringAfter(" · ", choice), false)
            }
        }
        layout.addView(input)
        box.addView(layout)
        return input
    }

    private fun entityAdapter() = ArrayAdapter(
        this,
        android.R.layout.simple_dropdown_item_1line,
        entityChoices.map { (id, name) -> if (name.isBlank()) id else "$name · $id" }
    )

    private fun iconResource(position: Int) = when (position) {
        WatchAppProtocol.ICON_LIGHT_BULB -> R.drawable.icon_light_bulb
        WatchAppProtocol.ICON_CEILING_LIGHT -> R.drawable.icon_ceiling_light
        WatchAppProtocol.ICON_FAN -> R.drawable.icon_fan
        WatchAppProtocol.ICON_DEVICE -> R.drawable.icon_device
        else -> R.drawable.icon_device
    }

    private fun iconChoiceView(position: Int, dropDown: Boolean): View = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(8), if (dropDown) dp(4) else 0, dp(8), if (dropDown) dp(4) else 0)
        val image = ImageView(this@MainActivity).apply {
            setImageResource(iconResource(position))
            setColorFilter(MaterialColors.getColor(this, com.google.android.material.R.attr.colorOnSurfaceVariant))
            contentDescription = when (position) {
                WatchAppProtocol.ICON_LIGHT_BULB -> "Light bulb Material icon"
                WatchAppProtocol.ICON_CEILING_LIGHT -> "Ceiling light Material icon"
                WatchAppProtocol.ICON_FAN -> "Fan Material icon"
                WatchAppProtocol.ICON_DEVICE -> "Device Material icon"
                else -> "Automatic Material icon"
            }
        }
        addView(image, LinearLayout.LayoutParams(dp(32), dp(32)).apply { marginEnd = dp(12) })
        addView(label(arrayOf("Auto", "Light bulb", "Ceiling light", "Fan", "Device").getOrElse(position) { "Auto" }, 14f))
    }

    private fun iconAdapter() = object : ArrayAdapter<String>(
        this, android.R.layout.simple_spinner_dropdown_item,
        arrayOf("Auto", "Light bulb", "Ceiling light", "Fan", "Device")
    ) {
        override fun getView(position: Int, convertView: View?, parent: ViewGroup) = iconChoiceView(position, false)
        override fun getDropDownView(position: Int, convertView: View?, parent: ViewGroup) = iconChoiceView(position, true)
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
        val scaffold = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(12), dp(16), dp(24))
        }
        val title = label("Home Control", 24f, bold = true)
        root.addView(title)
        status = label("Not connected")
        root.addView(status)
        space(root, 8)

        val homePage = ScrollView(this).apply { addView(root) }
        val controlsRoot = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(20), dp(16), dp(24))
        }
        controlsRoot.addView(label("Your controls", 24f, bold = true))
        controlsRoot.addView(label("Choose up to ten Home Assistant entities for your watch.", 14f))
        controlsRoot.addView(button("Load searchable entities from Home Assistant") { loadEntities() })
        space(controlsRoot, 8)
        val controlsPage = ScrollView(this).apply { addView(controlsRoot) }
        val installRoot = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(20), dp(16), dp(24))
        }
        installRoot.addView(label("Watch app installer", 24f, bold = true))
        installRoot.addView(label("Send HA Remote to your paired watch over Bluetooth LE.", 14f))
        space(installRoot, 8)
        val installPage = ScrollView(this).apply { addView(installRoot) }
        val pages = listOf(homePage, controlsPage, installPage)
        pages.forEachIndexed { index, view ->
            scaffold.addView(view, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            if (index != 0) view.visibility = View.GONE
        }
        val navigation = BottomNavigationView(this)
        navigation.menu.add(0, 1, 0, "Home").setIcon(android.R.drawable.ic_menu_view)
        navigation.menu.add(0, 2, 1, "Controls").setIcon(android.R.drawable.ic_menu_manage)
        navigation.menu.add(0, 3, 2, "Installer").setIcon(android.R.drawable.stat_sys_download_done)
        navigation.labelVisibilityMode = BottomNavigationView.LABEL_VISIBILITY_LABELED
        navigation.setOnItemSelectedListener { item ->
            val selected = item.itemId - 1
            pages.forEachIndexed { index, view -> view.visibility = if (index == selected) View.VISIBLE else View.GONE }
            true
        }
        scaffold.addView(navigation)
        setContentView(scaffold)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        ViewCompat.setOnApplyWindowInsetsListener(scaffold) { view, windowInsets ->
            val safeInsets = windowInsets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
            )
            view.setPadding(0, safeInsets.top, 0, safeInsets.bottom)
            windowInsets
        }
        ViewCompat.requestApplyInsets(scaffold)

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
        haButtons.addView(button("Load entities") { loadEntities() }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        haBox.addView(haButtons)
        haBox.addView(label("Plain HTTP works on your home network; the token is then not encrypted.", 12f))
        root.addCard(haBox)
        space(root, 12)

        val controlsBox = card("3 · Quick controls (up to ten)")
        controlsBox.addView(label("Names appear on the watch. Keep them short and non-sensitive.", 12f))
        for (i in 0..9) {
            space(controlsBox, 8)
            switches[i] = MaterialSwitch(this).apply { text = "Control " + (i + 1) }
            controlsBox.addView(switches[i])
            names[i] = fieldInto(controlsBox, "Watch label (max 15 bytes)")
            entities[i] = entityFieldInto(controlsBox, "Search by name or entity ID")
            sections[i] = fieldInto(controlsBox, "Section (optional, max 10 bytes)")
            val kindRow = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            kindRow.addView(label("Control type", 14f))
            val spinner = Spinner(this).apply {
                adapter = ArrayAdapter(
                    this@MainActivity,
                    android.R.layout.simple_spinner_dropdown_item,
                    arrayOf("Toggle switch", "Brightness slider")
                )
            }
            kinds[i] = spinner
            kindRow.addView(spinner)
            controlsBox.addView(kindRow)
            val iconRow = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            iconRow.addView(label("Watch icon", 14f))
            val iconSpinner = Spinner(this).apply { adapter = iconAdapter() }
            icons[i] = iconSpinner
            iconRow.addView(iconSpinner, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            controlsBox.addView(iconRow)
            favoriteChecks[i] = MaterialCheckBox(this).apply {
                text = "Favorite · page & widget"
                setOnCheckedChangeListener { _, checked ->
                    if (checked && favoriteChecks.count { it?.isChecked == true } > 5) {
                        isChecked = false
                        status.text = "Up to five favorites"
                    }
                }
            }
            controlsBox.addView(favoriteChecks[i])
        }
        space(controlsBox, 12)
        controlsBox.addView(label("Favorites open on the watch page and widget (up to five)", 14f, bold = true))
        controlsRoot.addCard(controlsBox)
        space(controlsRoot, 16)

        controlsRoot.addView(button("Save controls and connect") { connect() })
        space(controlsRoot, 4)
        controlsRoot.addView(button("Disconnect") { disconnect() })
        space(controlsRoot, 4)
        controlsRoot.addView(button("Keep running in background") { requestBatteryExemption() })
        space(root, 8)
        root.addView(button("Save and connect") { connect() })

        val installBox = card("Install HA Remote on your watch")
        installerSummary = label("No app package selected", 14f)
        installBox.addView(installerSummary)
        installBox.addView(button("Choose HA Remote .wp2app") { pickWatchPackage() })
        installerSlot = Spinner(this).apply {
            adapter = ArrayAdapter(this@MainActivity, android.R.layout.simple_spinner_dropdown_item,
                listOf("Auto · upgrade or first free slot") + (0 until Install.SLOTS).map { "Slot $it" })
        }
        installBox.addView(installerSlot)
        installBox.addView(button("Send app to watch") { installWatchPackage() })
        installBox.addView(label("HA Remote is bundled with this app. Choose another package only if you need to install a different app.", 12f))
        installRoot.addCard(installBox)
        loadBundledPackage()
        val backupBox = card("Settings backup")
        backupBox.addView(label("Export or restore your Home Assistant URL, controls and favorites. The token and watch address are excluded.", 13f))
        val backupActions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        backupActions.addView(button("Export") { exportBackup() }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        backupActions.addView(button("Restore") { importBackup() }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        backupBox.addView(backupActions)
        root.addCard(backupBox)
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
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(8), dp(20), 0)
        }
        val searchLayout = TextInputLayout(this).apply { hint = "Search devices" }
        val search = TextInputEditText(this).apply { setSingleLine(true) }
        searchLayout.addView(search)
        content.addView(searchLayout)
        val listView = ListView(this)
        content.addView(listView, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300)))
        val dialog = MaterialAlertDialogBuilder(this).setTitle("Choose your watch")
            .setView(content).setNegativeButton("Cancel", null).create()
        fun showMatches(query: String) {
            val matches = ordered.indices.filter { i ->
                entries[i].contains(query, true) || ordered[i].address.contains(query, true)
            }
            listView.adapter = ArrayAdapter(this, android.R.layout.simple_list_item_1, matches.map { entries[it] })
            val selectedMatch = matches.indexOfFirst { ordered[it].address == selectedAddress }
            if (selectedMatch >= 0) listView.setSelection(selectedMatch)
            listView.setOnItemClickListener { _, _, position, _ ->
                val index = matches[position]
                selectedAddress = ordered[index].address
                prefs.edit().putString("watch", selectedAddress).apply()
                updateWatchSummary()
                dialog.dismiss()
            }
        }
        search.addTextChangedListener(object : android.text.TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = showMatches(s?.toString().orEmpty())
            override fun afterTextChanged(s: android.text.Editable?) = Unit
        })
        dialog.setOnShowListener { showMatches("") }
        dialog.show()
    }

    // ---------- settings ----------

    private fun loadSettings() {
        haUrl.setText(prefs.getString("url", "http://homeassistant.local:8123"))
        haToken.setText(runCatching { decryptToken() }.getOrDefault(""))
        for (i in 0..9) {
            switches[i].isChecked = prefs.getBoolean("enabled$i", false)
            names[i].setText(prefs.getString("name$i", ""))
            entities[i].setText(prefs.getString("entity$i", ""))
            sections[i].setText(prefs.getString("sec$i", ""))
            val entity = prefs.getString("entity$i", "").orEmpty()
            val defaultKind = if (entity.startsWith("light.")) WatchAppProtocol.DIMMER else WatchAppProtocol.TOGGLE
            kinds[i]?.setSelection(prefs.getInt("kind$i", defaultKind).coerceIn(0, 1))
            icons[i]?.setSelection(prefs.getInt("icon$i", WatchAppProtocol.ICON_AUTO).coerceIn(WatchAppProtocol.ICON_AUTO, WatchAppProtocol.ICON_DEVICE))
            favoriteChecks[i]?.isChecked = false
        }
        val storedFavs = prefs.getStringSet("favs", null)
        if (storedFavs != null) {
            storedFavs.mapNotNull { it.toIntOrNull() }.forEach { slot ->
                if (slot in 0..9) favoriteChecks[slot]?.isChecked = true
            }
        } else {
            val legacy = prefs.getInt("fav", -1)
            if (legacy in 0..9) favoriteChecks[legacy]?.isChecked = true
        }
    }

    private fun showOnboardingIfNeeded() {
        if (prefs.getBoolean("onboarding_complete", false)) return
        val pages = arrayOf(
            "Your phone is in charge of the watch controls. When the app connects, the controls configured here replace any older list saved on the watch.",
            "Pair the CMF Watch Pro 2 once in Nothing X. This app uses that existing bond and does not start another pairing flow.",
            "Add your Home Assistant address and long-lived token on Home. Open Controls, enable a slot, type a name or entity ID to search, and choose its watch icon.",
            "Choose the watch, then save and connect. HA Remote is bundled in Installer, and Settings backup can export or restore your controls. The token is kept separately on this phone."
        )
        var page = 0
        val dialog = MaterialAlertDialogBuilder(this)
            .setTitle("Welcome to HA Remote")
            .setMessage(pages[page])
            .setNegativeButton("Skip", null)
            .setPositiveButton("Next", null)
            .create()
        dialog.setOnShowListener {
            val back = dialog.getButton(android.app.AlertDialog.BUTTON_NEGATIVE)
            val next = dialog.getButton(android.app.AlertDialog.BUTTON_POSITIVE)
            fun refresh() {
                dialog.setMessage(pages[page])
                back.text = if (page == 0) "Skip" else "Back"
                next.text = if (page == pages.lastIndex) "Get started" else "Next"
            }
            back.setOnClickListener {
                if (page == 0) {
                    prefs.edit().putBoolean("onboarding_complete", true).apply()
                    dialog.dismiss()
                } else {
                    page--
                    refresh()
                }
            }
            next.setOnClickListener {
                if (page == pages.lastIndex) {
                    prefs.edit().putBoolean("onboarding_complete", true).apply()
                    dialog.dismiss()
                } else {
                    page++
                    refresh()
                }
            }
            refresh()
        }
        dialog.show()
    }

    private fun createBackup(): JSONObject {
        val root = JSONObject().put("format", "ha-remote-settings").put("version", 2)
            .put("homeAssistantUrl", haUrl.text.toString().trim())
        val controls = JSONArray()
        for (i in 0..9) {
            controls.put(JSONObject()
                .put("enabled", switches[i].isChecked)
                .put("name", names[i].text.toString().trim())
                .put("entityId", entities[i].text.toString().trim())
                .put("section", sections[i].text.toString().trim())
                .put("kind", kinds[i]?.selectedItemPosition ?: 0)
                .put("icon", icons[i]?.selectedItemPosition ?: WatchAppProtocol.ICON_AUTO)
                .put("favorite", favoriteChecks[i]?.isChecked == true))
        }
        root.put("controls", controls)
        return root
    }

    private fun restoreBackup(root: JSONObject) {
        require(root.getString("format") == "ha-remote-settings") { "This is not an HA Remote settings backup" }
        val version = root.getInt("version")
        require(version in 1..2) { "This backup version is not supported" }
        val items = root.getJSONArray("controls")
        require(items.length() == 10) { "Backup must contain ten control slots" }
        val controls = (0..9).map { i ->
            val item = items.getJSONObject(i)
            val name = item.optString("name")
            val entity = item.optString("entityId")
            val section = item.optString("section")
            val kind = item.getInt("kind")
            val icon = if (version >= 2) item.optInt("icon", WatchAppProtocol.ICON_AUTO) else WatchAppProtocol.ICON_AUTO
            require(name.toByteArray(StandardCharsets.UTF_8).size <= 15) { "Control ${i + 1} name is too long" }
            require(section.toByteArray(StandardCharsets.UTF_8).size <= 10) { "Control ${i + 1} section is too long" }
            require(kind in WatchAppProtocol.TOGGLE..WatchAppProtocol.DIMMER) { "Control ${i + 1} has an unknown type" }
            require(icon in WatchAppProtocol.ICON_AUTO..WatchAppProtocol.ICON_DEVICE) { "Control ${i + 1} has an unknown icon" }
            val enabled = item.getBoolean("enabled")
            if (enabled) {
                require(entity.matches(Regex("[a-z_]+\\.[A-Za-z0-9_]+"))) { "Control ${i + 1} has an invalid entity ID" }
                require(kind != WatchAppProtocol.DIMMER || entity.startsWith("light.")) { "Control ${i + 1} brightness slider must use a light entity" }
            }
            BackupControl(enabled, name, entity, section, kind, icon, item.optBoolean("favorite"))
        }
        require(controls.count { it.favorite } <= 5) { "Backup has more than five favorites" }
        val editor = prefs.edit().putString("url", root.optString("homeAssistantUrl", ""))
        controls.forEachIndexed { i, c ->
            editor.putBoolean("enabled$i", c.enabled).putString("name$i", c.name)
                .putString("entity$i", c.entity).putString("sec$i", c.section).putInt("kind$i", c.kind).putInt("icon$i", c.icon)
        }
        val favorites = controls.withIndex().filter { it.value.favorite }.map { it.index.toString() }.toSet()
        editor.putStringSet("favs", favorites).putInt("fav", favorites.firstOrNull()?.toIntOrNull() ?: -1).apply()
    }

    private data class BackupControl(
        val enabled: Boolean, val name: String, val entity: String, val section: String, val kind: Int,
        val icon: Int, val favorite: Boolean
    )

    private fun exportBackup() {
        startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/json"
            putExtra(Intent.EXTRA_TITLE, "ha-remote-settings.json")
        }, EXPORT_BACKUP)
    }

    private fun importBackup() {
        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/json"
        }, IMPORT_BACKUP)
    }

    private fun saveSettings() {
        val editor = prefs.edit().putString("url", haUrl.text.toString().trim())
        for (i in 0..9) {
            editor.putBoolean("enabled$i", switches[i].isChecked)
                .putString("name$i", names[i].text.toString().trim())
                .putString("entity$i", entities[i].text.toString().trim())
                .putString("sec$i", sections[i].text.toString().trim())
                .putInt("kind$i", kinds[i]?.selectedItemPosition ?: 0)
                .putInt("icon$i", icons[i]?.selectedItemPosition ?: WatchAppProtocol.ICON_AUTO)
        }
        val favs = favoriteChecks.withIndex().filter { it.value?.isChecked == true }.map { it.index.toString() }.toSet()
        editor.putStringSet("favs", favs)
        editor.putInt("fav", favs.firstOrNull()?.toIntOrNull() ?: -1)
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
                status.text = "Home Assistant connected · $count entities found"
            } catch (e: Exception) {
                status.text = "Home Assistant connection failed: " + (e.message ?: "check address and token")
            }
        }
    }

    private fun loadEntities() {
        if (entitiesLoading) return
        val url = haUrl.text.toString().trim()
        val token = haToken.text.toString().trim()
        if (!isHttpUrl(url) || token.isEmpty()) {
            status.text = "Enter the http:// or https:// address and token first"
            return
        }
        scope.launch {
            entitiesLoading = true
            try {
                status.text = "Loading searchable Home Assistant entities…"
                val list = withContext(Dispatchers.IO) { HaApi(url, token).entities() }
                if (list.isEmpty()) {
                    status.text = "No entities found"
                    return@launch
                }
                entityChoices = list
                entities.forEach { input ->
                    input.setAdapter(entityAdapter())
                    if (input.hasFocus()) input.post { input.showDropDown() }
                }
                status.text = "${list.size} entities ready. Type a name or entity ID in Controls to search."
            } catch (e: Exception) {
                status.text = "Home Assistant error: " + (e.message ?: "request failed")
            } finally {
                entitiesLoading = false
            }
        }
    }

    private fun loadBundledPackage() {
        runCatching {
            val bytes = assets.open("ha-remote.wp2app").use { it.readBytes() }
            val header = Install.parsePackage(bytes)
            require(header.kind == Install.KIND_APP && header.name == "ha-remote") { "Bundled package is not HA Remote" }
            packageBytes = bytes
            packageHeader = header
            installerSummary.text = "Bundled HA Remote · v${Install.versionText(header.version)} · ${bytes.size / 1024} KB"
        }.onFailure {
            installerSummary.text = "Bundled HA Remote package could not be loaded"
            status.text = "Installer package error: ${it.message ?: it.javaClass.simpleName}"
        }
    }

    private fun pickWatchPackage() {
        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
        }, PICK_PACKAGE)
    }

    private fun installWatchPackage() {
        val address = selectedAddress
        val bytes = packageBytes
        val header = packageHeader
        if (address == null) { status.text = "Choose your watch first"; return }
        if (bytes == null || header == null) { status.text = "Choose a .wp2app package first"; return }
        if (header.kind != Install.KIND_APP) { status.text = "Only watch app packages can be installed"; return }
        val device = runCatching {
            getSystemService(BluetoothManager::class.java).adapter.getRemoteDevice(address)
        }.getOrElse { status.text = "Could not resolve the saved watch"; return }
        val requestedSlot = installerSlot.selectedItemPosition
        scope.launch {
            val link = WatchLink(this@MainActivity, device)
            try {
                status.text = "Connecting to watch…"
                link.open()
                val loaderAbi = link.stubInfo?.loaderAbiMinor
                if (loaderAbi != null && header.abiMinor > loaderAbi) {
                    error("Watch App Loader is too old for this package (needs ABI ${header.abiMinor}, has $loaderAbi)")
                }
                val used = HashMap<Int, String>()
                for (slot in 0 until Install.SLOTS) {
                    val reply = link.request(Install.INFO, byteArrayOf(slot.toByte())) ?: continue
                    if (reply.size > 1 && reply[0].toInt() == 0) {
                        runCatching { Install.parseHeader(reply.copyOfRange(1, minOf(reply.size, 1 + Install.HEADER_SIZE))).name }
                            .getOrNull()?.let { used[slot] = it }
                    }
                }
                val slot = if (requestedSlot == 0) Install.chooseSlot(header.name, used)
                    ?: error("No free watch app slot; remove an app and retry") else requestedSlot - 1
                val begin = link.request(Install.BEGIN, byteArrayOf(slot.toByte()) +
                    Install.u32le(bytes.size.toLong()) + Install.u32le(Install.crc32(bytes)))
                    ?: error("Watch did not answer the upload start")
                if (begin.isEmpty() || begin[0].toInt() != 0) {
                    error("Watch refused upload: ${begin.firstOrNull()?.toInt()?.and(255)?.let(Install::statusName) ?: "no status"}")
                }
                var offset = 0
                while (offset < bytes.size) {
                    val count = minOf(Install.MAX_DATA, bytes.size - offset)
                    val reply = link.request(Install.DATA, Install.u32le(offset.toLong()) + bytes.copyOfRange(offset, offset + count))
                        ?: error("Upload stopped at $offset bytes; keep the watch awake and retry")
                    if (reply.isEmpty() || reply[0].toInt() != 0) {
                        error("Watch refused upload data: ${reply.firstOrNull()?.toInt()?.and(255)?.let(Install::statusName) ?: "no status"}")
                    }
                    if (reply.size < 5 || Install.le32(reply, 1) != (offset + count).toLong()) error("Watch upload acknowledgement did not match")
                    offset += count
                    status.text = "Sending ${header.name} · ${offset * 100 / bytes.size}%"
                }
                val end = link.request(Install.END, byteArrayOf(0)) ?: error("Watch did not confirm the upload")
                if (end.isEmpty() || end[0].toInt() != 0) {
                    error("Watch refused the completed package: ${end.firstOrNull()?.toInt()?.and(255)?.let(Install::statusName) ?: "no status"}")
                }
                val verify = link.request(Install.INFO, byteArrayOf(slot.toByte()))
                val installed = verify?.takeIf { it.size > 1 && it[0].toInt() == 0 }
                    ?.let { Install.parseHeader(it.copyOfRange(1, minOf(it.size, 1 + Install.HEADER_SIZE))).name }
                status.text = if (installed == header.name) "Installed ${header.name} · slot $slot" else "Upload complete; watch slot verification failed"
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                status.text = "Watch install failed: ${e.message ?: e.javaClass.simpleName}"
            } finally {
                link.close()
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
        for (i in 0..9) {
            if (!switches[i].isChecked) continue
            if (names[i].text.toString().trim().toByteArray(StandardCharsets.UTF_8).size > 15) {
                status.text = "Control " + (i + 1) + ": label must be 15 UTF-8 bytes or fewer"
                return
            }
            val entity = entities[i].text.toString().trim()
            if (!entity.matches(Regex("[a-z_]+\\.[A-Za-z0-9_]+"))) {
                status.text = "Control " + (i + 1) + ": enter a Home Assistant entity id (like light.desk)"
                return
            }
            if (kinds[i]?.selectedItemPosition == WatchAppProtocol.DIMMER && !entity.startsWith("light.")) {
                status.text = "Control " + (i + 1) + ": brightness sliders need a light.* entity"
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

    companion object {
        private const val PICK_PACKAGE = 12
        private const val EXPORT_BACKUP = 13
        private const val IMPORT_BACKUP = 14
    }
}


# Home Assistant Remote for CMF Watch Pro 2

Android companion for the `apps/ha-remote` watch app. Configure up to ten Home Assistant entities of any domain,
each as a toggle or a brightness slider, with the short name, section and favorite shown on the watch. The phone
calls Home Assistant; the watch sends toggle and brightness requests and shows confirmed state.

## Dual connection behavior

Pair the watch once in Nothing X. In this app, select the already bonded watch and connect. The app opens a second
Android GATT client (`connectGatt`) and uses the encrypted app channel key K without starting another pairing or
bind handshake. This exact pattern is implemented in the public [Vela Android GATT session](https://github.com/arunk140/esp32-vela/blob/main/app/src/main/java/app/vela/device/watch/WatchSession.kt): it calls `connectGatt` on the bonded watch, reads K with `AT GETSECRET`, and uses the app channel. The author reports Vela and Nothing X operating together on one phone/watch pair in the [project thread](https://www.reddit.com/r/CMFTech/comments/1xynmyv/comment/19ss2wcvl0sro/?context=1&screen_view_count=8). That demonstrates the setup can work; Android Bluetooth behavior can still vary, so verify the particular phone/watch pair.

Our `WatchLink` follows the SDK protocol independently: LE GATT to the existing bonded identity, the stock
data-notify phone-ready subscription, shell read of K, encrypted channel requests, and no new bond or bind. It
also checks the running stub's widget capability and reconnects after a dropped link. Vela's source is a behavioral
reference; this project does not copy its implementation.

## Build

Install Android SDK platform 35 and Java 17+, then from this directory run `gradlew.bat testDebugUnitTest assembleDebug`.
Install `app/build/outputs/apk/debug/app-debug.apk`. The app requires Android's
`BLUETOOTH_CONNECT` runtime permission and network access. Connecting starts a connected-device foreground
service with an ongoing notification; it keeps the GATT session while the screen is off, reconnects with bounded
backoff after BLE interruptions, and keeps the watch link while Home Assistant is unavailable. It shows cached state
until HA recovers and fresh state reads succeed. Force-stopping the app or denying required system permissions stops
that service; reopen the app to resume it.

On connect, the companion also reads the running stub version with the SDK's `INFO` request under channel key K. Its
status reports whether the firmware stub advertises the level needed for app widgets. The loader is separate and
must also be a widget-capable release (2026-11-15 or newer); the OTA file's embedded widget entry alone cannot prove
which firmware or loader is currently installed on the watch.

The app takes an HTTP or HTTPS Home Assistant URL, a long-lived access token, and up to ten entities of any domain
(`light.*`, `switch.*`, `fan.*`, ...); brightness sliders require `light.*`. It uses Home Assistant's REST API.
The token is encrypted with a key held in Android Keystore. It does not scan or bind; select a device already bonded
in Nothing X. Never send Home Assistant credentials through the watch channel.

## Compatibility

Use FMC patched firmware and App Loader with channel key K and the app message channel (2026-11-13 or newer), then
install the `ha-remote` watch app. The optional app widget also needs firmware stub v4+ and a widget-capable loader
(2026-11-15 or newer). App button names are limited to 15 UTF-8 bytes. Add the watch card from Apps by long-pressing
the app and selecting **Add widget**.

Android Bluetooth stacks and Nothing X behavior differ by device. The public Vela example reports successful
simultaneous use, but still test the specific handset/watch pair: verify Nothing X stays connected and the channel
works after screen off, BLE reconnect, watch reboot, and Android process restart. The source tree does not contain
test credentials, pairing secrets, or personal data.

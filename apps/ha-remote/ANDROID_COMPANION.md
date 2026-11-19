# Home Assistant Remote Android companion

This is the implementation contract for the Android companion paired with `apps/ha-remote`. The phone is the only
Home Assistant client: the watch sends an intent, and the phone reports the entity state after Home Assistant
confirms it. The watch never stores a Home Assistant URL, token, entity ID, room name, or credential.
The configured short button labels are sent over BLE to be shown on the watch; keep them generic and non-sensitive.

## Connection model

1. The user pairs the watch once in Nothing X. The companion does not run a second pairing flow and must not perform
   the App Loader's `0x8047` / `0x8049` bind handshake.
2. Resolve the already bonded WP2 and use `connectGatt` to its existing GATT identity while Nothing X is active.
   Android coexistence with Nothing X is a required hardware validation: SDK documentation calls this expected but
   untested. If Android cannot maintain the second GATT client on a phone model, report that combination as
   unsupported instead of rebinding or disconnecting Nothing X.
3. Subscribe to notify UUIDs `0000FFF1-0000-1000-8000-00805F9B34FB` and
   `77D4FF02-2FE2-2334-0D35-9CCD078F529C`; write to `FFF2` and shell `77D4FF01-2FE2-2334-0D35-9CCD078F529C`.
   Accumulate shell notifications until the complete `GETSECRET:<32 hex>,OK` response arrives.
4. Treat K as a password. This companion keeps K in memory only and clears it when the GATT session closes; never
   log, export, sync, or put it in a project file. The SDK threat model notes that the watch pairing secret is
   readable over BLE by a nearby client, so this channel must not carry secrets or personal data.
5. Send one encrypted channel `PING` (`0x90A7`) under K to register. Encode/decode channel frames as specified in
   `.sdk/wp2-apps/docs/abi-1.4.md`. Do not send Nothing X authentication or pairing frames.
6. Decode `0xB0A6` EVENT notifications, verify sequence and running app slot, and pass only `H1` app messages to
   the protocol handler. Respond to watch HELLO with a full button configuration and current state snapshots.
7. On process restart, reconnect and try K directly. On no response, show a compatibility diagnostic for stock or
   old patched firmware; never silently start a fresh bind.

## Watch protocol (`H1`)

All integers are unsigned bytes. App messages have a 128 byte maximum. UTF-8 names are capped at 15 bytes. There
are at most five button records, indexed 0–4. Entity IDs remain phone-local.

| Direction | Frame | Meaning |
|---|---|---|
| Watch → phone | `H1?` | App started or resumed; send config and all states |
| Phone → watch | `H1C count (enabled name_len name)*` | Replace the configured buttons in order; count 0–5 |
| Watch → phone | `H1T seq index requested_on` | Request toggle to requested state; seq is diagnostic/deduplication only |
| Phone → watch | `H1A seq` | Command accepted by companion for processing; pending remains until `H1S` |
| Phone → watch | `H1P` | Companion heartbeat; refreshes the watch's phone-connected indicator |
| Watch → phone | `H1W result` | Runtime result from `Api::widget`; 0 means the current loader accepts widget updates |
| Phone → watch | `H1S index actual_on result` | Authoritative state; result 0 means confirmed, nonzero is generic failure |
| Phone → watch | `H1E seq error_code` | Generic failure; never include HA response text or entity data |

`H1C` body encoding is `count:u8`, then `count` records of `enabled:u8, name_len:u8, name[name_len]`. `H1S` is
exactly `index:u8, actual_on:u8, result:u8`; result 0 confirms the command and clears pending/error state, nonzero
reports failure while still updating the last known actual state. `H1E` is `seq:u8, error_code:u8`. On `H1T`,
send `H1A` promptly, then await HA confirmation and send `H1S`. The phone deduplicates repeated toggle sequences
and permits only one outstanding watch command at a time. A failed or timed-out Home Assistant call sends `H1S`
with nonzero result and a fresh state; it must not optimistically leave the watch showing the requested value.
`H1P` is sent every 10 seconds while the watch app is active. It refreshes liveness without changing a control or
rewriting watch storage.

Watch config persistence uses a versioned `D1` blob containing exactly five fixed records of
`enabled:u8, name_len:u8, actual_on:u8, name[15]`; this is a startup display fallback only. Android remains
authoritative and resends config and fresh states after every HELLO.

## Android app behavior

- Home Assistant setup: user enters instance URL and long-lived access token in app UI; token stays in Keystore-
  protected app storage and is used only by the phone's HTTPS REST client.
- Five slots: enable/disable, short display name, and a Home Assistant light or light group. The entity picker
  loads `light.*` entities and legacy `group.*` entities containing lights. Individual lights use
  `light.turn_on` / `light.turn_off`; legacy groups use `homeassistant.turn_on` / `homeassistant.turn_off`.
- Startup: load stored slot settings, connect HA, resolve current states, connect to the bonded watch, open K channel,
  then publish config and state. Send a full config and state snapshots on `H1?` and after any watch reconnect. Keep
  the app channel registered with `PING` on entry; the 2026-11-13+ loader delivers app messages unsolicited. While the
  app runs, send `H1P` at least every 10 seconds so the watch does not mark a healthy companion as disconnected.
- Toggle handling: validate slot and requested state; serialize watch commands; call `light.turn_on` or
  `light.turn_off`; await HA state confirmation; publish `H1S`. Duplicate seq is idempotent.
- HA state changes from another controller are detected by five-second polling and sent to the watch as `H1S`, so
  its indicators stay current.
- The Android connected-device foreground service owns the GATT session independently of the setup screen. It keeps
  an ongoing notification and reconnects with exponential backoff up to 60 seconds after BLE errors. Home Assistant
  outages keep the watch channel alive and retain the last confirmed states until polling recovers. Android
  force-stop and denied Bluetooth permission require the user to reopen the app. Request deduplication is held in
  memory for 15 seconds; Home Assistant on/off commands are idempotent, so an app process restart cannot turn a
  retried request into the opposite state.
- UI exposes BLE/channel/HA connection status and a **Test connection** action. Never expose K or raw protocol
  payloads in logs or diagnostics.

## Compatibility and release gates

- Firmware: K channel requires patched stub and loader from 2026-11-13 or later. The app widget requires stub v4+
  and a loader release from 2026-11-15 or later. Latest SDK upstream is `b81cb772` (2026-11-17); it adds a
  background phone wake-probe example and does not change the loader, K channel, or widget support used here.
- Android release gate: exercise Nothing X + companion simultaneously on each supported Android vendor/version;
  verify no new pairing popup, both BLE clients retain notifications, K channel works after screen off, service
  restart and watch reboot, and Home Assistant state changes propagate both ways.
- Watch release gate: host protocol tests, target build, FMC store check, then emulator/hardware validation. Emulator
  does not model LVGL widgets, cards, pages, or BLE coexistence.

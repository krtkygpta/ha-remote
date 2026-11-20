# Home Assistant remote Android companion

This is the implementation contract for the Android companion paired with `apps/ha-remote`. The phone is the only
Home Assistant client: the watch sends an intent, and the phone reports the entity state after Home Assistant
confirms it. The watch never stores a Home Assistant URL, token, entity ID, room name, or credential.
The configured short button labels are sent over BLE to be shown on the watch; keep them generic and non-sensitive.

## Connection model

1. The user pairs the watch once in Nothing X. The companion does not run a second pairing flow and must not perform
   the App Loader's `0x8047` / `0x8049` bind handshake.
2. Resolve the already bonded WP2 and use `connectGatt` to its existing GATT identity while Nothing X is active.
   Android coexistence with Nothing X is a required hardware validation. If Android cannot maintain the second GATT
   client on a phone model, report that combination as unsupported instead of rebinding or disconnecting Nothing X.
3. Subscribe to notify UUIDs `0000FFF1-0000-1000-8000-00805F9B34FB` and
   `77D4FF02-2FE2-2334-0D35-9CCD078F529C`; write to `FFF2` and shell `77D4FF01-2FE2-2334-0D35-9CCD078F529C`.
   Accumulate shell notifications until the complete `GETSECRET:<32 hex>,OK` response arrives.
4. Treat K as a password. This companion keeps K in memory only and clears it when the GATT session closes; never
   log, export, sync, or put it in a project file. Nothing personal goes over this channel.
5. Send one encrypted channel `PING` (`0x90A7`) under K to register. Encode/decode channel frames as specified in
   `.sdk/wp2-apps/docs/abi-1.4.md`. Do not send Nothing X authentication or pairing frames.
6. Decode `0xB0A6` EVENT notifications, verify sequence and running app slot, and pass only `H1` app messages to
   the protocol handler. Respond to watch HELLO with a full button configuration and current state snapshots.
7. On process restart, reconnect and try K directly. On no response, show a compatibility diagnostic for stock or
   old patched firmware; never silently start a fresh bind.

## Watch protocol v3 (`H1`)

All integers are unsigned bytes. App messages have a 128-byte maximum. UTF-8 names are capped at 15 bytes and
section titles at 10. There are **ten slots** (0-9), each with a kind: `0` toggle, `1` dimmer (brightness slider).
Up to **five favorites** are slot indexes sent as five bytes with `0xFF` for an empty slot. Entity IDs remain
phone-local.

| Direction | Frame | Meaning |
|---|---|---|
| Watch → phone | `H1?` | App started or resumed; send config and all states |
| Phone → watch | `H1V n (enabled kind len name)*` | Slots 5-9; `n <= 5` |
| Phone → watch | `H1J n (len section)*` | Sections for slots 0-4 |
| Phone → watch | `H1K n (len section)*` | Sections for slots 5-9 |
| Phone → watch | `H1U n (enabled kind len name)* fav[5]` | Slots 0-4 + favorites; **the commit frame the watch persists** |
| Watch → phone | `H1T seq slot on` | Toggle request (`slot 0..9`) |
| Watch → phone | `H1B seq slot value` | Brightness request, `value 0..=100`, dimmer slots only |
| Phone → watch | `H1A seq` | Command accepted; pending remains until `H1S` |
| Phone → watch | `H1P` | Companion heartbeat (10 s) |
| Phone → watch | `H1S slot on value result available` | Authoritative state; `value` is brightness percent, result `0` confirmed, nonzero generic failure, `available 0` means HA has no data for the entity (device lost power): the watch greys the row and shows **Inactive** |
| Phone → watch | `H1E seq error_code` | Generic failure; never include HA response text or entity data |
| Watch → phone | `H1W result` | Runtime result from `Api::widget` |

The phone sends `V, J, K, U` in that order on every HELLO; only `U` is persisted by the watch, so a half-delivered
configuration never replaces the stored one. The legacy `H1C` (five slots, optional single favorite) and `H1G`
frames from older companions are still accepted. For `H1B`, send `H1A` promptly, set the brightness through Home
Assistant, wait for the reported value to settle, then send `H1S`. The phone serializes commands: one outstanding
watch command at a time, duplicate sequences are idempotent, and a failed or timed-out Home Assistant call sends
`H1S` with a nonzero result and the cached state so the watch never keeps an optimistic value.

Watch config persistence uses a versioned `D3` blob: count, the five favorites, then ten fixed records of
`enabled, kind, on, value, len, name[15], sec_len, section[10]`. The old `D2` five-slot blob is migrated on load
(kind toggle, value mirrors on/off). Android remains authoritative and resends config and fresh states after every
HELLO.

State frames may be 6 bytes (legacy), 7 bytes (no availability byte) or 8 bytes (current); a missing availability
byte means available. An unavailable push is a device state, not an app error: the watch clears any pending command,
keeps the last on/value, and shows the row as Inactive until HA reports the entity again.

## Android app behavior

- Home Assistant setup: user enters instance URL and long-lived access token in app UI; token stays in Keystore-
  protected app storage and is used only by the phone's HTTPS REST client.
- **All entities** in Home Assistant are selectable (the picker loads `/api/states`); for each of the ten slots the
  user chooses the kind: toggle switch or brightness slider (sliders require `light.*`).
- Watch labels are short names (15 UTF-8 bytes), section titles are 10 bytes, and up to five slots can be marked
  favorite: favorites are what the watch page and widget show.
- Startup: load stored slot settings, connect HA, resolve current states, connect to the bonded watch, open K
  channel, then publish `V, J, K, U` and state snapshots. Send a full config and states on `H1?` and after every
  reconnect. Keep the channel registered with `P` at least every 10 seconds.
- Toggle handling: validate slot and requested state; serialize watch commands; call `homeassistant.turn_on/off`,
  await the state, publish `H1S` with the read-back value. Brightness uses `light.turn_on` with `brightness_pct`
  (0 turns off) and confirms against the reported brightness. Duplicate seq is idempotent.
- HA state changes from another controller are detected by five-second polling and sent to the watch as `H1S`, so
  the rows and widget stay current.
- The Android connected-device foreground service owns the GATT session independently of the setup screen; it keeps
  an ongoing notification and reconnects with exponential backoff up to 60 seconds after BLE errors. Home Assistant
  outages keep the watch channel alive and retain the last confirmed states until polling recovers.
- UI exposes BLE/channel/HA connection status and a **Test connection** action. Never expose K or raw protocol
  payloads in logs or diagnostics.

## Compatibility and release gates

- Firmware: K channel requires patched stub and loader from 2026-11-13 or later. The app widget requires stub v4+
  and a loader release from 2026-11-15 or later.
- Android release gate: exercise Nothing X + companion simultaneously on each supported Android vendor/version;
  verify no new pairing popup, both BLE clients retain notifications, K channel works after screen off, service
  restart and watch reboot, and Home Assistant state changes and brightness propagate both ways.
- Watch release gate: host protocol tests, target build, FMC store check, then emulator/hardware validation. The
  emulator does not model LVGL widgets, cards, pages, or BLE coexistence.

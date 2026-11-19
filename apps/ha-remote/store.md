Control up to five Home Assistant lights or groups. Configure each button in the Android companion app, give it a name, and tap its row on the watch to request a toggle. The phone sends Home Assistant commands and confirms the resulting state back to the watch.

Next to the watch face it offers two optional surfaces, both added by long-pressing the app in Apps:

- **Add page**: the same screen as the app, living next to the face — same rows, icons and
  arc scrolling, and touching a row toggles it right there.
- **Add widget**: a text card showing the favorite's name and state. A card is drawn by
  the stock firmware and a tap starts the app; the toggling happens on the page.

## Requirements

- FMC patched firmware/App Loader with channel key K support (2026-11-13 or later). The optional page and widget need
  firmware stub v4+ and a loader from 2026-11-15 or later.
- The HA Watch Remote Android companion app on the same phone as Nothing X. The companion connects to the watch's existing bonded BLE identity without rebinding, obtains the channel key K through the shell service, and sends commands through the encrypted app channel. Android's second-GATT-client coexistence with Nothing X must be validated on the target phone/watch combination.
  Companion source and builds: <https://github.com/krtkygpta/ha-remote>.
- A Home Assistant instance configured in the Android companion app.

The BLE app channel is readable by any nearby BLE client that can obtain the pairing secret. Do not send credentials or personal data over it. The app protocol carries button indexes, states, and generic error codes only.

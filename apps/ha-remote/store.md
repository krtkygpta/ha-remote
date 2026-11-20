Control up to ten Home Assistant entities from the wrist: lights, switches, fans, plugs, anything that turns on and off. The Android companion lists every entity in your Home Assistant; you pick each control's kind, short watch name and section, and the watch shows them grouped and centered.

- **Toggle rows** show a stock switch for on/off controls.
- **Brightness sliders** for lights work like the watch's volume slider: drag on the row to dim the bulb, and the phone confirms the real state back.
- **Up to five favorites** live on the page next to the watch face and the widget card; everything else stays in the Apps row.

The phone is the only Home Assistant client: it sends commands over the encrypted app channel and reports the confirmed state, including changes made from other apps.

## Requirements

- FMC patched firmware/App Loader with channel key K support (2026-11-13 or later). The optional page and widget need firmware stub v4+ and a loader from 2026-11-15 or later.
- The Home Assistant companion app v0.3.0 or later on the same phone as Nothing X: <https://github.com/krtkygpta/ha-remote/releases>. It connects to the watch's existing bonded BLE identity without rebinding and sends commands through the encrypted app channel.
- A Home Assistant instance configured in the companion app.

The BLE app channel is readable by any nearby BLE client that can obtain the pairing secret. Do not send credentials or personal data over it. The app protocol carries slot indexes, short labels, states and generic error codes only.

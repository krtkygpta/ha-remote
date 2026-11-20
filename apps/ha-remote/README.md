# Home Assistant (ha-remote)

Up to ten Home Assistant controls on the watch: toggle switches for on/off entities and brightness
sliders for lights, with names and sections set from the phone. The Apps row lists every control;
the page next to the face and the widget card show up to five favorites.

The Android companion (setup UI, entity picker, Home Assistant connection, the watch's BLE link,
and the APK) lives in its own repository: <https://github.com/krtkygpta/ha-remote>. That repository
also carries this watch app under the same `apps/ha-remote/` layout, plus PC install instructions.

`store.toml` and `store.md` are the FMC store listing; `ANDROID_COMPANION.md` documents the
watch-phone protocol contract.

## Build and install

From the repository root (the same commands work in both repositories):

```bash
make test APP=ha-remote     # host protocol and screen-batch tests
make build APP=ha-remote    # target/ha-remote.wp2app
make run APP=ha-remote ADDRESS=AA:BB:CC:DD:EE:FF   # install on the watch over BLE
```

The watch needs the FMC patched firmware and App Loader with channel key K (2026-11-13 or newer); the page and
widget surfaces need a loader from 2026-11-15 or newer.

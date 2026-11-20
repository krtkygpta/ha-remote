# Home Assistant remote for the CMF Watch Pro 2

Control up to ten Home Assistant entities from the watch: lights with brightness sliders, plus switches, fans and
anything else that turns on and off. Names, sections, control kinds and up to five favorites are set in the Android
companion; the watch sends toggle and brightness requests, and the phone runs the commands and confirms the real
state back.

- `apps/ha-remote/` — the watch app (the same files as the [`HA-remote` branch of krtkygpta/apps](https://github.com/krtkygpta/apps/tree/HA-remote),
  which is where it can be merged into the FMC store). `store.toml`/`store.md` are its store listing,
  `ANDROID_COMPANION.md` the watch-phone protocol contract.
- `android/` — the Android companion app (Material 3, foreground service keeps the BLE link and the Home
  Assistant connection alive, background state sync every 5 s).

## Installing the watch app from a PC

A watch with the [FMC patched firmware and App Loader](https://fmc.freethinkel.dev/firmware) is required: the
channel key K needs a stub/loader from 2026-11-13 or newer, and the page/widget surfaces need a loader from
2026-11-15 or newer.

Toolchain: [Rust](https://rustup.rs) with `rustup target add thumbv7em-none-eabi` and
`rustup component add llvm-tools`, [uv](https://docs.astral.sh/uv/), [resvg](https://github.com/linebender/resvg),
`make`, and a C linker.

```bash
git clone https://github.com/krtkygpta/ha-remote
cd ha-remote
make sdk                       # fetch cmf-watch-sdk at the pinned commit into .sdk/wp2-apps
make test APP=ha-remote        # host protocol tests
make build APP=ha-remote       # target/ha-remote.wp2app

uv run --locked wp2apps scan                                        # find the watch's address
uv run --locked wp2apps install target/ha-remote.wp2app --address AA:BB:CC:DD:EE:FF
uv run --locked wp2apps list --address AA:BB:CC:DD:EE:FF            # verify the install
```

`make run APP=ha-remote ADDRESS=AA:BB:CC:DD:EE:FF` does the build and install in one step.

## Android companion

```bash
cd android
gradlew.bat assembleDebug      # needs JDK 17 and the Android SDK (platform 35)
```

Install `android/app/build/outputs/apk/debug/app-debug.apk`. Pair the watch once in Nothing X; the companion
connects to the already bonded watch without a second pairing, reads the channel key over the shell service, and
keeps Nothing X connected. Then enter the Home Assistant URL and a long-lived access token, pick entities for up
to configure up to ten controls and up to five favorites, and connect. A foreground service keeps both links alive
while the screen is off.

The app channel is encrypted but readable by anyone who can obtain the watch's pairing secret; keep control names
generic and never send credentials through it.

## License

MIT, like the apps repository this app is destined for.

#![cfg_attr(
    any(target_os = "none", wp2_sim),
    no_main,
    cfg_attr(target_os = "none", no_std)
)]

#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod proto;
mod screen;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::proto::{self, Effect, MAX_BUTTONS, Model};
    use crate::screen::{Mode, Screen, View};
    use cmf_wp2_app::lv::{Batch, Lv};
    use cmf_wp2_app::{Api, BUTTON, CROWN, LV, MSG_MAX, PAUSE, RESUME, stock};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();
    cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET | cmf_wp2_app::PAGE);

    /// LVGL event codes: row tap, row long press, slider drag.
    const CLICKED: i16 = 7;
    const LONG_PRESSED: i16 = 5;
    const VALUE_CHANGED: i16 = 28;
    /// A long press opens the slider page; a CLICKED landing right after it belongs to the
    /// same press and must not flip the brightness.
    const LONG_GRACE_MS: u32 = 900;
    /// Percent per crown detent on the slider page.
    const CROWN_STEP: i16 = 5;

    static mut MODEL: Model = Model::new();
    static mut STORE: [u8; 384] = [0; 384];

    /// The widget card: the first favorite's name and state (or the first enabled control).
    fn update_widget(api: &Api, m: &Model) -> i32 {
        let chosen = m.primary().and_then(|i| m.buttons.get(i)).copied();
        let mut val = [0u8; 40];
        let value = if !m.connected {
            "Phone offline"
        } else {
            match chosen {
                Some(b) if b.enabled => {
                    let label = b.label();
                    let n = label.len().min(proto::NAME_MAX);
                    val[..n].copy_from_slice(&label.as_bytes()[..n]);
                    let mut k = n;
                    val[k] = b'\n';
                    k += 1;
                    if !b.available {
                        val[k..k + 8].copy_from_slice(b"INACTIVE");
                        k += 8;
                    } else {
                        let state: &[u8] = if b.on { b"ON" } else { b"OFF" };
                        val[k..k + 2].copy_from_slice(state);
                        k += 2;
                        if b.is_dimmer() {
                            val[k] = b' ';
                            k += 1;
                            let v = b.value.min(100);
                            if v >= 100 {
                                val[k..k + 3].copy_from_slice(b"100");
                                k += 3;
                            } else if v >= 10 {
                                val[k] = b'0' + v / 10;
                                val[k + 1] = b'0' + v % 10;
                                k += 2;
                            } else {
                                val[k] = b'0' + v;
                                k += 1;
                            }
                            val[k] = b'%';
                            k += 1;
                        }
                    }
                    core::str::from_utf8(&val[..k]).unwrap_or("")
                }
                _ => "Set up in the phone app",
            }
        };
        api.widget("Home Assistant", value)
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 };
        // The crown press / right swipe is ours: while the slider page is open it goes
        // back to the list; only a plain list press exits.
        api.own_back(true);
        let Some(mut batch) = Batch::new() else { return 6 };
        if stock::font(&mut batch)
            .and_then(|_| batch.submit(&lv))
            .is_err()
        {
            return 1;
        }
        // SAFETY: a single app thread owns these static buffers.
        let m = unsafe { &mut *core::ptr::addr_of_mut!(MODEL) };
        let stored = unsafe { &mut *core::ptr::addr_of_mut!(STORE) };
        let n = api.load(stored);
        if n > 0 {
            m.load(&stored[..n as usize]);
        }
        let mode = if api.on_page() { Mode::Page } else { Mode::Menu };
        list_main(api, m, stored, lv, batch, mode)
    }

    struct Link {
        last_rx: u32,
        last_connection: bool,
        widget_last: u32,
        widget_reported: bool,
        widget_dirty: bool,
        last_state: [u8; MAX_BUTTONS],
    }

    impl Link {
        fn new(api: &Api, m: &Model) -> Self {
            Self {
                last_rx: api.now(),
                last_connection: m.connected,
                widget_last: 0,
                widget_reported: false,
                widget_dirty: true,
                last_state: [0xFF; MAX_BUTTONS],
            }
        }
    }

    fn sync(api: &Api, m: &mut Model, stored: &mut [u8; 384], link: &mut Link) -> bool {
        let mut dirty = false;
        if m.connected != link.last_connection {
            link.last_connection = m.connected;
            dirty = true;
            link.widget_dirty = true;
        }
        for i in 0..MAX_BUTTONS {
            let b = match m.buttons.get(i) { Some(b) => b, None => continue };
            let state = ((b.enabled as u8) << 2) | ((b.on as u8) << 1) | (b.value >= 100) as u8;
            if link.last_state[i] != state {
                link.last_state[i] = state;
                link.widget_dirty = true;
                dirty = true;
            }
        }
        let mut rx = [0u8; MSG_MAX];
        while let Some(msg) = api.recv(&mut rx) {
            link.last_rx = api.now();
            match m.apply(msg) {
                Effect::Ignored | Effect::Heartbeat | Effect::Ack(_) => {}
                Effect::Linked => {
                    dirty = true;
                    link.widget_dirty = true;
                }
                Effect::Configured => {
                    let n = m.save(stored);
                    if n > 0 { let _ = api.save(&stored[..n]); }
                    dirty = true;
                    link.widget_dirty = true;
                }
                Effect::Changed => {
                    dirty = true;
                    link.widget_dirty = true;
                }
                Effect::Error(_) => dirty = true,
            }
        }
        let now = api.now();
        if m.pending.is_some() && now.wrapping_sub(link.last_rx) > 12_000 {
            m.cancel_pending(2);
            link.last_rx = now;
            dirty = true;
            link.widget_dirty = true;
        }
        if m.connected && now.wrapping_sub(link.last_rx) > 30_000 {
            m.mark_disconnected();
            dirty = true;
            link.widget_dirty = true;
        }
        if link.widget_dirty && now.wrapping_sub(link.widget_last) >= 1000 {
            let result = update_widget(api, m);
            link.widget_dirty = false;
            link.widget_last = now;
            if !link.widget_reported && api.send(&[b'H', b'1', b'W', result as i8 as u8]) >= 0 {
                link.widget_reported = true;
            }
        }
        dirty
    }

    /// Maps a negative `send` result to a diagnostic code shown on the phone row.
    fn send_code(r: i32) -> u8 {
        match r {
            -11 => 11,
            -107 => 107,
            _ => 255,
        }
    }

    /// Sends one already-built command and consumes its sequence number.
    fn send_cmd(api: &Api, m: &mut Model, frame: [u8; 6], seq: &mut u8) {
        *seq = seq.wrapping_add(1);
        let sent = api.send(&frame);
        if sent < 0 { m.cancel_pending(send_code(sent)); }
    }

    fn list_main(
        api: &'static Api,
        m: &mut Model,
        stored: &mut [u8; 384],
        lv: Lv,
        mut batch: Batch,
        mode: Mode,
    ) -> i32 {
        let mut flush = |b: &mut Batch| b.submit(&lv);
        let mut screen = match Screen::build(&mut batch, m, mode, &mut flush) {
            Ok(screen) => screen,
            Err(_) => return 2,
        };
        if flush(&mut batch).is_err() { return 3; }
        let mut seq = 0u8;
        let mut dirty = true;
        let mut redraw_retry = false;
        let mut last_redraw_retry = api.now();
        let mut link = Link::new(api, m);
        let mut long_at = 0u32;
        let mut long_slot = usize::MAX;

        let _ = api.send(&proto::HELLO);
        loop {
            let mut timeout = 250;
            if m.pending.is_some() { timeout = 20; }
            match api.event(timeout) {
                Some(e) if e.kind == BUTTON => {
                    if screen.view == View::Slider {
                        // Own back: close the slider page instead of leaving the app.
                        if screen.leave(&mut batch).is_err() { return 7; }
                        screen = match Screen::build(&mut batch, m, mode, &mut flush) {
                            Ok(s) => s,
                            Err(_) => return 7,
                        };
                        dirty = true;
                    } else if mode == Mode::Menu {
                        return 0;
                    }
                }
                Some(e) if e.kind == CROWN && screen.view == View::Slider => {
                    let slot = screen.slider_slot as usize;
                    if slot < MAX_BUTTONS {
                        let cur = m.buttons[slot].value.min(100) as i16;
                        let v = (cur + e.delta * CROWN_STEP).clamp(0, 100) as u8;
                        if let Some(frame) = m.brightness_frame(slot, v, seq) {
                            send_cmd(api, m, frame, &mut seq);
                        } else if !m.connected {
                            m.cancel_pending(107);
                        }
                        dirty = true;
                    }
                }
                Some(e) if e.kind == LV && e.y == CLICKED => {
                    let now = api.now();
                    let slot = screen.row_slot(e.x as u16);
                    if let Some(slot) = slot {
                        if slot == long_slot && now.wrapping_sub(long_at) < LONG_GRACE_MS {
                            long_slot = usize::MAX;
                        } else if let Some((enabled, dimmer, on, value)) =
                            m.buttons.get(slot).map(|b| (b.enabled, b.is_dimmer(), b.on, b.value))
                        {
                            if enabled && dimmer {
                                // Short press on a device: jump between 0 and 100.
                                let target = if on && value > 0 { 0 } else { 100 };
                                if let Some(frame) = m.brightness_frame(slot, target, seq) {
                                    send_cmd(api, m, frame, &mut seq);
                                } else if !m.connected {
                                    m.cancel_pending(107);
                                }
                                dirty = true;
                            } else if enabled {
                                if let Some(frame) = m.toggle_frame(slot, seq) {
                                    send_cmd(api, m, frame, &mut seq);
                                    dirty = true;
                                } else if !m.connected {
                                    m.cancel_pending(107);
                                    dirty = true;
                                }
                            }
                        }
                    }
                }
                Some(e) if e.kind == LV && e.y == LONG_PRESSED => {
                    if screen.view == View::List {
                        if let Some(slot) = screen.row_slot(e.x as u16) {
                            if m.buttons.get(slot).is_some_and(|b| b.enabled && b.is_dimmer()) {
                                long_at = api.now();
                                long_slot = slot;
                                if screen.leave(&mut batch).is_err() { return 7; }
                                screen = match Screen::build_slider(&mut batch, m, mode, &mut flush, slot) {
                                    Ok(s) => s,
                                    Err(_) => return 7,
                                };
                                dirty = true;
                            }
                        }
                    }
                }
                Some(e) if e.kind == LV && e.y == VALUE_CHANGED => {
                    if screen.is_slider(e.x as u16) {
                        let slot = screen.slider_slot as usize;
                        if slot < MAX_BUTTONS {
                            let v = e.delta.clamp(0, 100) as u8;
                            if let Some(frame) = m.brightness_frame(slot, v, seq) {
                                send_cmd(api, m, frame, &mut seq);
                            } else if !m.connected {
                                m.cancel_pending(107);
                            }
                            dirty = true;
                        }
                    }
                }
                Some(e) if e.kind == PAUSE => {
                    loop {
                        match api.event(u32::MAX).map(|e| e.kind) {
                            Some(BUTTON) if mode == Mode::Menu && screen.view == View::List => return 0,
                            Some(RESUME) => break,
                            _ => {}
                        }
                    }
                    if screen.view == View::Slider {
                        // Returning from a popup lands back on the main list.
                        if screen.leave(&mut batch).is_err() { return 7; }
                        screen = match Screen::build(&mut batch, m, mode, &mut flush) {
                            Ok(s) => s,
                            Err(_) => return 7,
                        };
                    }
                    let _ = api.send(&proto::HELLO);
                    dirty = true;
                }
                _ => {}
            }
            // Flush a brightness queued behind an in-flight command.
            if let Some(frame) = m.take_later(seq) {
                send_cmd(api, m, frame, &mut seq);
                dirty = true;
            }
            if redraw_retry && api.now().wrapping_sub(last_redraw_retry) >= 1000 {
                dirty = true;
                last_redraw_retry = api.now();
            }
            if sync(api, m, stored, &mut link) { dirty = true; }
            if dirty {
                let redraw = screen
                    .update(&mut batch, m, &mut flush)
                    .and_then(|_| flush(&mut batch));
                if redraw.is_err() {
                    // Keep the live BLE session: retry instead of exiting the app.
                    redraw_retry = true;
                    last_redraw_retry = api.now();
                } else {
                    redraw_retry = false;
                }
                dirty = false;
            }
        }
    }
}


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
    use crate::screen::Screen;
    use cmf_wp2_app::lv::{Batch, Lv};
    use cmf_wp2_app::{Api, BUTTON, LV, MSG_MAX, PAUSE, RESUME, stock};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();
    cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET | cmf_wp2_app::PAGE);

    static mut MODEL: Model = Model::new();
    static mut STORE: [u8; 192] = [0; 192];

    /// The widget card: a grey title line, white value lines (the control's name, then its
    /// state). The card is text-only and a tap starts the app; the interactive toggles live
    /// on the page next to the face.
    fn update_widget(api: &Api, m: &Model) -> i32 {
        let chosen = (m.fav != proto::NO_FAVORITE)
            .then(|| m.buttons[m.fav as usize])
            .filter(|b| b.enabled)
            .or_else(|| m.buttons.iter().find(|b| b.enabled).copied());
        let mut val = [0u8; 40];
        let value = if !m.connected {
            "Phone offline"
        } else {
            match chosen {
                Some(b) => {
                    let label = b.label();
                    let n = label.len().min(proto::NAME_MAX);
                    val[..n].copy_from_slice(&label.as_bytes()[..n]);
                    let mut k = n;
                    val[k] = b'\n';
                    k += 1;
                    let state: &[u8] = if b.on { b"ON" } else { b"OFF" };
                    val[k..k + 2].copy_from_slice(state);
                    k += 2;
                    let tail: &[u8] = b" \xc2\xb7 tap to toggle";
                    val[k..k + tail.len()].copy_from_slice(tail);
                    k += tail.len();
                    core::str::from_utf8(&val[..k]).unwrap_or("")
                }
                None => "Set up in the phone app",
            }
        };
        api.widget("Home Assistant", value)
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 };
        let Some(mut batch) = Batch::new() else {
            return -1;
        };
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
        // The same screen runs as the menu app and as the page next to the face; the loader
        // tells the surfaces apart, the app does not have to.
        list_main(api, m, stored, lv, batch)
    }

    /// The connection tail both loops share: phone messages into the model, the
    /// pending/disconnect timeouts, the connection flag and the widget card. Returns true
    /// when the caller's screen should redraw.
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

    fn sync(api: &Api, m: &mut Model, stored: &mut [u8; 192], link: &mut Link) -> bool {
        let mut dirty = false;
        if m.connected != link.last_connection {
            link.last_connection = m.connected;
            dirty = true;
            link.widget_dirty = true;
        }
        for i in 0..MAX_BUTTONS {
            let state = (m.buttons[i].enabled as u8) << 1 | m.buttons[i].on as u8;
            if link.last_state[i] != state {
                link.last_state[i] = state;
                link.widget_dirty = true;
            }
        }
        let mut rx = [0u8; MSG_MAX];
        while let Some(msg) = api.recv(&mut rx) {
            link.last_rx = api.now();
            match m.apply(msg) {
                Effect::Ignored => {}
                Effect::Configured => {
                    let n = m.save(stored);
                    if n > 0 {
                        let _ = api.save(&stored[..n]);
                    }
                    dirty = true;
                    link.widget_dirty = true;
                }
                Effect::Heartbeat => {}
                Effect::Changed => {
                    dirty = true;
                    link.widget_dirty = true;
                }
                Effect::Ack(_) | Effect::Error(_) => dirty = true,
            }
        }
        let now = api.now();
        if m.pending.is_some() && now.wrapping_sub(link.last_rx) > 12_000 {
            m.pending = None;
            m.error = Some(2);
            link.last_rx = now;
            dirty = true;
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

    /// The full control list, as the Apps-menu entry and as the page next to the face.
    /// BUTTON exits from the menu; on the page the crown stays with the watch, so the app
    /// keeps running behind the face until a swipe away pauses it.
    fn list_main(
        api: &'static Api,
        m: &mut Model,
        stored: &mut [u8; 192],
        lv: Lv,
        mut batch: Batch,
    ) -> i32 {
        let mut flush = |b: &mut Batch| b.submit(&lv);
        let mut screen = match Screen::build(&mut batch, m, &mut flush) {
            Ok(screen) => screen,
            Err(_) => return 2,
        };
        if flush(&mut batch).is_err() {
            return 3;
        }
        let mut seq = 0u8;
        let mut dirty = true;
        let mut redraw_retry = false;
        let mut last_redraw_retry = api.now();
        let mut link = Link::new(api, m);

        let _ = api.send(&proto::HELLO);
        loop {
            let mut timeout = 250;
            if m.pending.is_some() {
                timeout = 20;
            }
            match api.event(timeout) {
                Some(e) if e.kind == BUTTON => return 0,
                Some(e) if e.kind == LV && e.y == 7 => {
                    let i = screen.rows.iter().position(|row| row.0 == e.x as u16);
                    if let Some(i) = i {
                        if let Some(frame) = m.toggle_frame(i, seq) {
                            seq = seq.wrapping_add(1);
                            let sent = api.send(&frame);
                            if sent < 0 {
                                m.pending = None;
                                m.pending_target = None;
                                m.pending_seq = None;
                                // Keep the transport failure distinguishable from the
                                // companion's protocol error 1 (busy/invalid slot).
                                m.error = Some(match sent {
                                    -11 => 11,
                                    -107 => 107,
                                    _ => 255,
                                });
                            }
                            dirty = true;
                        }
                    }
                }
                Some(e) if e.kind == PAUSE => {
                    loop {
                        match api.event(u32::MAX).map(|e| e.kind) {
                            Some(BUTTON) => return 0,
                            Some(RESUME) => break,
                            _ => {}
                        }
                    }
                    let _ = api.send(&proto::HELLO);
                    dirty = true;
                }
                _ => {}
            }
            if redraw_retry && api.now().wrapping_sub(last_redraw_retry) >= 1000 {
                dirty = true;
                last_redraw_retry = api.now();
            }
            if sync(api, m, stored, &mut link) {
                dirty = true;
            }
            if dirty {
                let redraw = screen
                    .update(&mut batch, m)
                    .and_then(|_| batch.submit(&lv));
                if redraw.is_err() {
                    // LVGL batches can fail transiently when the loader is changing
                    // surfaces. Returning here closes the app and loses the live BLE
                    // session, so leave it running and retry on the next event/update.
                    m.error = Some(3);
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

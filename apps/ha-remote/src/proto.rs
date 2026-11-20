//! Compact, bounded protocol shared with the Android companion.
//!
//! Frames are ASCII `H1`, a type byte and fixed binary fields. v3 carries ten slots, a
//! control kind per slot (toggle or dimmer) and up to five favorites. Ten 15-byte names do
//! not fit one 128-byte message, so configuration is split: `V`/`K` configure slots 5..10
//! (config, sections), `J` their sections and `U` slots 0..5 plus the favorites; the phone
//! sends V, J, K, U in that order and `U` commits the state to NAND. The legacy `C`/`G`
//! frames from an older companion stay accepted, so an old phone still works.

pub const MAX_BUTTONS: usize = 10;
pub const MAX_FAVORITES: usize = 5;
pub const HALF: usize = 5;
pub const NAME_MAX: usize = 15;
pub const SECTION_MAX: usize = 10;

pub const KIND_TOGGLE: u8 = 0;
pub const KIND_DIMMER: u8 = 1;

pub const CONFIG_A: u8 = b'U';
pub const CONFIG_B: u8 = b'V';
pub const SECTIONS_A: u8 = b'J';
pub const SECTIONS_B: u8 = b'K';
pub const CONFIG: u8 = b'C';
pub const GROUPS: u8 = b'G';
pub const STATE: u8 = b'S';
pub const HEARTBEAT: u8 = b'P';
pub const ACK: u8 = b'A';
pub const ERROR: u8 = b'E';
pub const TOGGLE: u8 = b'T';
pub const BRIGHTNESS: u8 = b'B';
pub const RESULT_OK: u8 = 0;
pub const HELLO: [u8; 3] = *b"H1?";

/// One persisted controls blob: `D3`, count, favorites, then ten fixed records of
/// `enabled, kind, on, value, len, name[15], sec_len, section[10]`.
pub const SAVE_LEN: usize = 3 + MAX_FAVORITES + MAX_BUTTONS * (4 + NAME_MAX + 1 + SECTION_MAX);

/// No favorite in a favorites slot.
pub const NO_FAVORITE: u8 = 0xFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Button {
    pub enabled: bool,
    pub on: bool,
    /// Last HA availability; runtime-only (not persisted): true until the phone says otherwise.
    pub available: bool,
    /// Brightness percent 0..=100 for a dimmer; 0 or 100 mirrors `on` for a toggle.
    pub value: u8,
    pub kind: u8,
    pub name: [u8; NAME_MAX],
    pub len: u8,
    pub section: [u8; SECTION_MAX],
    pub sec_len: u8,
}

impl Button {
    pub const EMPTY: Self = Self {
        enabled: false,
        on: false,
        available: true,
        value: 0,
        kind: KIND_TOGGLE,
        name: [0; NAME_MAX],
        len: 0,
        section: [0; SECTION_MAX],
        sec_len: 0,
    };

    pub fn label(&self) -> &str {
        core::str::from_utf8(&self.name[..self.len as usize]).unwrap_or("Control")
    }

    pub fn section_label(&self) -> &str {
        core::str::from_utf8(&self.section[..self.sec_len as usize]).unwrap_or("")
    }

    pub fn is_dimmer(&self) -> bool { self.kind == KIND_DIMMER }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Ignored,
    Changed,
    /// A half config or sections frame: redraw, mark linked, no NAND write yet.
    Linked,
    /// The commit frame: redraw and persist.
    Configured,
    Heartbeat,
    Ack(u8),
    Error(u8),
}

pub struct Model {
    pub buttons: [Button; MAX_BUTTONS],
    pub favorites: [u8; MAX_FAVORITES],
    pub connected: bool,
    pub pending: Option<u8>,
    pub pending_target: Option<bool>,
    pub pending_value: Option<u8>,
    pub pending_seq: Option<u8>,
    /// Latest brightness the user dragged to while another command was in flight.
    pub later: Option<(u8, u8)>,
    pub error: Option<u8>,
}

impl Model {
    pub const fn new() -> Self {
        Self {
            buttons: [Button::EMPTY; MAX_BUTTONS],
            favorites: [NO_FAVORITE; MAX_FAVORITES],
            connected: false,
            pending: None,
            pending_target: None,
            pending_value: None,
            pending_seq: None,
            later: None,
            error: None,
        }
    }

    /// The widget's control: the first favorite, else the first enabled slot.
    pub fn primary(&self) -> Option<usize> {
        self.favorites
            .iter()
            .filter(|&&f| f != NO_FAVORITE)
            .map(|&f| f as usize)
            .find(|&i| i < MAX_BUTTONS && self.buttons[i].enabled)
            .or_else(|| self.buttons.iter().position(|b| b.enabled))
    }

    pub fn is_favorite(&self, slot: usize) -> bool {
        self.favorites.iter().any(|&f| f as usize == slot)
    }

    fn parse_record(msg: &[u8], p: usize, three_byte: bool) -> Option<(usize, bool, u8, usize, [u8; NAME_MAX])> {
        // three_byte: v3 records carry `enabled, kind, len`; legacy carries `enabled, len`.
        let head = if three_byte { 3 } else { 2 };
        if p + head > msg.len() { return None; }
        let enabled = msg[p];
        let (kind, len_at) = if three_byte { (msg[p + 1], p + 2) } else { (KIND_TOGGLE, p + 1) };
        let len = msg[len_at] as usize;
        if enabled > 1 || len > NAME_MAX { return None; }
        if three_byte && kind != KIND_TOGGLE && kind != KIND_DIMMER { return None; }
        let name_at = p + head;
        if name_at + len > msg.len() { return None; }
        if core::str::from_utf8(&msg[name_at..name_at + len]).is_err() { return None; }
        let mut name = [0u8; NAME_MAX];
        name[..len].copy_from_slice(&msg[name_at..name_at + len]);
        Some((name_at + len, enabled == 1, kind, len, name))
    }

    fn config_half(&mut self, msg: &[u8], base: usize, with_favs: bool) -> Effect {
        if base >= MAX_BUTTONS || msg[3] as usize > HALF { return Effect::Ignored; }
        let n = msg[3] as usize;
        let mut p = 4;
        let mut parsed = [(false, KIND_TOGGLE, 0usize, [0u8; NAME_MAX]); HALF];
        for slot in parsed.iter_mut().take(n) {
            match Self::parse_record(msg, p, true) {
                Some((next, enabled, kind, len, name)) => {
                    p = next;
                    *slot = (enabled, kind, len, name);
                }
                None => return Effect::Ignored,
            }
        }
        let fav_len = if with_favs { MAX_FAVORITES } else { 0 };
        if p + fav_len != msg.len() { return Effect::Ignored; }
        let mut favs = self.favorites;
        if with_favs {
            favs = [NO_FAVORITE; MAX_FAVORITES];
            for (f, &b) in favs.iter_mut().zip(&msg[p..p + MAX_FAVORITES]) {
                if b != NO_FAVORITE && b as usize >= MAX_BUTTONS { return Effect::Ignored; }
                *f = b;
            }
        }
        for (i, &(enabled, kind, len, ref name)) in parsed.iter().enumerate().take(n) {
            let b = &mut self.buttons[base + i];
            b.enabled = enabled;
            b.kind = kind;
            b.len = len as u8;
            b.name = *name;
        }
        for i in n..HALF {
            let b = &mut self.buttons[base + i];
            b.enabled = false;
            b.len = 0;
        }
        if with_favs { self.favorites = favs; }
        self.connected = true;
        if with_favs { Effect::Configured } else { Effect::Linked }
    }

    fn sections_half(&mut self, msg: &[u8], base: usize) -> Effect {
        if base >= MAX_BUTTONS || msg[3] as usize > HALF { return Effect::Ignored; }
        let n = msg[3] as usize;
        let mut p = 4;
        let mut sections = [[0u8; SECTION_MAX]; HALF];
        let mut lens = [0usize; HALF];
        for i in 0..n {
            if p >= msg.len() { return Effect::Ignored; }
            let len = msg[p] as usize;
            p += 1;
            if len > SECTION_MAX || p + len > msg.len() { return Effect::Ignored; }
            if core::str::from_utf8(&msg[p..p + len]).is_err() { return Effect::Ignored; }
            sections[i][..len].copy_from_slice(&msg[p..p + len]);
            lens[i] = len;
            p += len;
        }
        if p != msg.len() { return Effect::Ignored; }
        for i in 0..HALF {
            let b = &mut self.buttons[base + i];
            if i < n {
                b.sec_len = lens[i] as u8;
                b.section = sections[i];
            } else {
                b.sec_len = 0;
            }
        }
        self.connected = true;
        Effect::Linked
    }

    fn legacy_config(&mut self, msg: &[u8]) -> Effect {
        let n = msg[3] as usize;
        if n > HALF { return Effect::Ignored; }
        let mut p = 4;
        let mut parsed = [(false, 0usize, [0u8; NAME_MAX]); HALF];
        for slot in parsed.iter_mut().take(n) {
            match Self::parse_record(msg, p, false) {
                Some((next, enabled, _, len, name)) => { p = next; *slot = (enabled, len, name); }
                None => return Effect::Ignored,
            }
        }
        let mut favs = self.favorites;
        if p == msg.len() {
            // older phone, no favorite byte: keep what we have
        } else if p + 1 == msg.len() {
            let fav = msg[p];
            if fav != NO_FAVORITE && fav as usize >= MAX_BUTTONS { return Effect::Ignored; }
            favs = [NO_FAVORITE; MAX_FAVORITES];
            favs[0] = fav;
        } else {
            return Effect::Ignored;
        }
        for (i, &(enabled, len, ref name)) in parsed.iter().enumerate().take(n) {
            let b = &mut self.buttons[i];
            b.enabled = enabled;
            b.len = len as u8;
            b.name = *name;
        }
        for i in n..HALF {
            self.buttons[i].enabled = false;
            self.buttons[i].len = 0;
        }
        self.favorites = favs;
        self.connected = true;
        Effect::Configured
    }

    fn legacy_groups(&mut self, msg: &[u8]) -> Effect {
        if msg[3] as usize != HALF { return Effect::Ignored; }
        let mut p = 4;
        let mut sections = [[0u8; SECTION_MAX]; HALF];
        let mut lens = [0usize; HALF];
        for i in 0..HALF {
            if p >= msg.len() { return Effect::Ignored; }
            let len = msg[p] as usize;
            p += 1;
            if len > SECTION_MAX || p + len > msg.len() { return Effect::Ignored; }
            if core::str::from_utf8(&msg[p..p + len]).is_err() { return Effect::Ignored; }
            sections[i][..len].copy_from_slice(&msg[p..p + len]);
            lens[i] = len;
            p += len;
        }
        let mut favs = self.favorites;
        if p + 1 == msg.len() {
            let fav = msg[p];
            if fav != NO_FAVORITE && fav as usize >= MAX_BUTTONS { return Effect::Ignored; }
            favs = [NO_FAVORITE; MAX_FAVORITES];
            favs[0] = fav;
        } else if p != msg.len() {
            return Effect::Ignored;
        }
        for i in 0..HALF {
            self.buttons[i].sec_len = lens[i] as u8;
            self.buttons[i].section = sections[i];
        }
        self.favorites = favs;
        self.connected = true;
        Effect::Configured
    }

    pub fn apply(&mut self, msg: &[u8]) -> Effect {
        if msg.len() < 3 || &msg[..2] != b"H1" { return Effect::Ignored; }
        match msg[2] {
            CONFIG_A if msg.len() >= 4 => self.config_half(msg, 0, true),
            CONFIG_B if msg.len() >= 4 => self.config_half(msg, HALF, false),
            SECTIONS_A if msg.len() >= 4 => self.sections_half(msg, 0),
            SECTIONS_B if msg.len() >= 4 => self.sections_half(msg, HALF),
            CONFIG if msg.len() >= 4 => self.legacy_config(msg),
            GROUPS if msg.len() >= 4 => self.legacy_groups(msg),
            HEARTBEAT if msg.len() == 3 => { self.connected = true; Effect::Heartbeat }
            STATE if matches!(msg.len(), 6 | 7 | 8) => {
                let i = msg[3] as usize;
                if i >= MAX_BUTTONS || msg[4] > 1 { return Effect::Ignored; }
                let (value, result, available) = match msg.len() {
                    8 => {
                        if msg[5] > 100 || msg[7] > 1 { return Effect::Ignored; }
                        (msg[5], msg[6], msg[7] == 1)
                    }
                    7 => {
                        if msg[5] > 100 { return Effect::Ignored; }
                        (msg[5], msg[6], true)
                    }
                    _ => (if msg[4] == 1 { 100 } else { 0 }, msg[5], true),
                };
                let b = &mut self.buttons[i];
                b.on = msg[4] == 1;
                b.value = value;
                b.available = available;
                self.connected = true;
                let was_pending = self.pending == Some(i as u8);
                if self.pending == Some(i as u8) {
                    self.pending = None;
                    self.pending_target = None;
                    self.pending_value = None;
                    self.pending_seq = None;
                }
                if available {
                    if result != RESULT_OK { self.error = Some(result); } else { self.error = None; }
                } else if was_pending {
                    // The device itself is offline; the command is resolved and that is a
                    // device state, not an app/link failure.
                    self.error = None;
                }
                Effect::Changed
            }
            ACK if msg.len() == 4 && self.pending_seq == Some(msg[3]) => {
                self.error = None;
                Effect::Ack(msg[3])
            }
            ERROR if msg.len() == 5 && self.pending_seq == Some(msg[3]) => {
                let code = msg[4];
                self.cancel_pending(code);
                Effect::Error(code)
            }
            _ => Effect::Ignored,
        }
    }

    pub fn toggle_frame(&mut self, i: usize, seq: u8) -> Option<[u8; 6]> {
        if i >= MAX_BUTTONS || !self.connected || !self.buttons[i].enabled || self.pending.is_some() {
            return None;
        }
        let target = !self.buttons[i].on;
        self.pending = Some(i as u8);
        self.pending_target = Some(target);
        self.pending_value = None;
        self.pending_seq = Some(seq);
        Some([b'H', b'1', TOGGLE, seq, i as u8, target as u8])
    }

    /// Starts (or queues) a brightness change. Returns the frame to send now, or None when
    /// another command is in flight (the value is remembered and flushed by [`take_later`]).
    pub fn brightness_frame(&mut self, i: usize, value: u8, seq: u8) -> Option<[u8; 6]> {
        if i >= MAX_BUTTONS || !self.connected || !self.buttons[i].enabled || !self.buttons[i].is_dimmer() {
            return None;
        }
        let value = value.min(100);
        // Show the dragged/pressed value immediately; the phone's state confirms it.
        self.buttons[i].value = value;
        if self.pending.is_some() {
            self.later = Some((i as u8, value));
            return None;
        }
        self.pending = Some(i as u8);
        self.pending_value = Some(value);
        self.pending_target = None;
        self.pending_seq = Some(seq);
        Some([b'H', b'1', BRIGHTNESS, seq, i as u8, value])
    }

    /// Sends the queued drag value once the line is free again.
    pub fn take_later(&mut self, seq: u8) -> Option<[u8; 6]> {
        if self.pending.is_some() { return None; }
        let (i, v) = self.later?;
        self.later = None;
        self.brightness_frame(i as usize, v, seq)
    }

    /// A send failed or a command was rejected: drop every trace of it and show the code.
    pub fn cancel_pending(&mut self, code: u8) {
        self.pending = None;
        self.pending_target = None;
        self.pending_value = None;
        self.pending_seq = None;
        self.later = None;
        self.error = Some(code);
    }

    pub fn mark_disconnected(&mut self) {
        self.connected = false;
        self.pending = None;
        self.pending_target = None;
        self.pending_value = None;
        self.pending_seq = None;
        self.later = None;
        self.error = None;
    }

    pub fn load(&mut self, bytes: &[u8]) {
        if bytes.len() == SAVE_LEN && &bytes[..2] == b"D3" && bytes[2] as usize == MAX_BUTTONS {
            let base = 3 + MAX_FAVORITES;
            let stride = 4 + NAME_MAX + 1 + SECTION_MAX;
            let mut buttons = [Button::EMPTY; MAX_BUTTONS];
            let mut favs = [NO_FAVORITE; MAX_FAVORITES];
            for (f, &b) in favs.iter_mut().zip(&bytes[3..3 + MAX_FAVORITES]) {
                if b != NO_FAVORITE && b as usize >= MAX_BUTTONS { return; }
                *f = b;
            }
            for i in 0..MAX_BUTTONS {
                let p = base + i * stride;
                let len = bytes[p + 4] as usize;
                let sec_len = bytes[p + 5 + NAME_MAX] as usize;
                if bytes[p] > 1
                    || (bytes[p + 1] != KIND_TOGGLE && bytes[p + 1] != KIND_DIMMER)
                    || bytes[p + 2] > 1
                    || bytes[p + 3] > 100
                    || len > NAME_MAX
                    || sec_len > SECTION_MAX
                    || core::str::from_utf8(&bytes[p + 5..p + 5 + len]).is_err()
                    || core::str::from_utf8(&bytes[p + 6 + NAME_MAX..p + 6 + NAME_MAX + sec_len]).is_err()
                {
                    return;
                }
                let b = &mut buttons[i];
                b.enabled = bytes[p] == 1;
                b.kind = bytes[p + 1];
                b.on = bytes[p + 2] == 1;
                b.value = bytes[p + 3];
                b.len = len as u8;
                b.name[..len].copy_from_slice(&bytes[p + 5..p + 5 + len]);
                b.sec_len = sec_len as u8;
                b.section[..sec_len].copy_from_slice(&bytes[p + 6 + NAME_MAX..p + 6 + NAME_MAX + sec_len]);
            }
            self.buttons = buttons;
            self.favorites = favs;
            return;
        }
        // D2: the old five-slot blob (enabled, len, on, name[15], sec_len, section[10]).
        if bytes.len() == 4 + HALF * (3 + NAME_MAX + 1 + SECTION_MAX) && &bytes[..2] == b"D2" && bytes[2] as usize == HALF {
            let stride = 3 + NAME_MAX + 1 + SECTION_MAX;
            let mut buttons = [Button::EMPTY; MAX_BUTTONS];
            let fav = bytes[3];
            if fav != NO_FAVORITE && fav as usize >= HALF { return; }
            for i in 0..HALF {
                let p = 4 + i * stride;
                let len = bytes[p + 1] as usize;
                let sec_len = bytes[p + 3 + NAME_MAX] as usize;
                if bytes[p] > 1 || bytes[p + 2] > 1 || len > NAME_MAX || sec_len > SECTION_MAX
                    || core::str::from_utf8(&bytes[p + 3..p + 3 + len]).is_err()
                    || core::str::from_utf8(&bytes[p + 4 + NAME_MAX..p + 4 + NAME_MAX + sec_len]).is_err()
                {
                    return;
                }
                let b = &mut buttons[i];
                b.enabled = bytes[p] == 1;
                b.on = bytes[p + 2] == 1;
                b.value = if b.on { 100 } else { 0 };
                b.len = len as u8;
                b.name[..len].copy_from_slice(&bytes[p + 3..p + 3 + len]);
                b.sec_len = sec_len as u8;
                b.section[..sec_len].copy_from_slice(&bytes[p + 4 + NAME_MAX..p + 4 + NAME_MAX + sec_len]);
            }
            self.buttons = buttons;
            self.favorites = [NO_FAVORITE; MAX_FAVORITES];
            self.favorites[0] = fav;
        }
    }

    pub fn save(&self, bytes: &mut [u8]) -> usize {
        if bytes.len() < SAVE_LEN { return 0; }
        bytes[..SAVE_LEN].fill(0);
        bytes[..2].copy_from_slice(b"D3");
        bytes[2] = MAX_BUTTONS as u8;
        bytes[3..3 + MAX_FAVORITES].copy_from_slice(&self.favorites);
        let base = 3 + MAX_FAVORITES;
        let stride = 4 + NAME_MAX + 1 + SECTION_MAX;
        for i in 0..MAX_BUTTONS {
            let p = base + i * stride;
            let b = &self.buttons[i];
            bytes[p] = b.enabled as u8;
            bytes[p + 1] = b.kind;
            bytes[p + 2] = b.on as u8;
            bytes[p + 3] = b.value;
            bytes[p + 4] = b.len;
            bytes[p + 5..p + 5 + b.len as usize].copy_from_slice(&b.name[..b.len as usize]);
            bytes[p + 5 + NAME_MAX] = b.sec_len;
            bytes[p + 6 + NAME_MAX..p + 6 + NAME_MAX + b.sec_len as usize]
                .copy_from_slice(&b.section[..b.sec_len as usize]);
        }
        SAVE_LEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_frame(t: u8, base: usize, specs: &[(&str, bool, u8)], favs: Option<&[u8]>) -> Vec<u8> {
        let mut f = vec![b'H', b'1', t, specs.len() as u8];
        for (name, enabled, kind) in specs {
            let raw = name.as_bytes();
            f.push(*enabled as u8);
            f.push(*kind);
            f.push(raw.len() as u8);
            f.extend_from_slice(raw);
        }
        if let Some(favs) = favs { f.extend_from_slice(favs); }
        let _ = base;
        f
    }

    fn sections_frame(t: u8, sections: &[&str]) -> Vec<u8> {
        let mut f = vec![b'H', b'1', t, sections.len() as u8];
        for s in sections {
            f.push(s.len() as u8);
            f.extend_from_slice(s.as_bytes());
        }
        f
    }

    fn ten_slots() -> Model {
        let mut m = Model::new();
        let half_a = [("Lamp", true, KIND_DIMMER), ("Fan", true, KIND_TOGGLE), ("Desk", true, KIND_TOGGLE), ("Hall", false, KIND_TOGGLE), ("Plug", true, KIND_TOGGLE)];
        let half_b = [("Strip", true, KIND_DIMMER), ("Porch", false, KIND_TOGGLE), ("Bed", true, KIND_DIMMER), ("TV", true, KIND_TOGGLE), ("Heater", true, KIND_TOGGLE)];
        let favs = [0u8, 5, 7, 9, NO_FAVORITE];
        assert_eq!(m.apply(&config_frame(CONFIG_B, HALF, &half_b, None)), Effect::Linked);
        assert_eq!(m.apply(&sections_frame(SECTIONS_A, &["Room", "Room", "", "", ""])), Effect::Linked);
        assert_eq!(m.apply(&sections_frame(SECTIONS_B, &["", "", "Room", "", ""])), Effect::Linked);
        assert_eq!(m.apply(&config_frame(CONFIG_A, 0, &half_a, Some(&favs))), Effect::Configured);
        m
    }

    #[test]
    fn ten_slots_kinds_sections_and_favorites() {
        let m = ten_slots();
        assert_eq!(m.buttons[0].label(), "Lamp");
        assert!(m.buttons[0].is_dimmer());
        assert_eq!(m.buttons[5].label(), "Strip");
        assert!(m.buttons[5].is_dimmer());
        assert!(!m.buttons[1].is_dimmer());
        assert!(!m.buttons[6].enabled);
        assert_eq!(m.buttons[0].section_label(), "Room");
        assert_eq!(m.buttons[7].section_label(), "Room");
        assert_eq!(m.favorites, [0, 5, 7, 9, NO_FAVORITE]);
        assert!(m.is_favorite(9));
        assert!(!m.is_favorite(1));
        assert_eq!(m.primary(), Some(0));
    }

    #[test]
    fn legacy_five_slot_companion_still_configures() {
        let mut m = Model::new();
        assert_eq!(m.apply(b"H1C\x02\x01\x04Desk\x01\x05Hall1"), Effect::Configured);
        assert_eq!(m.apply(b"H1G\x05\x04Room\x00\x00\x00\x00\x00"), Effect::Configured);
        assert!(m.buttons[0].enabled);
        assert_eq!(m.buttons[0].label(), "Desk");
        assert_eq!(m.buttons[0].section_label(), "Room");
        assert!(!m.buttons[0].is_dimmer());
        assert_eq!(m.favorites[0], 0);
        for i in HALF..MAX_BUTTONS { assert!(!m.buttons[i].enabled); }
        assert_eq!(m.apply(b"H1C\x06"), Effect::Ignored);
    }

    #[test]
    fn toggle_state_old_and_new_brightness_flow() {
        let mut m = ten_slots();
        assert_eq!(m.toggle_frame(0, 7), Some(*b"H1T\x07\x00\x01"));
        assert!(m.toggle_frame(0, 8).is_none());
        // old 6-byte state from an older companion: value mirrors on/off
        assert_eq!(m.apply(b"H1S\x00\x01\x00"), Effect::Changed);
        assert!(m.buttons[0].on);
        assert_eq!(m.buttons[0].value, 100);
        assert!(m.pending.is_none());
        // new 7-byte state: authoritative brightness
        assert_eq!(m.apply(b"H1S\x05\x01\x19\x00"), Effect::Changed);
        assert_eq!(m.buttons[5].value, 25);
        // drag while busy is queued, then flushed when the line frees
        assert_eq!(m.apply(b"H1S\x05\x01\x19\x00"), Effect::Changed);
        let f = m.brightness_frame(7, 40, 1).unwrap();
        assert_eq!(f, [b'H', b'1', b'B', 1, 7, 40]);
        assert_eq!(m.buttons[7].value, 40);
        assert!(m.brightness_frame(7, 80, 2).is_none());
        assert_eq!(m.later, Some((7, 80)));
        assert_eq!(m.apply(b"H1S\x07\x01\x19\x00"), Effect::Changed);
        let f2 = m.take_later(3).unwrap();
        assert_eq!(f2, [b'H', b'1', b'B', 3, 7, 80]);
        assert_eq!(m.buttons[7].value, 80);
    }

    #[test]
    fn errors_timeouts_and_disconnect_clear_pending_state() {
        let mut m = ten_slots();
        m.toggle_frame(0, 19).unwrap();
        assert_eq!(m.apply(b"H1A\x12"), Effect::Ignored);
        assert_eq!(m.apply(b"H1E\x13\x02"), Effect::Error(2));
        assert_eq!(m.pending, None);
        assert_eq!(m.error, Some(2));
        m.brightness_frame(5, 60, 1).unwrap();
        m.brightness_frame(5, 75, 2); // queued
        assert_eq!(m.later, Some((5, 75)));
        m.cancel_pending(107);
        assert_eq!(m.pending, None);
        assert_eq!(m.later, None);
        assert_eq!(m.error, Some(107));
        m.mark_disconnected();
        assert!(!m.connected);
        assert_eq!(m.error, None);
        assert!(m.toggle_frame(0, 1).is_none());
        assert!(m.brightness_frame(0, 10, 1).is_none());
        assert_eq!(m.apply(b"H1P"), Effect::Heartbeat);
        assert!(m.connected);
    }

    #[test]
    fn availability_marks_a_device_inactive_without_an_app_error() {
        let mut m = ten_slots();
        m.toggle_frame(0, 9).unwrap();
        // 8-byte state: slot, on, value, result, available
        assert_eq!(m.apply(b"H1S\x00\x00\x00\x00\x00"), Effect::Changed);
        assert!(!m.buttons[0].available);
        assert!(m.pending.is_none());
        assert_eq!(m.error, None);
        // HA reports the entity again
        assert_eq!(m.apply(b"H1S\x00\x01\x64\x00\x01"), Effect::Changed);
        assert!(m.buttons[0].available);
        // an availability byte other than 0/1 is rejected
        assert_eq!(m.apply(b"H1S\x01\x00\x00\x00\x02"), Effect::Ignored);
        assert!(m.buttons[1].available);
    }

    #[test]
    fn malformed_v3_frames_are_ignored_without_mutation() {
        let mut m = ten_slots();
        let before = m.buttons[0].label().to_string();
        assert_eq!(m.apply(&config_frame(CONFIG_A, 0, &[("X", true, KIND_DIMMER)], Some(&[0, 1, 2, 3, 10]))), Effect::Ignored); // favorite 10 is not a slot
        let mut bad_kind = config_frame(CONFIG_A, 0, &[("X", true, 9)], Some(&[0, 1, 2, 3, 4]));
        bad_kind[3] = 1;
        assert_eq!(m.apply(&bad_kind), Effect::Ignored);
        assert_eq!(m.apply(&sections_frame(SECTIONS_A, &["Room", "Room", "", "", "", "Extra"])), Effect::Ignored);
        assert_eq!(m.apply(b"H1S\x0A\x01\x32\x00"), Effect::Ignored); // slot 10
        assert_eq!(m.apply(b"H1S\x00\x01\x65\x00"), Effect::Ignored); // 101 percent
        assert_eq!(m.buttons[0].label(), before);
    }

    #[test]
    fn d3_round_trip_and_d2_migration() {
        let mut m = ten_slots();
        m.buttons[0].on = true;
        m.buttons[0].value = 65;
        m.buttons[7].on = true;
        m.buttons[7].value = 100;
        let mut data = [0u8; SAVE_LEN];
        let n = m.save(&mut data);
        let mut loaded = Model::new();
        loaded.load(&data[..n]);
        assert_eq!(loaded.buttons[0].label(), "Lamp");
        assert_eq!(loaded.buttons[0].value, 65);
        assert!(loaded.buttons[7].is_dimmer());
        assert_eq!(loaded.favorites, m.favorites);
        // the old five-slot blob migrates: toggles, one favorite, slots 5.. empty
        let mut old = [0u8; 4 + HALF * (3 + NAME_MAX + 1 + SECTION_MAX)];
        old[..2].copy_from_slice(b"D2");
        old[2] = HALF as u8;
        old[3] = 1;
        let p = 4;
        old[p] = 1; old[p + 1] = 4; old[p + 2] = 1;
        old[p + 3..p + 7].copy_from_slice(b"Desk");
        let mut mig = Model::new();
        mig.load(&old);
        assert!(mig.buttons[0].enabled && mig.buttons[0].on);
        assert_eq!(mig.buttons[0].value, 100);
        assert!(!mig.buttons[0].is_dimmer());
        assert_eq!(mig.favorites[0], 1);
        for i in HALF..MAX_BUTTONS { assert!(!mig.buttons[i].enabled); }
    }
}

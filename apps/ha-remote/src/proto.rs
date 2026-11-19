//! Compact, bounded protocol shared with the Android companion.
//!
//! Frames are ASCII `H1`, a type byte and fixed binary fields. Names are UTF-8 and capped at
//! 15 bytes, section titles at 10 bytes. The phone sends the controls (CONFIG) and then their
//! sections and favorite (GROUPS) as two messages, because one message cannot carry both
//! within MSG_MAX. The favorite is a slot index, or 0xFF for none.

pub const MAX_BUTTONS: usize = 5;
pub const NAME_MAX: usize = 15;
pub const SECTION_MAX: usize = 10;
pub const CONFIG: u8 = b'C';
pub const GROUPS: u8 = b'G';
pub const STATE: u8 = b'S';
pub const HEARTBEAT: u8 = b'P';
pub const ACK: u8 = b'A';
pub const ERROR: u8 = b'E';
pub const TOGGLE: u8 = b'T';
pub const RESULT_OK: u8 = 0;
pub const HELLO: [u8; 3] = *b"H1?";

/// One persisted controls blob.
pub const SAVE_LEN: usize = 4 + MAX_BUTTONS * (3 + NAME_MAX + 1 + SECTION_MAX);

/// No favorite control.
pub const NO_FAVORITE: u8 = 0xFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Button {
    pub enabled: bool,
    pub on: bool,
    pub name: [u8; NAME_MAX],
    pub len: u8,
    pub section: [u8; SECTION_MAX],
    pub sec_len: u8,
}

impl Button {
    pub const EMPTY: Self = Self {
        enabled: false,
        on: false,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Ignored,
    Changed,
    Configured,
    Heartbeat,
    Ack(u8),
    Error(u8),
}

pub struct Model {
    pub buttons: [Button; MAX_BUTTONS],
    pub fav: u8,
    pub connected: bool,
    pub pending: Option<u8>,
    pub pending_target: Option<bool>,
    pub pending_seq: Option<u8>,
    pub error: Option<u8>,
}

impl Model {
    pub const fn new() -> Self {
        Self {
            buttons: [Button::EMPTY; MAX_BUTTONS],
            fav: NO_FAVORITE,
            connected: false,
            pending: None,
            pending_target: None,
            pending_seq: None,
            error: None,
        }
    }

    pub fn load(&mut self, bytes: &[u8]) {
        if bytes.len() != SAVE_LEN || &bytes[..2] != b"D2" {
            return;
        }
        if bytes[2] as usize != MAX_BUTTONS {
            return;
        }
        let fav = bytes[3];
        if fav != NO_FAVORITE && fav as usize >= MAX_BUTTONS {
            return;
        }
        for i in 0..MAX_BUTTONS {
            let p = 4 + i * (3 + NAME_MAX + 1 + SECTION_MAX);
            let len = bytes[p + 1] as usize;
            let sec_len = bytes[p + 3 + NAME_MAX] as usize;
            if bytes[p] > 1
                || bytes[p + 2] > 1
                || len > NAME_MAX
                || sec_len > SECTION_MAX
                || core::str::from_utf8(&bytes[p + 3..p + 3 + len]).is_err()
                || core::str::from_utf8(&bytes[p + 4 + NAME_MAX..p + 4 + NAME_MAX + sec_len]).is_err()
            {
                return;
            }
            self.buttons[i].enabled = bytes[p] == 1;
            self.buttons[i].on = bytes[p + 2] == 1;
            self.buttons[i].len = len as u8;
            self.buttons[i].name[..len].copy_from_slice(&bytes[p + 3..p + 3 + len]);
            self.buttons[i].sec_len = sec_len as u8;
            self.buttons[i].section[..sec_len]
                .copy_from_slice(&bytes[p + 4 + NAME_MAX..p + 4 + NAME_MAX + sec_len]);
        }
        self.fav = fav;
    }

    pub fn save(&self, bytes: &mut [u8]) -> usize {
        if bytes.len() < SAVE_LEN {
            return 0;
        }
        bytes[..SAVE_LEN].fill(0);
        bytes[..2].copy_from_slice(b"D2");
        bytes[2] = MAX_BUTTONS as u8;
        bytes[3] = self.fav;
        for i in 0..MAX_BUTTONS {
            let p = 4 + i * (3 + NAME_MAX + 1 + SECTION_MAX);
            let b = &self.buttons[i];
            bytes[p] = b.enabled as u8;
            bytes[p + 1] = b.len;
            bytes[p + 2] = b.on as u8;
            bytes[p + 3..p + 3 + b.len as usize].copy_from_slice(&b.name[..b.len as usize]);
            bytes[p + 3 + NAME_MAX] = b.sec_len;
            bytes[p + 4 + NAME_MAX..p + 4 + NAME_MAX + b.sec_len as usize]
                .copy_from_slice(&b.section[..b.sec_len as usize]);
        }
        SAVE_LEN
    }

    pub fn apply(&mut self, msg: &[u8]) -> Effect {
        if msg.len() < 3 || &msg[..2] != b"H1" {
            return Effect::Ignored;
        }
        match msg[2] {
            CONFIG if msg.len() >= 4 => {
                let count = (msg[3] as usize).min(MAX_BUTTONS);
                if msg[3] as usize > MAX_BUTTONS {
                    return Effect::Ignored;
                }
                let mut next = [Button::EMPTY; MAX_BUTTONS];
                let mut p = 4;
                for b in next.iter_mut().take(count) {
                    if p + 2 > msg.len() {
                        return Effect::Ignored;
                    }
                    let enabled = msg[p] == 1;
                    let len = msg[p + 1] as usize;
                    p += 2;
                    if msg[p - 2] > 1
                        || len > NAME_MAX
                        || p + len > msg.len()
                        || core::str::from_utf8(&msg[p..p + len]).is_err()
                    {
                        return Effect::Ignored;
                    }
                    b.enabled = enabled;
                    b.len = len as u8;
                    b.name[..len].copy_from_slice(&msg[p..p + len]);
                    p += len;
                }
                // The favorite byte is optional in CONFIG: an older phone sends none and
                // keeps the current favorite. GROUPS always carries it.
                let fav = if p == msg.len() {
                    self.fav
                } else if p + 1 == msg.len() {
                    let fav = msg[p];
                    if fav != NO_FAVORITE && fav as usize >= MAX_BUTTONS {
                        return Effect::Ignored;
                    }
                    p += 1;
                    fav
                } else {
                    return Effect::Ignored;
                };
                if p != msg.len() {
                    return Effect::Ignored;
                }
                for i in 0..MAX_BUTTONS {
                    next[i].on = self.buttons[i].on;
                    next[i].section = self.buttons[i].section;
                    next[i].sec_len = self.buttons[i].sec_len;
                }
                self.buttons = next;
                self.fav = fav;
                self.connected = true;
                Effect::Configured
            }
            GROUPS if msg.len() >= 4 => {
                if msg[3] as usize != MAX_BUTTONS {
                    return Effect::Ignored;
                }
                let mut p = 4;
                for b in self.buttons.iter_mut() {
                    if p + 1 > msg.len() {
                        return Effect::Ignored;
                    }
                    let len = msg[p] as usize;
                    p += 1;
                    if len > SECTION_MAX || p + len > msg.len() {
                        return Effect::Ignored;
                    }
                    if core::str::from_utf8(&msg[p..p + len]).is_err() {
                        return Effect::Ignored;
                    }
                    b.sec_len = len as u8;
                    b.section[..len].copy_from_slice(&msg[p..p + len]);
                    p += len;
                }
                if p != msg.len() {
                    // GROUPS always carries the favorite: a slot index, or 0xFF for none.
                    if p + 1 != msg.len() {
                        return Effect::Ignored;
                    }
                    let fav = msg[p];
                    if fav != NO_FAVORITE && fav as usize >= MAX_BUTTONS {
                        return Effect::Ignored;
                    }
                    p += 1;
                    self.fav = fav;
                }
                if p != msg.len() {
                    return Effect::Ignored;
                }
                self.connected = true;
                Effect::Configured
            }
            HEARTBEAT if msg.len() == 3 => {
                self.connected = true;
                Effect::Heartbeat
            }
            STATE if msg.len() == 6 && (msg[3] as usize) < MAX_BUTTONS && msg[4] <= 1 => {
                let i = msg[3] as usize;
                self.buttons[i].on = msg[4] == 1;
                self.connected = true;
                if self.pending == Some(i as u8)
                    && (msg[5] != RESULT_OK || self.pending_target == Some(self.buttons[i].on))
                {
                    self.pending = None;
                    self.pending_target = None;
                    self.pending_seq = None;
                }
                if msg[5] != RESULT_OK {
                    self.error = Some(msg[5]);
                } else if self.pending.is_none() {
                    self.error = None;
                }
                Effect::Changed
            }
            ACK if msg.len() == 4 => {
                if self.pending_seq == Some(msg[3]) {
                    self.error = None;
                    Effect::Ack(msg[3])
                } else {
                    Effect::Ignored
                }
            }
            ERROR if msg.len() == 5 && self.pending_seq == Some(msg[3]) => {
                self.pending = None;
                self.pending_target = None;
                self.pending_seq = None;
                self.error = Some(msg[4]);
                Effect::Error(msg[4])
            }
            _ => Effect::Ignored,
        }
    }

    pub fn toggle_frame(&mut self, i: usize, seq: u8) -> Option<[u8; 6]> {
        if i >= MAX_BUTTONS || !self.connected || !self.buttons[i].enabled || self.pending.is_some()
        {
            return None;
        }
        self.pending = Some(i as u8);
        let target = !self.buttons[i].on;
        self.pending_target = Some(target);
        self.pending_seq = Some(seq);
        Some([b'H', b'1', TOGGLE, seq, i as u8, target as u8])
    }

    pub fn mark_disconnected(&mut self) {
        self.connected = false;
        self.pending = None;
        self.pending_target = None;
        self.pending_seq = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_state_toggle_and_bad_packets() {
        let mut m = Model::new();
        assert_eq!(m.apply(b"H1C\x01\x01\x04Lamp"), Effect::Configured);
        assert!(m.buttons[0].enabled);
        assert_eq!(m.buttons[0].label(), "Lamp");
        assert_eq!(m.toggle_frame(0, 7), Some(*b"H1T\x07\x00\x01"));
        assert!(m.toggle_frame(0, 8).is_none());
        assert_eq!(m.apply(b"H1S\x00\x01\x00"), Effect::Changed);
        assert!(m.buttons[0].on);
        assert!(m.pending.is_none());
        assert_eq!(m.apply(b"H1C\x06"), Effect::Ignored);
    }

    #[test]
    fn persisted_config_round_trips_names_sections_and_states() {
        let mut m = Model::new();
        m.apply(b"H1C\x02\x01\x04Desk\x01\x05Hall1");
        assert_eq!(m.apply(b"H1G\x05\x04Room\x00\x00\x00\x00\x01"), Effect::Configured);
        m.toggle_frame(0, 3).unwrap();
        m.apply(b"H1S\x00\x01\x00");
        let mut data = [0u8; SAVE_LEN];
        let n = m.save(&mut data);
        let mut loaded = Model::new();
        loaded.load(&data[..n]);
        assert_eq!(loaded.buttons[0].label(), "Desk");
        assert_eq!(loaded.buttons[0].section_label(), "Room");
        assert_eq!(loaded.buttons[1].label(), "Hall1");
        assert_eq!(loaded.buttons[1].section_label(), "");
        assert!(loaded.buttons[0].on);
        assert_eq!(loaded.fav, 1);
    }

    #[test]
    fn groups_must_carry_every_slot_and_valid_utf8() {
        let mut m = Model::new();
        assert_eq!(m.apply(b"H1G\x04\x00\x00\x00\x00"), Effect::Ignored);
        assert_eq!(m.apply(b"H1G\x05\xFF\x00\x00\x00\x00\xFF"), Effect::Ignored);
        assert_eq!(m.apply(b"H1G\x05\x01\xC3\x00\x00\x00\x00\xFF"), Effect::Ignored);
        assert_eq!(m.apply(b"H1G\x05\x01z\x00\x00\x00\x00\xFF"), Effect::Configured);
        assert_eq!(m.apply(b"H1G\x05\x01z\x00\x00\x00\x00\x05"), Effect::Ignored);
    }

    #[test]
    fn config_keeps_sections_and_state_updates_only_resolve_matching_toggle() {
        let mut m = Model::new();
        m.apply(b"H1C\x01\x01\x04Desk");
        m.apply(b"H1G\x05\x04Down\x00\x00\x00\x00\xFF");
        m.apply(b"H1C\x01\x01\x04Desk");
        assert_eq!(m.buttons[0].section_label(), "Down");
        m.toggle_frame(0, 3).unwrap();
        assert_eq!(m.apply(b"H1S\x01\x01\x00"), Effect::Changed);
        assert_eq!(m.pending, Some(0));
        assert_eq!(m.apply(b"H1S\x00\x01\x00"), Effect::Changed);
        assert_eq!(m.pending, None);
    }

    #[test]
    fn errors_and_acks_must_match_the_pending_sequence() {
        let mut m = Model::new();
        m.apply(b"H1C\x01\x01\x04Lamp");
        m.toggle_frame(0, 19).unwrap();
        assert_eq!(m.apply(b"H1A\x12"), Effect::Ignored);
        assert_eq!(m.apply(b"H1E\x12\x01"), Effect::Ignored);
        assert_eq!(m.pending, Some(0));
        assert_eq!(m.apply(b"H1E\x13\x02"), Effect::Error(2));
        assert_eq!(m.pending, None);
        assert_eq!(m.error, Some(2));
    }

    #[test]
    fn heartbeat_restores_connection_without_changing_controls() {
        let mut m = Model::new();
        m.apply(b"H1C\x01\x01\x04Lamp");
        m.mark_disconnected();
        assert_eq!(m.apply(b"H1P"), Effect::Heartbeat);
        assert!(m.connected);
        assert_eq!(m.buttons[0].label(), "Lamp");
    }
}

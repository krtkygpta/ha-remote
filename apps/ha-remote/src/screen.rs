use crate::proto::{MAX_BUTTONS, Model, NAME_MAX};
use cmf_wp2_app::lv::{
    Align, BUF, Batch, Class, CONTENT, CHECKED, Ev, Flag, LongMode, LvError, Obj, Prop, ROOT,
};
use cmf_wp2_app::stock;

const ROW_BYTES: usize = 1200;
const UPDATE_BYTES: usize = 400;
const ROW_H: i32 = 110;
const ROW_GAP: i32 = 2;
const ICON_SLOT: i32 = 80;
const ICON_X: i16 = 8;
const NAME_X: i16 = 104;
const NAME_W: i32 = stock::ROW_W - NAME_X as i32 - 84;
const SWITCH_W: i32 = 56;
const SWITCH_H: i32 = 28;
const KNOB: i32 = 24;
const PCT_W: i32 = 84;

/// Font slot for the slider page's big percent number; opened beside the system font.
pub(crate) const FONT_BIG: u8 = 1;

/// Which surface is showing: the Apps row / face page list, or a device's slider page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode { Menu, Page }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum View { List, Slider }

pub(crate) struct Screen {
    pub(crate) mode: Mode,
    pub(crate) view: View,
    page: Obj,
    // list surface (ROOT handles while the slider page is showing)
    phone_detail: Obj,
    headers: [Obj; MAX_BUTTONS],
    names: [Obj; MAX_BUTTONS],
    pills: [Obj; MAX_BUTTONS],
    knobs: [Obj; MAX_BUTTONS],
    details: [Obj; MAX_BUTTONS],
    inactive: [Obj; MAX_BUTTONS],
    icons: [Obj; MAX_BUTTONS],
    icon_kinds: [u8; MAX_BUTTONS],
    pub(crate) rows: [Obj; MAX_BUTTONS],
    // slider surface
    slider: Obj,
    value_label: Obj,
    /// The slot whose slider page is open (0xFF on the list).
    pub(crate) slider_slot: u8,
}

/// A row shows when its slot is enabled; the page next to the face also requires a favorite.
fn visible(m: &Model, i: usize, mode: Mode) -> bool {
    m.buttons.get(i).is_some_and(|b| b.enabled)
        && (mode == Mode::Menu || m.is_favorite(i))
}

fn header_for<'a>(m: &'a Model, i: usize, mode: Mode) -> &'a str {
    if !visible(m, i, mode) { return ""; }
    let section = m.buttons[i].section_label();
    if section.is_empty() { return ""; }
    match (0..i).rev().find(|&j| visible(m, j, mode)) {
        Some(prev) if m.buttons[prev].section_label() == section => "",
        _ => section,
    }
}

/// "65%" / "100%" for a dimmer row or the slider page.
pub(crate) fn pct_text<'a>(value: u8, buf: &'a mut [u8; 5]) -> &'a str {
    let v = value.min(100);
    let n = if v >= 100 {
        buf[..3].copy_from_slice(b"100");
        3
    } else if v >= 10 {
        buf[0] = b'0' + v / 10;
        buf[1] = b'0' + v % 10;
        2
    } else {
        buf[0] = b'0' + v;
        1
    };
    buf[n] = b'%';
    core::str::from_utf8(&buf[..n + 1]).unwrap_or("")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Glyph {
    Bulb,
    Switch,
    Fan,
    Phone,
}

const LIGHT_WORDS: &[&str] = &["light", "lamp", "bulb", "chandelier", "sconce", "lantern"];

fn word_matches(word: &str, needles: &[&str]) -> bool {
    let bare = word.strip_suffix('s').or_else(|| word.strip_suffix('S')).unwrap_or(word);
    needles.iter().any(|n| word.eq_ignore_ascii_case(n) || bare.eq_ignore_ascii_case(n))
}

fn words_of(name: &str) -> impl Iterator<Item = &str> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
}

fn glyph_kind(name: &str) -> Glyph {
    if words_of(name).any(|w| word_matches(w, LIGHT_WORDS)) {
        Glyph::Bulb
    } else if words_of(name).any(|w| word_matches(w, &["switch"])) {
        Glyph::Switch
    } else if words_of(name).any(|w| word_matches(w, &["fan"])) {
        Glyph::Fan
    } else {
        Glyph::Switch
    }
}

fn write_label<'a>(m: &'a Model, i: usize, buf: &'a mut [u8; NAME_MAX + 2]) -> &'a str {
    let label = match m.buttons.get(i) { Some(b) => b.label(), None => return "Control" };
    let prefix: &[u8] = if m.pending == Some(i as u8) {
        b"\xe2\x80\xa6 "
    } else if m.error.is_some() {
        b"! "
    } else {
        b""
    };
    let copy = prefix.len().min(buf.len());
    buf[..copy].copy_from_slice(&prefix[..copy]);
    let n = (prefix.len() + label.len()).min(buf.len());
    buf[prefix.len().min(n)..n].copy_from_slice(&label.as_bytes()[..n - prefix.len().min(n)]);
    core::str::from_utf8(&buf[..n]).unwrap_or(label)
}

fn glyph_piece(
    b: &mut Batch,
    parent: Obj,
    w: i32,
    h: i32,
    x: i16,
    y: i16,
    radius: i32,
    color: i32,
) -> Result<Obj, LvError> {
    let piece = b.create(Class::Obj, parent)?;
    b.size(piece, w, h)?;
    b.align(piece, Align::Center, x, y)?;
    b.style(piece, Prop::Radius, 0, radius)?;
    b.style(piece, Prop::BgColor, 0, color)?;
    b.style(piece, Prop::BgOpa, 0, 255)?;
    b.style(piece, Prop::BorderWidth, 0, 0)?;
    b.flags(piece, &[], &[Flag::Clickable, Flag::Scrollable])?;
    Ok(piece)
}

fn glyph_blade(b: &mut Batch, parent: Obj, rotation: u16) -> Result<Obj, LvError> {
    let blade = b.create(Class::Arc, parent)?;
    b.size(blade, 56, 56)?;
    b.align(blade, Align::Center, 0, 0)?;
    b.angles(blade, 15, 80, rotation)?;
    b.value(blade, 0, false)?;
    b.style(blade, Prop::ArcWidth, 0, 12)?;
    b.style(blade, Prop::ArcRounded, 0, 1)?;
    b.style(blade, Prop::ArcColor, 0, stock::WHITE)?;
    b.style(blade, Prop::ArcOpa, 0, 255)?;
    b.style(blade, Prop::ArcOpa, stock::INDICATOR, 0)?;
    b.style(blade, Prop::BgOpa, stock::KNOB, 0)?;
    b.flags(blade, &[], &[Flag::Clickable, Flag::Scrollable])?;
    Ok(blade)
}

fn draw_glyph(b: &mut Batch, slot: Obj, kind: Glyph) -> Result<(), LvError> {
    match kind {
        Glyph::Bulb => {
            glyph_piece(b, slot, 46, 46, 0, -11, stock::CIRCLE, stock::WHITE)?;
            glyph_piece(b, slot, 20, 9, 0, 14, 4, stock::WHITE)?;
            glyph_piece(b, slot, 12, 8, 0, 25, 3, stock::WHITE)?;
        }
        Glyph::Switch => {
            glyph_piece(b, slot, 48, 28, 0, 0, 14, stock::WHITE)?;
            glyph_piece(b, slot, 20, 20, 10, 0, stock::CIRCLE, stock::BLACK)?;
        }
        Glyph::Fan => {
            glyph_piece(b, slot, 18, 18, 0, 0, stock::CIRCLE, stock::WHITE)?;
            glyph_blade(b, slot, 0)?;
            glyph_blade(b, slot, 90)?;
            glyph_blade(b, slot, 180)?;
            glyph_blade(b, slot, 270)?;
        }
        Glyph::Phone => {
            glyph_piece(b, slot, 30, 48, 0, 0, 9, stock::WHITE)?;
            glyph_piece(b, slot, 22, 34, 0, -3, 5, stock::BLACK)?;
            glyph_piece(b, slot, 6, 6, 0, 16, stock::CIRCLE, stock::WHITE)?;
        }
    }
    Ok(())
}

fn draw_icon(b: &mut Batch, row: Obj, kind: Glyph) -> Result<Obj, LvError> {
    let slot = b.create(Class::Obj, row)?;
    b.size(slot, ICON_SLOT, ICON_SLOT)?;
    b.align(slot, Align::LeftMid, ICON_X, 0)?;
    b.style(slot, Prop::BgOpa, 0, 0)?;
    b.style(slot, Prop::BorderWidth, 0, 0)?;
    b.flags(slot, &[], &[Flag::Clickable, Flag::Scrollable])?;
    draw_glyph(b, slot, kind)?;
    Ok(slot)
}

impl Screen {
    fn empty(mode: Mode, view: View, page: Obj) -> Self {
        Self {
            mode,
            view,
            page,
            phone_detail: ROOT,
            headers: [ROOT; MAX_BUTTONS],
            names: [ROOT; MAX_BUTTONS],
            pills: [ROOT; MAX_BUTTONS],
            knobs: [ROOT; MAX_BUTTONS],
            details: [ROOT; MAX_BUTTONS],
            inactive: [ROOT; MAX_BUTTONS],
            icons: [ROOT; MAX_BUTTONS],
            icon_kinds: [0; MAX_BUTTONS],
            rows: [ROOT; MAX_BUTTONS],
            slider: ROOT,
            value_label: ROOT,
            slider_slot: 0xFF,
        }
    }

    /// The control list, as the Apps row or the page next to the face.
    pub(crate) fn build(
        b: &mut Batch,
        m: &Model,
        mode: Mode,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
    ) -> Result<Self, LvError> {
        stock::theme(b)?;
        let (page, _bar) = stock::focus_page(b, 0)?;
        let mut s = Self::empty(mode, View::List, page);
        b.style(page, Prop::PadRow, 0, ROW_GAP)?;
        b.style(page, Prop::PadTop, 0, 40)?;
        let title = b.create(Class::Label, page)?;
        b.size(title, stock::ROW_W, 72)?;
        b.style(title, Prop::TextColor, 0, stock::WHITE)?;
        b.style(title, Prop::TextAlign, 0, 2)?;
        b.text(title, "Home Assistant")?;

        Self::room(b, flush, ROW_BYTES)?;
        let phone = Self::row_base(b, page)?;
        draw_icon(b, phone, Glyph::Phone)?;
        let phone_name = b.create(Class::Label, phone)?;
        b.text(phone_name, "Phone")?;
        b.align(phone_name, Align::LeftMid, NAME_X, 0)?;
        s.phone_detail = b.create(Class::Label, phone)?;
        b.text(s.phone_detail, Self::phone_status(m))?;
        b.style(s.phone_detail, Prop::TextColor, 0, stock::GREY)?;
        b.align(s.phone_detail, Align::RightMid, -20, 0)?;

        for i in 0..MAX_BUTTONS {
            Self::room(b, flush, ROW_BYTES)?;
            let header = b.create(Class::Label, page)?;
            let text = header_for(m, i, mode);
            b.size(header, stock::ROW_W, CONTENT)?;
            b.style(header, Prop::TextAlign, 0, 2)?;
            b.style(header, Prop::TextColor, 0, stock::GREY)?;
            b.text(header, text)?;
            if text.is_empty() { b.flags(header, &[Flag::Hidden], &[])?; }
            s.headers[i] = header;

            let row = Self::row_base(b, page)?;
            s.rows[i] = row;
            if !visible(m, i, mode) { b.flags(row, &[Flag::Hidden], &[])?; }
            let button = &m.buttons[i];
            let kind = glyph_kind(button.label());
            s.icons[i] = draw_icon(b, row, kind)?;
            s.icon_kinds[i] = kind as u8;
            let name_label = b.create(Class::Label, row)?;
            b.size(name_label, NAME_W, CONTENT)?;
            b.long_mode(name_label, LongMode::Dot)?;
            b.text(name_label, if button.enabled { button.label() } else { "Not configured" })?;
            s.names[i] = name_label;

            let pill = b.create(Class::Obj, row)?;
            b.size(pill, SWITCH_W, SWITCH_H)?;
            b.align(pill, Align::RightMid, -20, 0)?;
            b.style(pill, Prop::Radius, 0, stock::CIRCLE)?;
            b.style(pill, Prop::BgColor, 0, stock::GREY)?;
            b.style(pill, Prop::BgOpa, 0, 255)?;
            b.flags(pill, &[], &[Flag::Clickable, Flag::Scrollable])?;
            let knob = b.create(Class::Obj, pill)?;
            b.size(knob, KNOB, KNOB)?;
            b.style(knob, Prop::Radius, 0, stock::CIRCLE)?;
            b.style(knob, Prop::BgColor, 0, stock::WHITE)?;
            b.style(knob, Prop::BgOpa, 0, 255)?;
            b.flags(knob, &[], &[Flag::Clickable, Flag::Scrollable])?;
            b.align(knob, Align::LeftMid, 2, 0)?;
            s.pills[i] = pill;
            s.knobs[i] = knob;

            // A dimmer's row shows its brightness; a switch row shows the pill.
            let pct = b.create(Class::Label, row)?;
            b.size(pct, PCT_W, CONTENT)?;
            b.style(pct, Prop::TextAlign, 0, 2)?;
            b.style(pct, Prop::TextColor, 0, stock::GREY)?;
            s.details[i] = pct;
            let inactive = b.create(Class::Label, row)?;
            b.text(inactive, "Inactive")?;
            b.size(inactive, 120, CONTENT)?;
            b.style(inactive, Prop::TextColor, 0, stock::GREY)?;
            b.style(inactive, Prop::TextAlign, 0, 2)?;
            s.inactive[i] = inactive;
            Self::row_state_layout(
                b, name_label, pill, pct, inactive, button.is_dimmer(), button.available,
            )?;
        }
        Ok(s)
    }

    /// A device's brightness page: the stock-looking thick vertical slider, a big percent
    /// number, and the crown (the page is non-scrollable, so knob() routes CROWN to us).
    pub(crate) fn build_slider(
        b: &mut Batch,
        m: &Model,
        mode: Mode,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
        slot: usize,
    ) -> Result<Self, LvError> {
        if slot >= MAX_BUTTONS || !m.buttons[slot].is_dimmer() { return Err(LvError::Code(-22)); }
        stock::theme(b)?;
        Self::room(b, flush, ROW_BYTES)?;
        let page = b.create(Class::Obj, ROOT)?;
        b.size(page, stock::SCREEN, stock::SCREEN)?;
        for p in [
            Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight,
            Prop::BorderWidth, Prop::Radius, Prop::BgOpa,
        ] {
            b.style(page, p, 0, 0)?;
        }
        b.flags(page, &[], &[Flag::Scrollable])?;
        b.knob(page)?;

        let mut s = Self::empty(mode, View::Slider, page);
        s.slider_slot = slot as u8;
        let title = b.create(Class::Label, page)?;
        b.size(title, stock::ROW_W, 72)?;
        b.style(title, Prop::TextColor, 0, stock::WHITE)?;
        b.style(title, Prop::TextAlign, 0, 2)?;
        b.align(title, Align::Center, 0, -170)?;
        b.text(title, m.buttons[slot].label())?;

        let mut pbuf = [0u8; 5];
        s.value_label = b.create(Class::Label, page)?;
        b.size(s.value_label, 240, 80)?;
        b.style(s.value_label, Prop::TextFont, 0, FONT_BIG as i32)?;
        b.style(s.value_label, Prop::TextAlign, 0, 2)?;
        b.align(s.value_label, Align::Center, 0, -88)?;
        b.text(s.value_label, pct_text(m.buttons[slot].value, &mut pbuf))?;

        s.slider = b.create(Class::Slider, page)?;
        b.size(s.slider, 104, 236)?;
        b.align(s.slider, Align::Center, 0, 54)?;
        b.range(s.slider, 0, 100)?;
        b.value(s.slider, m.buttons[slot].value.min(100) as i32, false)?;
        for (part, colour) in [
            (0, stock::LETTER_BG),
            (stock::INDICATOR, stock::WHITE),
            (stock::KNOB, stock::WHITE),
        ] {
            b.style(s.slider, Prop::BgColor, part, colour)?;
            b.style(s.slider, Prop::BgOpa, part, 255)?;
            b.style(s.slider, Prop::Radius, part, stock::CIRCLE)?;
        }
        b.style(s.slider, Prop::BorderWidth, stock::KNOB, 6)?;
        b.style(s.slider, Prop::BorderColor, stock::KNOB, stock::BLACK)?;
        b.events(s.slider, &[Ev::ValueChanged])?;
        Ok(s)
    }

    /// Deletes the current screen (its page, with every object and its scroll bar), like
    /// ui-gallery's screen switch; the next build starts from a clean handle table.
    pub(crate) fn leave(&mut self, b: &mut Batch) -> Result<(), LvError> {
        if self.page != ROOT { b.del(self.page)?; }
        Ok(())
    }

    fn row_base(b: &mut Batch, parent: Obj) -> Result<Obj, LvError> {
        let row = stock::focus_row(b, parent)?;
        b.size(row, stock::ROW_W, ROW_H)?;
        b.style(row, Prop::BgOpa, CHECKED, 0)?;
        // Long press opens the device's slider page.
        b.events(row, &[Ev::LongPressed])?;
        Ok(row)
    }

    /// Switch/percent/inactive visibility for the row's kind and availability.
    fn row_state_layout(
        b: &mut Batch,
        name: Obj,
        pill: Obj,
        pct: Obj,
        inactive: Obj,
        dimmer: bool,
        available: bool,
    ) -> Result<(), LvError> {
        b.align(name, Align::LeftMid, NAME_X, 0)?;
        b.align(pct, Align::RightMid, -20, 0)?;
        b.align(inactive, Align::RightMid, -20, 0)?;
        b.style(name, Prop::TextColor, 0, if available { stock::WHITE } else { stock::GREY })?;
        if !available {
            b.flags(pct, &[Flag::Hidden], &[])?;
            b.flags(inactive, &[], &[Flag::Hidden])?;
            b.flags(pill, &[Flag::Hidden], &[])?;
        } else if dimmer {
            b.flags(pill, &[Flag::Hidden], &[])?;
            b.flags(inactive, &[Flag::Hidden], &[])?;
            b.flags(pct, &[], &[Flag::Hidden])?;
        } else {
            b.flags(pct, &[Flag::Hidden], &[])?;
            b.flags(inactive, &[Flag::Hidden], &[])?;
            b.flags(pill, &[], &[Flag::Hidden])?;
        }
        Ok(())
    }

    fn room(
        b: &mut Batch,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
        need: usize,
    ) -> Result<(), LvError> {
        if b.bytes().len() + need > BUF { flush(b)?; }
        Ok(())
    }

    fn phone_status(m: &Model) -> &'static str {
        if !m.connected {
            "Offline"
        } else if m.error == Some(2) {
            "Timeout"
        } else if m.error.is_some() {
            "Failed"
        } else if m.pending.is_some() {
            "Busy"
        } else {
            "Online"
        }
    }

    pub(crate) fn row_slot(&self, handle: u16) -> Option<usize> {
        if self.view != View::List { return None; }
        self.rows.iter().position(|r| r.0 == handle && handle != ROOT.0)
    }

    /// True when [handle] is the slider page's slider.
    pub(crate) fn is_slider(&self, handle: u16) -> bool {
        self.view == View::Slider && self.slider.0 == handle
    }

    pub(crate) fn update(
        &mut self,
        b: &mut Batch,
        m: &Model,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
    ) -> Result<(), LvError> {
        match self.view {
            View::Slider => {
                let i = self.slider_slot as usize;
                if i >= MAX_BUTTONS { return Ok(()); }
                let value = m.buttons[i].value.min(100);
                let mut pbuf = [0u8; 5];
                b.text(self.value_label, pct_text(value, &mut pbuf))?;
                b.value(self.slider, value as i32, false)?;
                Ok(())
            }
            View::List => {
                b.text(self.phone_detail, Self::phone_status(m))?;
                for i in 0..MAX_BUTTONS {
                    Self::room(b, flush, UPDATE_BYTES)?;
                    let text = header_for(m, i, self.mode);
                    b.text(self.headers[i], text)?;
                    if text.is_empty() { b.flags(self.headers[i], &[Flag::Hidden], &[])?; }
                    else { b.flags(self.headers[i], &[], &[Flag::Hidden])?; }
                    let button = &m.buttons[i];
                    if visible(m, i, self.mode) { b.flags(self.rows[i], &[], &[Flag::Hidden])?; }
                    else { b.flags(self.rows[i], &[Flag::Hidden], &[])?; }
                    let kind = glyph_kind(button.label());
                    if kind as u8 != self.icon_kinds[i] {
                        Self::room(b, flush, UPDATE_BYTES)?;
                        let _ = b.del(self.icons[i]);
                        match draw_icon(b, self.rows[i], kind) {
                            Ok(icon) => {
                                self.icons[i] = icon;
                                self.icon_kinds[i] = kind as u8;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    let mut buf = [0u8; NAME_MAX + 2];
                    b.text(self.names[i], write_label(m, i, &mut buf))?;
                    let dimmer = button.is_dimmer();
                    if dimmer && button.available {
                        let mut pbuf = [0u8; 5];
                        b.text(self.details[i], pct_text(button.value, &mut pbuf))?;
                    }
                    if !dimmer && button.available {
                        b.style(self.pills[i], Prop::BgColor, 0, if button.on { stock::WHITE } else { stock::GREY })?;
                        b.style(self.knobs[i], Prop::BgColor, 0, if button.on { stock::BLACK } else { stock::WHITE })?;
                        b.align(
                            self.knobs[i],
                            if button.on { Align::RightMid } else { Align::LeftMid },
                            if button.on { -2 } else { 2 },
                            0,
                        )?;
                    }
                    Self::row_state_layout(
                        b,
                        self.names[i],
                        self.pills[i],
                        self.details[i],
                        self.inactive[i],
                        dimmer,
                        button.available,
                    )?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Effect, KIND_DIMMER, NO_FAVORITE};
    use cmf_wp2_app::lv::StockFont;
    use wp2_core::lvcmd::{self, Table};

    const FONTS: u8 = (1 << stock::FONT) | (1 << FONT_BIG);

    fn open_fonts(b: &mut Batch) {
        stock::font(b).unwrap();
        b.font(FONT_BIG, StockFont::F48M).unwrap();
    }

    fn validate_and_apply(b: &mut Batch, table: &mut Table) -> Result<(), LvError> {
        assert_eq!(lvcmd::validate(b.bytes(), table, FONTS), Ok(()));
        for record in lvcmd::records(b.bytes()) {
            lvcmd::apply(table, &record.unwrap());
        }
        b.clear();
        Ok(())
    }

    fn menu_model() -> Model {
        let mut m = Model::new();
        let mut v = vec![b'H', b'1', b'V', 5];
        for (name, kind, en) in [("Strip", KIND_DIMMER, 1u8), ("Porch", 0, 1), ("Bed", KIND_DIMMER, 1), ("TV", 0, 1), ("Heater", 0, 0)] {
            v.extend([en, kind, name.len() as u8]);
            v.extend_from_slice(name.as_bytes());
        }
        assert_eq!(m.apply(&v), Effect::Linked);
        let mut u = vec![b'H', b'1', b'U', 3];
        for (name, kind, en) in [("Lamp", KIND_DIMMER, 1u8), ("Fan", 0, 1), ("Desk", 0, 1)] {
            u.extend([en, kind, name.len() as u8]);
            u.extend_from_slice(name.as_bytes());
        }
        u.extend([0, 5, 7, 9, 0xFF]);
        assert_eq!(m.apply(&u), Effect::Configured);
        m
    }

    #[test]
    fn menu_build_updates_and_lifecycle_validate() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        open_fonts(&mut b);
        assert_eq!(lvcmd::validate(b.bytes(), &Table::new(), FONTS), Ok(()));
        b.clear();

        let mut model = menu_model();
        let mut table = Table::new();
        let mut screen = Screen::build(&mut b, &model, Mode::Menu, &mut |batch| {
            validate_and_apply(batch, &mut table)
        })
        .unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();

        assert!(table.has(wp2_core::lvcmd::Class::ScrollBar));
        for i in 0..MAX_BUTTONS {
            assert_eq!(table.class(screen.rows[i].0), Some(wp2_core::lvcmd::Class::Obj));
            assert_eq!(table.class(screen.details[i].0), Some(wp2_core::lvcmd::Class::Label));
            assert_eq!(table.class(screen.pills[i].0), Some(wp2_core::lvcmd::Class::Obj));
            assert_eq!(table.class(screen.inactive[i].0), Some(wp2_core::lvcmd::Class::Label));
        }
        // state, failure, disconnect and the inactive state all redraw cleanly
        assert_eq!(model.apply(b"H1S\x00\x01\x19\x00\x01"), Effect::Changed);
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        model.apply(b"H1S\x02\x00\x00\x01\x01");
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        model.mark_disconnected();
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        model.apply(b"H1S\x00\x00\x00\x00\x00");
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        // a rename swaps the glyph
        model.apply(b"H1C\x01\x01\x04Desk");
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();

        // switch to a dimmer's slider page and back: delete + rebuild, ui-gallery style
        screen.leave(&mut b).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        let mut slider = Screen::build_slider(&mut b, &model, Mode::Menu, &mut |batch| {
            validate_and_apply(batch, &mut table)
        }, 0)
        .unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        assert!(slider.is_slider(slider.slider.0));
        model.buttons[0].value = 65;
        slider.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        slider.leave(&mut b).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        let back = Screen::build(&mut b, &model, Mode::Menu, &mut |batch| {
            validate_and_apply(batch, &mut table)
        })
        .unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        assert_eq!(back.view, View::List);
    }

    #[test]
    fn page_shows_only_favorites_and_headers_follow_visible_rows() {
        let mut m = menu_model();
        m.apply(b"H1J\x05\x04East\x04East\x04West\x00\x00");
        m.apply(b"H1K\x05\x04East\x00\x04West\x00\x00");
        assert!(visible(&m, 0, Mode::Page));
        assert!(!visible(&m, 1, Mode::Page));
        assert!(!visible(&m, 3, Mode::Page));
        assert!(visible(&m, 5, Mode::Page));
        assert!(visible(&m, 7, Mode::Page));
        assert_eq!(header_for(&m, 0, Mode::Page), "East");
        assert_eq!(header_for(&m, 7, Mode::Page), "West");
        assert!(visible(&m, 1, Mode::Menu));
        assert_eq!(header_for(&m, 1, Mode::Menu), "");
        assert_eq!(header_for(&m, 5, Mode::Menu), "East");
        assert_eq!(header_for(&m, 7, Mode::Menu), "West");
    }

    #[test]
    fn rows_map_back_to_slots_and_pct_formats() {
        let m = menu_model();
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let mut flush = |batch: &mut Batch| {
            batch.clear();
            Ok(())
        };
        let s = Screen::build(&mut b, &m, Mode::Menu, &mut flush).unwrap();
        assert_eq!(s.row_slot(s.rows[3].0), Some(3));
        assert_eq!(s.row_slot(ROOT.0), None);
        assert!(!s.is_slider(s.rows[3].0));
        let mut buf = [0u8; 5];
        assert_eq!(pct_text(0, &mut buf), "0%");
        assert_eq!(pct_text(7, &mut buf), "7%");
        assert_eq!(pct_text(65, &mut buf), "65%");
        assert_eq!(pct_text(100, &mut buf), "100%");
        let _ = m.primary();
        assert_eq!(m.favorites.iter().filter(|&&f| f != NO_FAVORITE).count(), 4);
    }

    #[test]
    fn section_headers_show_only_at_group_starts() {
        let mut m = Model::new();
        m.apply(b"H1C\x03\x01\x02A1\x01\x02A2\x01\x02B1");
        m.apply(b"H1G\x05\x04East\x04East\x04West\x00\x00\xFF");
        assert_eq!(header_for(&m, 0, Mode::Menu), "East");
        assert_eq!(header_for(&m, 1, Mode::Menu), "");
        assert_eq!(header_for(&m, 2, Mode::Menu), "West");
    }

    #[test]
    fn control_names_pick_their_icon() {
        assert!(word_matches("LIGHTS", LIGHT_WORDS));
        assert!(word_matches("Lamps", LIGHT_WORDS));
        assert_eq!(glyph_kind("Living Room Light"), Glyph::Bulb);
        assert_eq!(glyph_kind("Fan with Light"), Glyph::Bulb);
        assert_eq!(glyph_kind("Ceiling Fan"), Glyph::Fan);
        assert_eq!(glyph_kind("Main Switch"), Glyph::Switch);
    }
}

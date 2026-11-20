use crate::proto::{MAX_BUTTONS, Model, NAME_MAX};
use cmf_wp2_app::lv::{
    Align, BUF, Batch, Class, CONTENT, CHECKED, Ev, Flag, LongMode, LvError, Obj, Prop,
};
use cmf_wp2_app::stock;

/// Bytes reserved before building one header+row group (fan glyphs make rows the biggest).
const ROW_BYTES: usize = 1200;
/// Bytes reserved before writing one row's live update, including an icon swap.
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

/// The dimmer: the name rides the top half and the stock slider spans the lower half,
/// like the volume slider in settings.
const SLIDER_W: i32 = stock::ROW_W - NAME_X as i32 - 20;
const SLIDER_H: i32 = 40;
const NAME_Y_DIMMER: i16 = -26;
const SLIDER_Y: i16 = 27;

/// Which surfaces the screen is showing: the Apps row lists every control, the page next
/// to the watch face shows favorites only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode { Menu, Page }

pub(crate) struct Screen {
    mode: Mode,
    phone_detail: Obj,
    headers: [Obj; MAX_BUTTONS],
    names: [Obj; MAX_BUTTONS],
    pills: [Obj; MAX_BUTTONS],
    knobs: [Obj; MAX_BUTTONS],
    sliders: [Obj; MAX_BUTTONS],
    /// Shown in place of the switch/slider while the entity is unavailable in HA.
    inactive: [Obj; MAX_BUTTONS],
    icons: [Obj; MAX_BUTTONS],
    icon_kinds: [u8; MAX_BUTTONS],
    /// The row belongs to slot `i`; kept so events map back to controls.
    pub(crate) rows: [Obj; MAX_BUTTONS],
}

/// A row shows when its slot is enabled; the page also requires a favorite.
fn visible(m: &Model, i: usize, mode: Mode) -> bool {
    m.buttons.get(i).is_some_and(|b| b.enabled)
        && (mode == Mode::Menu || m.is_favorite(i))
}

/// The section header above row `i`: shown only on the first visible row of its section,
/// skipping invisible rows so a section never starts without a heading.
fn header_for<'a>(m: &'a Model, i: usize, mode: Mode) -> &'a str {
    if !visible(m, i, mode) { return ""; }
    let section = m.buttons[i].section_label();
    if section.is_empty() { return ""; }
    match (0..i).rev().find(|&j| visible(m, j, mode)) {
        Some(prev) if m.buttons[prev].section_label() == section => "",
        _ => section,
    }
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

/// The row name with a "…" prefix while its command is in flight, "!" after an error.
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
    pub(crate) fn build(
        b: &mut Batch,
        m: &Model,
        mode: Mode,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
    ) -> Result<Self, LvError> {
        stock::theme(b)?;
        let (page, _bar) = stock::focus_page(b, 0)?;
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
        let phone_detail = b.create(Class::Label, phone)?;
        b.text(phone_detail, Self::phone_status(m))?;
        b.style(phone_detail, Prop::TextColor, 0, stock::GREY)?;
        b.align(phone_detail, Align::RightMid, -20, 0)?;

        let mut headers = [Obj(0); MAX_BUTTONS];
        let mut names = [Obj(0); MAX_BUTTONS];
        let mut pills = [Obj(0); MAX_BUTTONS];
        let mut knobs = [Obj(0); MAX_BUTTONS];
        let mut sliders = [Obj(0); MAX_BUTTONS];
        let mut inactives = [Obj(0); MAX_BUTTONS];
        let mut rows = [Obj(0); MAX_BUTTONS];
        let mut icons = [Obj(0); MAX_BUTTONS];
        let mut icon_kinds = [0u8; MAX_BUTTONS];
        for i in 0..MAX_BUTTONS {
            Self::room(b, flush, ROW_BYTES)?;
            let header = b.create(Class::Label, page)?;
            let text = header_for(m, i, mode);
            b.size(header, stock::ROW_W, CONTENT)?;
            b.style(header, Prop::TextAlign, 0, 2)?;
            b.style(header, Prop::TextColor, 0, stock::GREY)?;
            b.text(header, text)?;
            if text.is_empty() { b.flags(header, &[Flag::Hidden], &[])?; }
            headers[i] = header;

            let row = Self::row_base(b, page)?;
            rows[i] = row;
            if !visible(m, i, mode) { b.flags(row, &[Flag::Hidden], &[])?; }
            let button = &m.buttons[i];
            let kind = glyph_kind(button.label());
            icons[i] = draw_icon(b, row, kind)?;
            icon_kinds[i] = kind as u8;
            let name_label = b.create(Class::Label, row)?;
            b.size(name_label, NAME_W, CONTENT)?;
            b.long_mode(name_label, LongMode::Dot)?;
            b.text(name_label, if button.enabled { button.label() } else { "Not configured" })?;
            names[i] = name_label;

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
            pills[i] = pill;
            knobs[i] = knob;

            let slider = b.create(Class::Slider, row)?;
            b.size(slider, SLIDER_W, SLIDER_H)?;
            b.range(slider, 0, 100)?;
            b.value(slider, button.value.min(100) as i32, false)?;
            stock::slider(b, slider)?;
            b.events(slider, &[Ev::ValueChanged])?;
            sliders[i] = slider;

            let inactive = b.create(Class::Label, row)?;
            b.text(inactive, "Inactive")?;
            b.size(inactive, 120, CONTENT)?;
            b.style(inactive, Prop::TextColor, 0, stock::GREY)?;
            b.style(inactive, Prop::TextAlign, 0, 2)?;
            inactives[i] = inactive;

            // Kind and availability are set by the phone and can arrive or change after
            // this first build.
            Self::row_state_layout(
                b,
                names[i],
                pills[i],
                sliders[i],
                inactives[i],
                button.is_dimmer(),
                button.available,
            )?;
        }
        Ok(Self {
            mode,
            phone_detail: Obj(phone_detail.0),
            headers,
            names,
            pills,
            knobs,
            sliders,
            inactive: inactives,
            icons,
            icon_kinds,
            rows,
        })
    }

    /// The row's shape for the current kind and availability: name position/colour and
    /// which of the switch, slider and "Inactive" label is visible.
    fn row_state_layout(
        b: &mut Batch,
        name: Obj,
        pill: Obj,
        slider: Obj,
        inactive: Obj,
        dimmer: bool,
        available: bool,
    ) -> Result<(), LvError> {
        if dimmer {
            b.align(name, Align::LeftMid, NAME_X, NAME_Y_DIMMER)?;
            b.align(inactive, Align::LeftMid, NAME_X, SLIDER_Y)?;
        } else {
            b.align(name, Align::LeftMid, NAME_X, 0)?;
            b.align(inactive, Align::RightMid, -20, 0)?;
        }
        b.style(name, Prop::TextColor, 0, if available { stock::WHITE } else { stock::GREY })?;
        if !available {
            b.flags(pill, &[Flag::Hidden], &[])?;
            b.flags(slider, &[Flag::Hidden], &[])?;
            b.flags(inactive, &[], &[Flag::Hidden])?;
        } else {
            b.flags(inactive, &[Flag::Hidden], &[])?;
            if dimmer {
                b.flags(pill, &[Flag::Hidden], &[])?;
                b.flags(slider, &[], &[Flag::Hidden])?;
            } else {
                b.flags(slider, &[Flag::Hidden], &[])?;
                b.flags(pill, &[], &[Flag::Hidden])?;
            }
        }
        Ok(())
    }

    fn row_base(b: &mut Batch, parent: Obj) -> Result<Obj, LvError> {
        let row = stock::focus_row(b, parent)?;
        b.size(row, stock::ROW_W, ROW_H)?;
        // The focus loader marks the centered row CHECKED; keep it transparent so only the
        // fisheye moves it (no grey box follows the scroll position).
        b.style(row, Prop::BgOpa, CHECKED, 0)?;
        Ok(row)
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

    /// The slot a tapped row belongs to, for CLICKED events.
    pub(crate) fn row_slot(&self, handle: u16) -> Option<usize> {
        self.rows.iter().position(|r| r.0 == handle)
    }

    /// The slot a dragged slider belongs to, for VALUE_CHANGED events.
    pub(crate) fn slider_slot(&self, handle: u16) -> Option<usize> {
        self.sliders.iter().position(|s| s.0 == handle)
    }

    pub(crate) fn update(
        &mut self,
        b: &mut Batch,
        m: &Model,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
    ) -> Result<(), LvError> {
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

            // A rename or a new kind can swap the glyph. The old icon may already be gone
            // after a failed batch, so deleting is tolerant and the swap retries cleanly.
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
            b.value(self.sliders[i], button.value.min(100) as i32, false)?;
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
                self.sliders[i],
                self.inactive[i],
                dimmer,
                button.available,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Effect, KIND_DIMMER, NO_FAVORITE};
    use wp2_core::lvcmd::{self, Table};

    fn validate_and_apply(b: &mut Batch, table: &mut Table) -> Result<(), LvError> {
        assert_eq!(lvcmd::validate(b.bytes(), table, 1 << stock::FONT), Ok(()));
        for record in lvcmd::records(b.bytes()) {
            lvcmd::apply(table, &record.unwrap());
        }
        b.clear();
        Ok(())
    }

    fn ten_slots() -> Model {
        let mut m = Model::new();
        let mut v = vec![b'H', b'1', b'V', 5];
        for (name, kind, en) in [("Strip", KIND_DIMMER, 1u8), ("Porch", 0, 1), ("Bed", KIND_DIMMER, 1), ("TV", 0, 1), ("Heater", 0, 0)] {
            v.extend([en, kind, name.len() as u8]);
            v.extend_from_slice(name.as_bytes());
        }
        assert_eq!(m.apply(&v), Effect::Linked);
        m
    }

    #[test]
    fn menu_build_state_updates_and_error_paths_validate() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        stock::font(&mut b).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &Table::new(), 0), Ok(()));
        b.clear();

        let mut model = Model::new();
        // v3 phone order: V, J, K, U
        let mut v = vec![b'H', b'1', b'V', 5];
        for (name, kind, en) in [("Strip", KIND_DIMMER, 1u8), ("Porch", 0, 1), ("Bed", KIND_DIMMER, 1), ("TV", 0, 1), ("Heater", 0, 0)] {
            v.extend([en, kind, name.len() as u8]);
            v.extend_from_slice(name.as_bytes());
        }
        model.apply(&v);
        let mut u = vec![b'H', b'1', b'U', 3];
        for (name, kind, en) in [("Lamp", KIND_DIMMER, 1u8), ("Fan", 0, 1), ("Desk", 0, 1)] {
            u.extend([en, kind, name.len() as u8]);
            u.extend_from_slice(name.as_bytes());
        }
        u.extend([0, 5, 7, 9, 0xFF]);
        assert_eq!(model.apply(&u), Effect::Configured);
        model.apply(b"H1J\x05\x04Room\x00\x00\x00\x00");
        model.apply(b"H1K\x05\x00\x00\x04Room\x00\x00");

        let mut table = Table::new();
        let mut screen = Screen::build(&mut b, &model, Mode::Menu, &mut |batch| {
            validate_and_apply(batch, &mut table)
        })
        .unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();

        assert!(table.has(wp2_core::lvcmd::Class::ScrollBar));
        for i in 0..MAX_BUTTONS {
            assert_eq!(table.class(screen.rows[i].0), Some(wp2_core::lvcmd::Class::Obj));
            assert_eq!(table.class(screen.sliders[i].0), Some(wp2_core::lvcmd::Class::Slider));
            assert_eq!(table.class(screen.pills[i].0), Some(wp2_core::lvcmd::Class::Obj));
            assert_eq!(table.class(screen.inactive[i].0), Some(wp2_core::lvcmd::Class::Label));
        }
        // disabled slots show, then a config state arrives: on, value, error, disconnect
        assert_eq!(model.apply(b"H1S\x00\x01\x19\x00"), Effect::Changed);
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        assert_eq!(model.apply(b"H1S\x02\x00\x00\x01"), Effect::Changed);
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        model.mark_disconnected();
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        // an entity HA cannot reach flips to the inactive state without breaking the batch
        model.apply(b"H1S\x00\x00\x00\x00\x00");
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        // a rename swaps the icon; the batch must stay valid
        model.apply(b"H1C\x01\x01\x04Desk");
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
        // a kind change (toggle -> dimmer) morphs the row
        let mut u2 = vec![b'H', b'1', b'U', 1, 1, KIND_DIMMER, 4, b'D', b'e', b's', b'k'];
        u2.extend([0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        model.apply(&u2);
        screen.update(&mut b, &model, &mut |batch| validate_and_apply(batch, &mut table)).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
    }

    #[test]
    fn page_shows_only_favorites_and_headers_follow_visible_rows() {
        let mut m = ten_slots();
        m.apply(b"H1J\x05\x04East\x04East\x04West\x00\x00");
        m.apply(b"H1K\x05\x04East\x00\x04West\x00\x00");
        let mut u = vec![b'H', b'1', b'U', 4];
        for (name, kind, en) in [("A", KIND_DIMMER, 1u8), ("B", 0, 1), ("C", 0, 1), ("D", 0, 0)] {
            u.extend([en, kind, name.len() as u8]);
            u.extend_from_slice(name.as_bytes());
        }
        u.extend([0, 5, 7, 0xFF, 0xFF]);
        m.apply(&u);
        // page visibility: enabled && favorite
        assert!(visible(&m, 0, Mode::Page));
        assert!(!visible(&m, 1, Mode::Page));
        assert!(!visible(&m, 3, Mode::Page));
        assert!(visible(&m, 5, Mode::Page));
        assert!(visible(&m, 7, Mode::Page));
        // header on the first visible row of a section only (slot 7 carries "West")
        assert_eq!(header_for(&m, 0, Mode::Page), "East");
        assert_eq!(header_for(&m, 1, Mode::Page), "");
        assert_eq!(header_for(&m, 7, Mode::Page), "West");
        // menu lists everything enabled
        assert!(visible(&m, 1, Mode::Menu));
        assert_eq!(header_for(&m, 1, Mode::Menu), ""); // same section as slot 0
        assert_eq!(header_for(&m, 5, Mode::Menu), "East"); // first visible row after an empty section
        assert_eq!(header_for(&m, 7, Mode::Menu), "West");
    }

    #[test]
    fn favorites_and_events_map_back_to_slots() {
        let mut m = Model::new();
        m.apply(b"H1C\x01\x01\x04Lamp");
        m.apply(b"H1G\x05\x00\x00\x00\x00\x00");
        assert_eq!(m.primary(), Some(0));
        assert!(m.favorites.iter().all(|&f| f == 0 || f == NO_FAVORITE));
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let mut flush = |batch: &mut Batch| {
            batch.clear();
            Ok(())
        };
        let build = Screen::build(&mut b, &m, Mode::Menu, &mut flush);
        let at = b.bytes().len();
        let mut s = build.unwrap_or_else(|e| panic!("build failed {:?} with {} bytes buffered", e, at));
        assert_eq!(s.row_slot(s.rows[3].0), Some(3));
        assert_eq!(s.slider_slot(s.sliders[7].0), Some(7));
        assert_eq!(s.row_slot(s.sliders[7].0), None);
        s.update(&mut b, &m, &mut flush).unwrap();
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
        assert_eq!(glyph_kind("LIGHTS"), Glyph::Bulb);
        assert_eq!(glyph_kind("Fan with Light"), Glyph::Bulb);
        assert_eq!(glyph_kind("Ceiling Fan"), Glyph::Fan);
        assert_eq!(glyph_kind("Main Switch"), Glyph::Switch);
        assert_eq!(glyph_kind("Clamp"), Glyph::Switch);
    }
}

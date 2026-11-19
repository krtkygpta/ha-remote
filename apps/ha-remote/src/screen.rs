use crate::proto::{MAX_BUTTONS, Model, NO_FAVORITE};
use cmf_wp2_app::lv::{
    Align, BUF, Batch, Class, CONTENT, Flag, LongMode, LvError, Obj, Prop,
};
use cmf_wp2_app::stock;

const ROW_BYTES: usize = 700;

/// Row height: the same full-size row as before; the compact look comes from the gaps.
const ROW_H: i32 = 110;
/// The flex gap between two rows of one section; the section header between groups adds the
/// separation, and an empty header is Hidden so it adds no gap at all.
const ROW_GAP: i32 = 2;
/// The control icon at a row's left edge, and where the name starts, right of it.
const ICON_SLOT: i32 = 80;
const ICON_X: i16 = 8;
const NAME_X: i16 = 104;
/// The name stops before the switch: the row's right pad (20), the pill (56) and a gap.
const NAME_W: i32 = stock::ROW_W - NAME_X as i32 - 84;

/// The toggle switch at a row's right edge: stock pill and knob colours.
const SWITCH_W: i32 = 56;
const SWITCH_H: i32 = 28;
const KNOB: i32 = 24;

pub(crate) struct Screen {
    phone_detail: Obj,
    headers: [Obj; MAX_BUTTONS],
    names: [Obj; MAX_BUTTONS],
    pills: [Obj; MAX_BUTTONS],
    knobs: [Obj; MAX_BUTTONS],
    /// The icon slot of each row, and the glyph it currently shows (a rename swaps it).
    icons: [Obj; MAX_BUTTONS],
    icon_kinds: [u8; MAX_BUTTONS],
    pub(crate) rows: [Obj; MAX_BUTTONS],
}

/// The text of the section header above row [i]: the slot's section when it differs from the
/// previous slot's, otherwise empty (the row continues the section above it).
fn header_text(m: &Model, i: usize) -> &str {
    let section = m.buttons[i].section_label();
    if !m.buttons[i].enabled
        || section.is_empty()
        || (i > 0 && m.buttons[i - 1].section_label() == section)
    {
        ""
    } else {
        section
    }
}

/// The glyph shown beside a row's name, picked from the name's words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Glyph {
    Bulb,
    Switch,
    Fan,
    Phone,
}

/// Words that make a control a light: the bulb glyph. Whole words only, so "Clamp" stays a
/// switch, with a tolerant trailing "s" (lights, lamps).
const LIGHT_WORDS: &[&str] = &["light", "lamp", "bulb", "chandelier", "sconce", "lantern"];

fn word_matches(word: &str, needles: &[&str]) -> bool {
    let bare = word
        .strip_suffix('s')
        .or_else(|| word.strip_suffix('S'))
        .unwrap_or(word);
    needles
        .iter()
        .any(|n| word.eq_ignore_ascii_case(n) || bare.eq_ignore_ascii_case(n))
}

fn words_of(name: &str) -> impl Iterator<Item = &str> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
}

/// A light word -> bulb, "switch" -> switch, "fan" -> fan; anything else is a plain switch,
/// the generic toggle. A light word wins, so "Fan with Light" gets the bulb.
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

/// The row name with a "…" prefix while its toggle is in flight, "!" after an error.
fn write_label<'a>(
    m: &'a Model,
    i: usize,
    buf: &'a mut [u8; crate::proto::NAME_MAX + 2],
) -> &'a str {
    let label = m.buttons[i].label();
    let prefix: &[u8] = if m.pending == Some(i as u8) {
        b"\xe2\x80\xa6 "
    } else if m.error.is_some() {
        b"! "
    } else {
        b""
    };
    let n = (prefix.len() + label.len()).min(buf.len());
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..n].copy_from_slice(&label.as_bytes()[..n - prefix.len()]);
    core::str::from_utf8(&buf[..n]).unwrap_or(label)
}

/// One solid shape of a glyph; it cannot be pressed, so a press goes to the row.
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

/// One curved blade of the fan: a white arc stroke, `rotation` degrees around the hub.
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

/// The glyph pieces inside an icon slot.
fn draw_glyph(b: &mut Batch, slot: Obj, kind: Glyph) -> Result<(), LvError> {
    match kind {
        Glyph::Bulb => {
            // Material Symbols "lightbulb" (rounded): a round glass over two base bars.
            glyph_piece(b, slot, 46, 46, 0, -11, stock::CIRCLE, stock::WHITE)?; // the glass
            glyph_piece(b, slot, 20, 9, 0, 14, 4, stock::WHITE)?; // the base
            glyph_piece(b, slot, 12, 8, 0, 25, 3, stock::WHITE)?; // the base tip
        }
        Glyph::Switch => {
            // Material Symbols "toggle_on" (rounded): a track with the knob at its end.
            glyph_piece(b, slot, 48, 28, 0, 0, 14, stock::WHITE)?; // the track
            glyph_piece(b, slot, 20, 20, 10, 0, stock::CIRCLE, stock::BLACK)?; // the knob
        }
        Glyph::Fan => {
            // Material Symbols "mode_fan" (rounded): a hub with four curved blades.
            glyph_piece(b, slot, 18, 18, 0, 0, stock::CIRCLE, stock::WHITE)?; // the hub
            glyph_blade(b, slot, 0)?;
            glyph_blade(b, slot, 90)?;
            glyph_blade(b, slot, 180)?;
            glyph_blade(b, slot, 270)?;
        }
        Glyph::Phone => {
            glyph_piece(b, slot, 30, 48, 0, 0, 9, stock::WHITE)?; // the body
            glyph_piece(b, slot, 22, 34, 0, -3, 5, stock::BLACK)?; // the screen
            glyph_piece(b, slot, 6, 6, 0, 16, stock::CIRCLE, stock::WHITE)?; // the home dot
        }
    }
    Ok(())
}

/// The transparent icon slot at a row's left edge, with its glyph inside.
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
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
    ) -> Result<Self, LvError> {
        stock::theme(b)?;
        // The stock app list's fisheye: the loader slides every child right with its distance
        // from the centre line and fades its text, so rows travel along the screen's arc
        // while the list scrolls. (The Exercise-list mode narrows the cards instead, which
        // reads as a teardrop rather than an arc.) The fixed left pad keeps the column
        // itself centred.
        let (page, _bar) = stock::focus_page(b, 0)?;
        b.style(page, Prop::PadRow, 0, ROW_GAP)?;
        // The title scrolls with the list as its first row, so nothing is ever clipped
        // behind a fixed band: the fisheye slides and fades it out at the top edge. The
        // small top pad keeps it near the top at rest, like the stock Settings list.
        b.style(page, Prop::PadTop, 0, 40)?;
        let title = b.create(Class::Label, page)?;
        b.size(title, stock::ROW_W, 72)?;
        // White: the watch has a single system font, so there is no bold face to switch to.
        b.style(title, Prop::TextColor, 0, stock::WHITE)?;
        b.style(title, Prop::TextAlign, 0, 2)?;
        b.text(title, "Home Assistant")?;

        Self::room(b, flush)?;
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
        let mut rows = [Obj(0); MAX_BUTTONS];
        let mut icons = [Obj(0); MAX_BUTTONS];
        let mut icon_kinds = [0u8; MAX_BUTTONS];
        for i in 0..MAX_BUTTONS {
            Self::room(b, flush)?;
            let header = b.create(Class::Label, page)?;
            let text = header_text(m, i);
            b.size(header, stock::ROW_W, CONTENT)?;
            b.style(header, Prop::TextAlign, 0, 2)?;
            b.style(header, Prop::TextColor, 0, stock::GREY)?;
            b.text(header, text)?;
            if text.is_empty() {
                // A hidden child is skipped by the flex layout, so it adds no gap either:
                // rows of one section sit exactly ROW_GAP apart.
                b.flags(header, &[Flag::Hidden], &[])?;
            }
            headers[i] = header;

            let row = Self::row_base(b, page)?;
            rows[i] = row;
            let button = &m.buttons[i];
            // Unconfigured controls do not appear in the list at all.
            if !button.enabled {
                b.flags(row, &[Flag::Hidden], &[])?;
            }
            let name = if button.enabled {
                button.label()
            } else {
                "Not configured"
            };
            // The icon comes from the name's words and travels with it: left-aligned text
            // beside a fixed icon stays balanced while the fisheye slides the row.
            let kind = glyph_kind(button.label());
            icons[i] = draw_icon(b, row, kind)?;
            icon_kinds[i] = kind as u8;
            let name_label = b.create(Class::Label, row)?;
            b.size(name_label, NAME_W, CONTENT)?;
            b.long_mode(name_label, LongMode::Dot)?;
            b.align(name_label, Align::LeftMid, NAME_X, 0)?;
            b.text(name_label, name)?;
            names[i] = name_label;

            // A switch built from stock pill and knob colours; the loader has no switch class,
            // so the row stays the tap target and the switch animates with state.
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
        }
        Ok(Self {
            phone_detail: Obj(phone_detail.0),
            headers,
            names,
            pills,
            knobs,
            icons,
            icon_kinds,
            rows,
        })
    }

    /// A tappable list row: transparent like the stock app list's, so the icon and the name
    /// ride the fisheye arc with no card behind any of them.
    fn row_base(b: &mut Batch, parent: Obj) -> Result<Obj, LvError> {
        let row = stock::focus_row(b, parent)?;
        b.size(row, stock::ROW_W, ROW_H)?;
        Ok(row)
    }

    fn room(
        b: &mut Batch,
        flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>,
    ) -> Result<(), LvError> {
        if b.bytes().len() + ROW_BYTES > BUF {
            flush(b)?;
        }
        Ok(())
    }

    fn phone_status(m: &Model) -> &'static str {
        if !m.connected {
            "Offline"
        } else if m.error.is_some() {
            "Error"
        } else if m.pending.is_some() {
            "Busy"
        } else {
            "Online"
        }
    }

    pub(crate) fn update(&mut self, b: &mut Batch, m: &Model) -> Result<(), LvError> {
        b.text(self.phone_detail, Self::phone_status(m))?;
        for i in 0..MAX_BUTTONS {
            let text = header_text(m, i);
            b.text(self.headers[i], text)?;
            if text.is_empty() {
                b.flags(self.headers[i], &[Flag::Hidden], &[])?;
            } else {
                b.flags(self.headers[i], &[], &[Flag::Hidden])?;
            }
            let button = &m.buttons[i];
            if button.enabled {
                b.flags(self.rows[i], &[], &[Flag::Hidden])?;
            } else {
                b.flags(self.rows[i], &[Flag::Hidden], &[])?;
            }
            // A renamed control swaps its glyph; the name itself updates below either way.
            let kind = glyph_kind(button.label());
            if kind as u8 != self.icon_kinds[i] {
                b.del(self.icons[i])?;
                self.icons[i] = draw_icon(b, self.rows[i], kind)?;
                self.icon_kinds[i] = kind as u8;
            }
            let mut buf = [0u8; crate::proto::NAME_MAX + 2];
            // Status lives in the name itself: "…" while the toggle is in flight, "!" after
            // an error.
            b.text(self.names[i], write_label(m, i, &mut buf))?;
            // Switch reflects state; the row itself is the tap target. White and grey only:
            // the watch's accent is orange and reads as a warning here.
            b.style(self.pills[i], Prop::BgColor, 0, if button.on { stock::WHITE } else { stock::GREY })?;
            b.style(self.knobs[i], Prop::BgColor, 0, if button.on { stock::BLACK } else { stock::WHITE })?;
            b.align(
                self.knobs[i],
                if button.on { Align::RightMid } else { Align::LeftMid },
                if button.on { -2 } else { 2 },
                0,
            )?;
        }
        Ok(())
    }
}

/// Unused today but keeps the favorite rule visible next to the widget code.
#[allow(dead_code)]
fn favorite_is(m: &Model, i: usize) -> bool {
    m.fav != NO_FAVORITE && m.fav as usize == i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Effect;
    use wp2_core::lvcmd::{self, Table};

    fn validate_and_apply(b: &mut Batch, table: &mut Table) -> Result<(), LvError> {
        assert_eq!(lvcmd::validate(b.bytes(), table, 1 << stock::FONT), Ok(()));
        for record in lvcmd::records(b.bytes()) {
            lvcmd::apply(table, &record.unwrap());
        }
        b.clear();
        Ok(())
    }

    #[test]
    fn compact_rows_batches_and_live_updates_validate() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        stock::font(&mut b).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &Table::new(), 0), Ok(()));
        b.clear();

        let mut model = Model::new();
        assert_eq!(model.apply(b"H1C\x02\x01\x04Desk\x01\x05Hall1"), Effect::Configured);
        assert_eq!(model.apply(b"H1G\x05\x04Room\x00\x00\x00\x00\xFF"), Effect::Configured);
        let mut table = Table::new();
        let mut flush = |batch: &mut Batch| validate_and_apply(batch, &mut table);
        let mut screen = Screen::build(&mut b, &model, &mut flush).unwrap();
        flush(&mut b).unwrap();
        drop(flush);

        assert!(table.has(wp2_core::lvcmd::Class::ScrollBar));
        for i in 0..MAX_BUTTONS {
            let row = screen.rows[i];
            assert_eq!(table.class(row.0), Some(wp2_core::lvcmd::Class::Obj));
            assert_eq!(table.class(screen.names[i].0), Some(wp2_core::lvcmd::Class::Label));
            assert_eq!(table.class(screen.pills[i].0), Some(wp2_core::lvcmd::Class::Obj));
            assert_eq!(table.class(screen.headers[i].0), Some(wp2_core::lvcmd::Class::Label));
        }

        model.apply(b"H1S\x00\x01\x00");
        screen.update(&mut b, &model).unwrap();
        validate_and_apply(&mut b, &mut table).unwrap();
    }

    #[test]
    fn section_headers_show_only_at_group_starts() {
        let mut m = Model::new();
        m.apply(b"H1C\x03\x01\x02A1\x01\x02A2\x01\x02B1");
        m.apply(b"H1G\x05\x04East\x04East\x04West\x00\x00\xFF");
        assert_eq!(header_text(&m, 0), "East");
        assert_eq!(header_text(&m, 1), "");
        assert_eq!(header_text(&m, 2), "West");
    }

    #[test]
    fn control_names_pick_their_icon() {
        assert!(word_matches("LIGHTS", LIGHT_WORDS));
        assert!(word_matches("Lamps", LIGHT_WORDS));
        assert!(words_of("Living Room Light").any(|w| word_matches(w, LIGHT_WORDS)));
        assert_eq!(glyph_kind("Living Room Light"), Glyph::Bulb);
        assert_eq!(glyph_kind("LIGHTS"), Glyph::Bulb);
        assert_eq!(glyph_kind("Bedroom Lamp"), Glyph::Bulb);
        assert_eq!(glyph_kind("Chandelier"), Glyph::Bulb);
        assert_eq!(glyph_kind("Fan with Light"), Glyph::Bulb);
        assert_eq!(glyph_kind("Ceiling Fan"), Glyph::Fan);
        assert_eq!(glyph_kind("Main Switch"), Glyph::Switch);
        assert_eq!(glyph_kind("Clamp"), Glyph::Switch);
        assert_eq!(glyph_kind("TV Plug"), Glyph::Switch);
    }

}


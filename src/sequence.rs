//! Mermaid sequence diagrams (```` ```mermaid ```` blocks starting with `sequenceDiagram`):
//! read, laid out with a column for each participant and a row for each message, note and
//! frame line, and drawn. Light on purpose: no activation bars, no boxes round participants.

use gtk::{gdk, glib, graphene, gsk, pango, prelude::*};

use crate::flowchart::Rect;
use crate::theme;

#[derive(Debug, PartialEq)]
pub struct Participant {
    pub id: String,
    pub label: String,
    /// An actor (drawn as a figure) rather than a box.
    pub actor: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Line {
    Solid,
    Dotted,
}

/// What a message's arrow ends in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Head {
    None,
    Arrow,
    Cross,
    /// `-)`: an open arrowhead, for a message that doesn't wait.
    Open,
}

/// Where a note goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    Left(usize),
    Right(usize),
    Over(usize, usize),
}

#[derive(Debug, PartialEq)]
pub enum Step {
    /// A message, between two participants (indices) or to oneself; `both`: arrowheads at both
    /// ends.
    Message { from: usize, to: usize, text: String, line: Line, head: Head, both: bool },
    Note { side: Side, text: String },
    /// A frame starts: `loop`, `alt`, `opt`, `par`, `critical`, `break` or `rect`, with its label.
    Start { kind: String, label: String },
    /// Its next part (`else`, `and`, `option`), with its label.
    Part { label: String },
    End,
}

#[derive(Debug, PartialEq)]
pub struct Diagram {
    pub participants: Vec<Participant>,
    pub steps: Vec<Step>,
    /// `autonumber`: the messages are numbered.
    pub numbered: bool,
}

/// The sequence diagram in a Mermaid block, or nothing when it is another kind of diagram.
pub fn parse(source: &str) -> Option<Diagram> {
    let mut lines = source.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("%%"));
    if lines.next()? != "sequenceDiagram" {
        return None;
    }
    let mut diagram = Diagram { participants: Vec::new(), steps: Vec::new(), numbered: false };
    // What each `end` closes: a frame, or a box round participants (nothing to draw).
    let mut open: Vec<bool> = Vec::new();
    for line in lines {
        let (word, rest) = first_word(line);
        match word {
            "participant" | "actor" => diagram.declare(rest, word == "actor"),
            "create" => {
                let (word, rest) = first_word(rest);
                if matches!(word, "participant" | "actor") {
                    diagram.declare(rest, word == "actor");
                }
            }
            "autonumber" => diagram.numbered = true,
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" => {
                open.push(true);
                // A rect's words are its colour.
                let label = if word == "rect" { String::new() } else { words(rest) };
                diagram.steps.push(Step::Start { kind: word.to_owned(), label });
            }
            "else" | "and" | "option" => diagram.steps.push(Step::Part { label: words(rest) }),
            "box" => open.push(false),
            "end" => {
                if open.pop() == Some(true) {
                    diagram.steps.push(Step::End);
                }
            }
            "activate" | "deactivate" | "destroy" | "title" | "link" | "links" | "properties" | "details" => {}
            _ if word.eq_ignore_ascii_case("note") => diagram.note(rest),
            _ => diagram.message(line),
        }
    }
    Some(diagram)
}

/// A line's first word, and the rest.
fn first_word(line: &str) -> (&str, &str) {
    line.split_once(char::is_whitespace).map_or((line, ""), |(word, rest)| (word, rest.trim()))
}

/// What a message or a note says: `<br>` breaks a line.
fn words(text: &str) -> String {
    text.trim().replace("<br/>", "\n").replace("<br />", "\n").replace("<br>", "\n")
}

/// The arrows, longest first where one starts another: their line, their head, and whether
/// both ends have one.
const ARROWS: [(&str, Line, Head, bool); 10] = [
    ("<<-->>", Line::Dotted, Head::Arrow, true),
    ("<<->>", Line::Solid, Head::Arrow, true),
    ("-->>", Line::Dotted, Head::Arrow, false),
    ("->>", Line::Solid, Head::Arrow, false),
    ("--x", Line::Dotted, Head::Cross, false),
    ("-x", Line::Solid, Head::Cross, false),
    ("--)", Line::Dotted, Head::Open, false),
    ("-)", Line::Solid, Head::Open, false),
    ("-->", Line::Dotted, Head::None, false),
    ("->", Line::Solid, Head::None, false),
];

impl Diagram {
    /// The participant called `id`: one named already, or a new one at the end.
    fn index(&mut self, id: &str) -> usize {
        if let Some(index) = self.participants.iter().position(|participant| participant.id == id) {
            return index;
        }
        self.participants.push(Participant { id: id.to_owned(), label: id.to_owned(), actor: false });
        self.participants.len() - 1
    }

    /// `A`, or `A as Alice`.
    fn declare(&mut self, rest: &str, actor: bool) {
        let (id, label) = rest.split_once(" as ").map_or((rest, rest), |(id, label)| (id.trim(), label.trim()));
        if id.is_empty() {
            return;
        }
        let index = self.index(id);
        self.participants[index].label = words(label);
        self.participants[index].actor = actor;
    }

    /// `right of A: …`, `left of A: …`, `over A: …` or `over A,B: …`.
    fn note(&mut self, rest: &str) {
        let Some((place, text)) = rest.split_once(':') else { return };
        let place = place.trim();
        let side = if let Some(who) = place.strip_prefix("right of") {
            Side::Right(self.index(who.trim()))
        } else if let Some(who) = place.strip_prefix("left of") {
            Side::Left(self.index(who.trim()))
        } else if let Some(who) = place.strip_prefix("over") {
            match who.split_once(',') {
                Some((first, second)) => Side::Over(self.index(first.trim()), self.index(second.trim())),
                None => {
                    let index = self.index(who.trim());
                    Side::Over(index, index)
                }
            }
        } else {
            return;
        };
        self.steps.push(Step::Note { side, text: words(text) });
    }

    /// `A->>B: …`, with any of the arrows; `+` or `-` after the arrow (switching a participant
    /// on or off) is left out.
    fn message(&mut self, line: &str) {
        let Some(at) = line.find(['-', '<']) else { return };
        let Some(&(arrow, kind, head, both)) = ARROWS.iter().find(|(arrow, ..)| line[at..].starts_with(arrow)) else { return };
        let rest = line[at + arrow.len()..].trim_start().trim_start_matches(['+', '-']);
        let (to, text) = rest.split_once(':').unwrap_or((rest, ""));
        let (from, to) = (line[..at].trim(), to.trim());
        if from.is_empty() || to.is_empty() {
            return;
        }
        let (from, to) = (self.index(from), self.index(to));
        self.steps.push(Step::Message { from, to, text: words(text), line: kind, head, both });
    }
}

/// Where everything goes, in pixels from the top left.
#[derive(Debug)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    /// Each participant's lifeline (where it runs across), and its box at the top and the bottom.
    pub columns: Vec<f64>,
    pub tops: Vec<Rect>,
    pub bottoms: Vec<Rect>,
    /// Each step's place, and the row it takes (its top and bottom: what the tests check the
    /// order of; drawing needs only the places).
    pub spots: Vec<Spot>,
    #[allow(dead_code)]
    pub rows: Vec<(f64, f64)>,
}

#[derive(Debug)]
pub enum Spot {
    /// A message's arrow: how far down, where it leaves and where it arrives (the same for one
    /// to oneself, which loops out to the right), and where its words are centred.
    Arrow { y: f64, from: f64, to: f64, words: (f64, f64) },
    Note(Rect),
    /// A frame, from its start to its end; its words at its top left.
    Frame(Rect),
    /// A frame's next part: the dashed line across it, its words under the line.
    Part { y: f64, from: f64, to: f64 },
    End,
}

/// Room round a participant's name and a note's words; round the diagram; between
/// participants' boxes at least.
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 9.0;
const MARGIN: f64 = 8.0;
const GAP: f64 = 36.0;
/// Room each side of a message's words, between its participants' lifelines.
const WORDS_PAD: f64 = 16.0;
/// How far a message to oneself loops out to the right, and how far down it comes back.
const LOOP: f64 = 28.0;
const LOOP_DOWN: f64 = 20.0;
/// A note's distance from its lifeline; a frame's from what is in it.
const NOTE_OFF: f64 = 10.0;
const FRAME_PAD: f64 = 12.0;

/// Lays `diagram` out, given the size of each participant's name and of each step's words
/// ((0, 0) for none; a frame's start: its kind and label together), as (width, height).
pub fn layout(diagram: &Diagram, names: &[(f64, f64)], words: &[(f64, f64)]) -> Layout {
    let count = diagram.participants.len();
    let size = |i: usize| names.get(i).map_or((2.0 * PAD_X, 2.0 * PAD_Y), |&(w, h)| (w + 2.0 * PAD_X, h + 2.0 * PAD_Y));
    let word = |i: usize| words.get(i).copied().unwrap_or((0.0, 0.0));
    // Columns: each box beside the one before, then apart wherever what goes between two
    // lifelines needs the room.
    let mut columns = Vec::with_capacity(count);
    let mut x = 0.0;
    for i in 0..count {
        columns.push(x + size(i).0 / 2.0);
        x += size(i).0 + GAP;
    }
    for (index, step) in diagram.steps.iter().enumerate() {
        let (width, _) = word(index);
        match *step {
            Step::Message { from, to, .. } if from != to => apart(&mut columns, from.min(to), from.max(to), width + 2.0 * WORDS_PAD),
            // To oneself: the loop and its words, before the next lifeline.
            Step::Message { from, .. } if from + 1 < count => apart(&mut columns, from, from + 1, LOOP + width + 2.0 * WORDS_PAD),
            Step::Note { side: Side::Right(i), .. } if i + 1 < count => apart(&mut columns, i, i + 1, width + 2.0 * (PAD_X + NOTE_OFF)),
            Step::Note { side: Side::Left(i), .. } if i > 0 => apart(&mut columns, i - 1, i, width + 2.0 * (PAD_X + NOTE_OFF)),
            _ => {}
        }
    }

    // Rows, down the page under the boxes. Each step also reaches somewhere across, for the
    // frames round it.
    let tall = (0..count).map(|i| size(i).1).fold(0.0, f64::max);
    let mut y = MARGIN + tall + 18.0;
    let mut spots: Vec<Spot> = Vec::new();
    let mut rows: Vec<(f64, f64)> = Vec::new();
    let mut reaches: Vec<Option<(f64, f64)>> = Vec::new();
    // Frames open: the step each starts at, and its parts.
    let mut frames: Vec<(usize, Vec<usize>)> = Vec::new();
    for (index, step) in diagram.steps.iter().enumerate() {
        let (width, height) = word(index);
        let top = y;
        let (spot, reach) = match *step {
            Step::Message { from, to, .. } if from != to => {
                let arrow = top + height + 6.0;
                y = arrow + 14.0;
                let middle = (columns[from] + columns[to]) / 2.0;
                let reach = (columns[from].min(columns[to]).min(middle - width / 2.0), columns[from].max(columns[to]).max(middle + width / 2.0));
                (Spot::Arrow { y: arrow, from: columns[from], to: columns[to], words: (middle, top + height / 2.0) }, Some(reach))
            }
            Step::Message { from, .. } => {
                let arrow = top + 4.0;
                y = arrow + LOOP_DOWN.max(height) + 14.0;
                let words = (columns[from] + LOOP + WORDS_PAD / 2.0 + width / 2.0, arrow + LOOP_DOWN / 2.0);
                (Spot::Arrow { y: arrow, from: columns[from], to: columns[from], words }, Some((columns[from], words.0 + width / 2.0)))
            }
            Step::Note { side, .. } => {
                let (wide, high) = (width + 2.0 * PAD_X, height + 2.0 * PAD_Y);
                let (x, wide) = match side {
                    Side::Right(i) => (columns[i] + NOTE_OFF, wide),
                    Side::Left(i) => (columns[i] - NOTE_OFF - wide, wide),
                    Side::Over(a, b) => {
                        let (i, j) = (a.min(b), a.max(b));
                        let wide = wide.max(columns[j] - columns[i] + 2.0 * NOTE_OFF);
                        ((columns[i] + columns[j]) / 2.0 - wide / 2.0, wide)
                    }
                };
                y = top + high + 12.0;
                (Spot::Note(Rect { x, y: top, width: wide, height: high }), Some((x, x + wide)))
            }
            Step::Start { .. } => {
                frames.push((index, Vec::new()));
                y = top + height.max(14.0) + 16.0;
                // How wide and how tall: known at its end.
                (Spot::Frame(Rect { x: 0.0, y: top, width: 0.0, height: 0.0 }), None)
            }
            Step::Part { .. } => {
                if let Some((_, parts)) = frames.last_mut() {
                    parts.push(index);
                }
                y = top + height.max(14.0) + 16.0;
                (Spot::Part { y: top, from: 0.0, to: 0.0 }, None)
            }
            Step::End => {
                if let Some((start, parts)) = frames.pop() {
                    // Round what is in it (all of the diagram if nothing), wide enough for its words.
                    let (mut left, mut right) = reaches[start + 1..].iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, r): (f64, f64), &(a, b)| (l.min(a), r.max(b)));
                    if !left.is_finite() {
                        (left, right) = (columns.first().copied().unwrap_or(0.0), columns.last().copied().unwrap_or(0.0));
                    }
                    let left = left - FRAME_PAD;
                    let right = (right + FRAME_PAD).max(left + word(start).0 + 2.0 * PAD_X);
                    spots[start] = Spot::Frame(Rect { x: left, y: rows[start].0, width: right - left, height: top + 4.0 - rows[start].0 });
                    reaches[start] = Some((left, right));
                    for part in parts {
                        if let Spot::Part { y, .. } = spots[part] {
                            spots[part] = Spot::Part { y, from: left, to: right };
                        }
                    }
                }
                y = top + 16.0;
                (Spot::End, None)
            }
        };
        spots.push(spot);
        rows.push((top, y));
        reaches.push(reach);
    }

    // The boxes again at the bottom; then all of it moved right so that nothing sticks out on
    // the left.
    let bottom = y + 6.0;
    let place = |i: usize, y: f64| Rect { x: columns[i] - size(i).0 / 2.0, y, width: size(i).0, height: tall };
    let mut tops: Vec<Rect> = (0..count).map(|i| place(i, MARGIN)).collect();
    let mut bottoms: Vec<Rect> = (0..count).map(|i| place(i, bottom)).collect();
    let boxes = tops.iter().map(|rect| (rect.x, rect.x + rect.width));
    let (left, right) = boxes.chain(reaches.iter().flatten().copied()).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, r), (a, b)| (l.min(a), r.max(b)));
    let shift = if left.is_finite() { MARGIN - left } else { MARGIN };
    columns.iter_mut().for_each(|x| *x += shift);
    tops.iter_mut().chain(bottoms.iter_mut()).for_each(|rect| rect.x += shift);
    for spot in &mut spots {
        match spot {
            Spot::Arrow { from, to, words, .. } => {
                *from += shift;
                *to += shift;
                words.0 += shift;
            }
            Spot::Note(rect) | Spot::Frame(rect) => rect.x += shift,
            Spot::Part { from, to, .. } => {
                *from += shift;
                *to += shift;
            }
            Spot::End => {}
        }
    }
    let width = if right.is_finite() { right + shift + MARGIN } else { 2.0 * MARGIN };
    Layout { width, height: bottom + tall + MARGIN, columns, tops, bottoms, spots, rows }
}

/// Words wider than this wrap.
const WRAP: i32 = 260;
/// An arrowhead's length; an actor's figure's height (above the name); a number's circle.
const ARROW: f32 = 9.0;
const FIGURE: f64 = 30.0;
const NUMBER: f32 = 8.0;
/// Wider diagrams are shown smaller.
const WIDEST: f64 = 900.0;

/// The diagram drawn as a picture; `text` lays out words in the rendered view's font.
pub fn picture(diagram: &Diagram, text: &dyn Fn(&str) -> pango::Layout) -> gtk::Picture {
    let laid = |words: &str, markup: bool| {
        let layout = text("");
        if markup {
            layout.set_markup(words);
        } else {
            layout.set_text(words);
        }
        layout.set_alignment(pango::Alignment::Center);
        if layout.pixel_size().0 > WRAP {
            layout.set_wrap(pango::WrapMode::WordChar);
            layout.set_width(WRAP * pango::SCALE);
        }
        let (_, extents) = layout.pixel_extents();
        (layout, extents)
    };
    let names: Vec<_> = diagram.participants.iter().map(|participant| laid(&participant.label, false)).collect();
    let words: Vec<Option<_>> = diagram
        .steps
        .iter()
        .map(|step| match step {
            Step::Message { text, .. } | Step::Note { text, .. } if !text.is_empty() => Some(laid(text, false)),
            Step::Start { kind, label } => Some(laid(&format!("<b>{kind}</b>  {}", glib::markup_escape_text(label)), true)),
            Step::Part { label } if !label.is_empty() => Some(laid(&format!("[{label}]"), false)),
            _ => None,
        })
        .collect();
    let extent = |extents: &pango::Rectangle| (f64::from(extents.width()), f64::from(extents.height()));
    // An actor's figure stands over its name.
    let name_sizes: Vec<(f64, f64)> = diagram
        .participants
        .iter()
        .zip(&names)
        .map(|(participant, (_, extents))| {
            let (width, height) = extent(extents);
            (width, if participant.actor { height + FIGURE } else { height })
        })
        .collect();
    let word_sizes: Vec<(f64, f64)> = words.iter().map(|words| words.as_ref().map_or((0.0, 0.0), |(_, extents)| extent(extents))).collect();
    let laid = layout(diagram, &name_sizes, &word_sizes);

    let colors = theme::colors();
    let color = |rgb: crate::color::Rgb, alpha: f32| gdk::RGBA::new(rgb.r as f32, rgb.g as f32, rgb.b as f32, alpha);
    let (line, border, fill, ink, ground) =
        (color(colors.dim, 1.0), color(colors.marker, 1.0), color(colors.quote_background, 1.0), color(colors.text, 1.0), color(colors.background, 1.0));
    let snapshot = gtk::Snapshot::new();
    // Words centred on `(x, y)` (or with their top left there).
    let write = |(layout, extents): &(pango::Layout, pango::Rectangle), (x, y): (f64, f64), centred: bool, ink: &gdk::RGBA| {
        let (left, top) = if centred {
            (x - f64::from(extents.width()) / 2.0, y - f64::from(extents.height()) / 2.0)
        } else {
            (x, y)
        };
        snapshot.save();
        snapshot.translate(&graphene::Point::new((left - f64::from(extents.x())) as f32, (top - f64::from(extents.y())) as f32));
        snapshot.append_layout(layout, ink);
        snapshot.restore();
    };
    let rect = |rect: &Rect| graphene::Rect::new(rect.x as f32, rect.y as f32, rect.width as f32, rect.height as f32);
    let dashed = |width: f32| {
        let stroke = gsk::Stroke::new(width);
        stroke.set_dash(&[3.0, 4.0]);
        stroke
    };

    // Frames first, under everything; a rect is a patch of colour.
    for (step, (spot, words)) in diagram.steps.iter().zip(laid.spots.iter().zip(&words)) {
        match (step, spot) {
            (Step::Start { kind, .. }, Spot::Frame(frame)) if kind == "rect" => {
                snapshot.append_color(&color(colors.quote_background, 0.6), &rect(frame));
            }
            (Step::Start { .. }, Spot::Frame(frame)) => {
                let outline = gsk::RoundedRect::from_rect(rect(frame), 4.0);
                snapshot.append_border(&outline, &[1.0; 4], &[border; 4]);
                if let Some(words) = words {
                    write(words, (frame.x + 8.0, frame.y + 4.0), false, &ink);
                }
            }
            (_, Spot::Part { y, from, to }) => {
                let path = gsk::PathBuilder::new();
                path.move_to(*from as f32, *y as f32);
                path.line_to(*to as f32, *y as f32);
                snapshot.append_stroke(&path.to_path(), &dashed(1.0), &border);
                if let Some(words) = words {
                    write(words, (from + 8.0, y + 4.0), false, &ink);
                }
            }
            _ => {}
        }
    }
    // Lifelines, from the boxes at the top to those at the bottom.
    for (x, (top, bottom)) in laid.columns.iter().zip(laid.tops.iter().zip(&laid.bottoms)) {
        let path = gsk::PathBuilder::new();
        path.move_to(*x as f32, (top.y + top.height) as f32);
        path.line_to(*x as f32, bottom.y as f32);
        snapshot.append_stroke(&path.to_path(), &dashed(1.0), &line);
    }
    // Messages and notes.
    let mut number = 0;
    for (step, (spot, words)) in diagram.steps.iter().zip(laid.spots.iter().zip(&words)) {
        match (step, spot) {
            (Step::Message { line: kind, head, both, .. }, Spot::Arrow { y, from, to, words: middle }) => {
                draw_message(&snapshot, *y as f32, *from as f32, *to as f32, *kind, *head, *both, &line);
                if let Some(words) = words {
                    // On a patch of the page: an arrow may cross other lifelines.
                    let (w, h) = (words.1.width() as f32 + 8.0, words.1.height() as f32 + 2.0);
                    let patch = graphene::Rect::new(middle.0 as f32 - w / 2.0, middle.1 as f32 - h / 2.0, w, h);
                    snapshot.append_color(&ground, &patch);
                    write(words, *middle, true, &ink);
                }
                if diagram.numbered {
                    number += 1;
                    draw_number(&snapshot, number, (*from as f32, *y as f32), text, &color(colors.accent, 1.0), &ground);
                }
            }
            (Step::Note { .. }, Spot::Note(note)) => {
                let outline = gsk::RoundedRect::from_rect(rect(note), 3.0);
                snapshot.push_rounded_clip(&outline);
                snapshot.append_color(&ground, &rect(note));
                snapshot.append_color(&color(colors.changed, 0.16), &rect(note));
                snapshot.pop();
                snapshot.append_border(&outline, &[1.0; 4], &[border; 4]);
                if let Some(words) = words {
                    write(words, (note.x + note.width / 2.0, note.y + note.height / 2.0), true, &ink);
                }
            }
            _ => {}
        }
    }
    // The participants, at the top and at the bottom: boxes, or figures for actors.
    for ((participant, name), (top, bottom)) in diagram.participants.iter().zip(&names).zip(laid.tops.iter().zip(&laid.bottoms)) {
        for at in [top, bottom] {
            if participant.actor {
                draw_figure(&snapshot, (at.x + at.width / 2.0) as f32, at.y as f32 + 2.0, &border);
                write(name, (at.x + at.width / 2.0, at.y + FIGURE + (at.height - FIGURE) / 2.0), true, &ink);
            } else {
                let outline = gsk::RoundedRect::from_rect(rect(at), 4.0);
                snapshot.push_rounded_clip(&outline);
                snapshot.append_color(&fill, &rect(at));
                snapshot.pop();
                snapshot.append_border(&outline, &[1.2; 4], &[border; 4]);
                write(name, (at.x + at.width / 2.0, at.y + at.height / 2.0), true, &ink);
            }
        }
    }

    let picture = match snapshot.to_paintable(Some(&graphene::Size::new(laid.width as f32, laid.height as f32))) {
        Some(paintable) => gtk::Picture::for_paintable(&paintable),
        None => gtk::Picture::new(),
    };
    picture.set_can_shrink(true);
    let width = laid.width.min(WIDEST);
    picture.set_size_request(width as i32, (laid.height * width / laid.width.max(1.0)) as i32);
    picture.set_alternative_text(Some("Sequence diagram"));
    picture
}

/// A message's arrow at height `y`, from one lifeline to another, or out to the right and
/// back for one to oneself.
#[allow(clippy::too_many_arguments)]
fn draw_message(snapshot: &gtk::Snapshot, y: f32, from: f32, to: f32, line: Line, head: Head, both: bool, color: &gdk::RGBA) {
    let stroke = gsk::Stroke::new(1.4);
    stroke.set_line_join(gsk::LineJoin::Round);
    if line == Line::Dotted {
        stroke.set_dash(&[3.0, 4.0]);
    }
    let path = gsk::PathBuilder::new();
    // A filled head ends the line at its base.
    let short = |head: Head| if head == Head::Arrow { ARROW } else { 0.0 };
    if from == to {
        let (out, down) = (from + LOOP as f32, y + LOOP_DOWN as f32);
        path.move_to(from, y);
        path.line_to(out, y);
        path.line_to(out, down);
        path.line_to(from + short(head), down);
        snapshot.append_stroke(&path.to_path(), &stroke, color);
        draw_head(snapshot, head, (from, down), (out, down), color);
        return;
    }
    let way = (to - from).signum();
    path.move_to(if both { from + way * short(head) } else { from }, y);
    path.line_to(to - way * short(head), y);
    snapshot.append_stroke(&path.to_path(), &stroke, color);
    draw_head(snapshot, head, (to, y), (from, y), color);
    if both {
        draw_head(snapshot, head, (from, y), (to, y), color);
    }
}

/// An arrow's head at `tip`, coming from `from`.
fn draw_head(snapshot: &gtk::Snapshot, head: Head, tip: (f32, f32), from: (f32, f32), color: &gdk::RGBA) {
    let length = (tip.0 - from.0).hypot(tip.1 - from.1).max(0.001);
    let (dx, dy) = ((tip.0 - from.0) / length, (tip.1 - from.1) / length);
    let base = (tip.0 - dx * ARROW, tip.1 - dy * ARROW);
    let side = (-dy * ARROW / 2.0, dx * ARROW / 2.0);
    let path = gsk::PathBuilder::new();
    match head {
        Head::None => {}
        Head::Arrow => {
            path.move_to(tip.0, tip.1);
            path.line_to(base.0 + side.0, base.1 + side.1);
            path.line_to(base.0 - side.0, base.1 - side.1);
            path.close();
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, color);
        }
        Head::Open => {
            path.move_to(base.0 + side.0, base.1 + side.1);
            path.line_to(tip.0, tip.1);
            path.line_to(base.0 - side.0, base.1 - side.1);
            snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(1.4), color);
        }
        Head::Cross => {
            let (cx, cy, r) = (tip.0 - dx * 5.0, tip.1 - dy * 5.0, 4.0);
            path.move_to(cx - r, cy - r);
            path.line_to(cx + r, cy + r);
            path.move_to(cx + r, cy - r);
            path.line_to(cx - r, cy + r);
            snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(1.6), color);
        }
    }
}

/// `autonumber`'s number for a message, in a circle where its arrow starts.
fn draw_number(snapshot: &gtk::Snapshot, number: usize, (x, y): (f32, f32), text: &dyn Fn(&str) -> pango::Layout, color: &gdk::RGBA, ink: &gdk::RGBA) {
    let circle = gsk::PathBuilder::new();
    circle.add_circle(&graphene::Point::new(x, y), NUMBER);
    snapshot.append_fill(&circle.to_path(), gsk::FillRule::Winding, color);
    let layout = text(&number.to_string());
    if let Some(mut font) = layout.font_description().or_else(|| layout.context().font_description()) {
        font.set_size(font.size() * 3 / 4);
        layout.set_font_description(Some(&font));
    }
    let (_, extents) = layout.pixel_extents();
    snapshot.save();
    let (left, top) = (x - extents.width() as f32 / 2.0 - extents.x() as f32, y - extents.height() as f32 / 2.0 - extents.y() as f32);
    snapshot.translate(&graphene::Point::new(left, top));
    snapshot.append_layout(&layout, ink);
    snapshot.restore();
}

/// An actor: a figure, its head at `top`.
fn draw_figure(snapshot: &gtk::Snapshot, x: f32, top: f32, color: &gdk::RGBA) {
    let path = gsk::PathBuilder::new();
    path.add_circle(&graphene::Point::new(x, top + 5.0), 5.0);
    path.move_to(x, top + 10.0);
    path.line_to(x, top + 19.0);
    path.move_to(x - 8.0, top + 13.0);
    path.line_to(x + 8.0, top + 13.0);
    path.move_to(x - 6.0, top + 27.0);
    path.line_to(x, top + 19.0);
    path.line_to(x + 6.0, top + 27.0);
    snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(1.4), color);
}

/// Moves lifeline `j` (and those after it) along, if need be, to be `wide` from lifeline `i`.
fn apart(columns: &mut [f64], i: usize, j: usize, wide: f64) {
    let short = wide - (columns[j] - columns[i]);
    if short > 0.0 {
        columns[j..].iter_mut().for_each(|x| *x += short);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant(id: &str, label: &str, actor: bool) -> Participant {
        Participant { id: id.to_owned(), label: label.to_owned(), actor }
    }

    fn message(from: usize, to: usize, text: &str, line: Line, head: Head) -> Step {
        Step::Message { from, to, text: text.to_owned(), line, head, both: false }
    }

    #[test]
    fn the_first_line_says_sequence_diagram() {
        assert_eq!(parse("graph TD\nA-->B"), None);
        assert_eq!(parse("%% a comment\nsequenceDiagram"), Some(Diagram { participants: Vec::new(), steps: Vec::new(), numbered: false }));
    }

    #[test]
    fn participants_come_as_declared_then_as_messages_name_them() {
        let diagram = parse("sequenceDiagram\nparticipant A as Alice\nactor B\nC->>A: hi\nparticipant D as Dave Smith").unwrap();
        assert_eq!(
            diagram.participants,
            vec![participant("A", "Alice", false), participant("B", "B", true), participant("C", "C", false), participant("D", "Dave Smith", false)]
        );
    }

    #[test]
    fn messages_carry_their_line_head_and_words() {
        let diagram = parse("sequenceDiagram\nA->>B: hi\nB-->>A: back<br>again\nA-xB: lost\nA--)B: later\nA->B: plain\nA->>+B: on\nB-->>-A:\nA<<->>B: both").unwrap();
        assert_eq!(
            diagram.steps,
            vec![
                message(0, 1, "hi", Line::Solid, Head::Arrow),
                message(1, 0, "back\nagain", Line::Dotted, Head::Arrow),
                message(0, 1, "lost", Line::Solid, Head::Cross),
                message(0, 1, "later", Line::Dotted, Head::Open),
                message(0, 1, "plain", Line::Solid, Head::None),
                message(0, 1, "on", Line::Solid, Head::Arrow),
                message(1, 0, "", Line::Dotted, Head::Arrow),
                Step::Message { from: 0, to: 1, text: "both".to_owned(), line: Line::Solid, head: Head::Arrow, both: true },
            ]
        );
    }

    #[test]
    fn notes_frames_and_numbers() {
        let source = "sequenceDiagram\nautonumber\nNote right of A: one\nnote over A,B: two\nloop Every day\nA->>B: x\nend\nalt yes\nB->>A: y\nelse no\nB->>A: z\nend\nbox Group\nparticipant C\nend\nactivate A";
        let diagram = parse(source).unwrap();
        assert!(diagram.numbered);
        assert_eq!(
            diagram.steps,
            vec![
                Step::Note { side: Side::Right(0), text: "one".to_owned() },
                Step::Note { side: Side::Over(0, 1), text: "two".to_owned() },
                Step::Start { kind: "loop".to_owned(), label: "Every day".to_owned() },
                message(0, 1, "x", Line::Solid, Head::Arrow),
                Step::End,
                Step::Start { kind: "alt".to_owned(), label: "yes".to_owned() },
                message(1, 0, "y", Line::Solid, Head::Arrow),
                Step::Part { label: "no".to_owned() },
                message(1, 0, "z", Line::Solid, Head::Arrow),
                Step::End,
            ]
        );
        assert_eq!(diagram.participants.len(), 3);
    }

    /// `source` laid out with every name 40 × 16 and words 8 px a character, 16 high.
    fn laid(source: &str) -> Layout {
        let diagram = parse(source).unwrap();
        let names = vec![(40.0, 16.0); diagram.participants.len()];
        let size = |text: &str| if text.is_empty() { (0.0, 0.0) } else { (8.0 * text.chars().count() as f64, 16.0) };
        let words: Vec<(f64, f64)> = diagram
            .steps
            .iter()
            .map(|step| match step {
                Step::Message { text, .. } | Step::Note { text, .. } => size(text),
                Step::Start { kind, label } => size(&format!("{kind}  {label}")),
                Step::Part { label } => size(label),
                Step::End => (0.0, 0.0),
            })
            .collect();
        layout(&diagram, &names, &words)
    }

    #[test]
    fn columns_leave_room_for_the_words_between_them() {
        let short = laid("sequenceDiagram\nA->>B: hi\nB->>C: hi");
        let long = laid("sequenceDiagram\nA->>B: a much longer message than that\nB->>C: hi");
        assert!(short.columns.windows(2).all(|pair| pair[1] > pair[0] + 40.0));
        // 31 characters, between A and B; C keeps its distance from B.
        assert!(long.columns[1] - long.columns[0] >= 248.0);
        assert!(long.columns[2] - long.columns[1] < long.columns[1] - long.columns[0]);
    }

    #[test]
    fn everything_goes_down_the_page_and_frames_hold_their_steps() {
        let laid = laid("sequenceDiagram\nA->>B: one\nNote right of B: a note\nloop again\nB->>A: two\nend\nA->>A: self");
        let tops: Vec<f64> = laid.rows.iter().map(|(top, _)| *top).collect();
        assert!(tops.windows(2).all(|pair| pair[1] > pair[0]));
        assert!(tops[0] > laid.tops[0].y + laid.tops[0].height);
        let lowest = laid.rows.iter().map(|(_, bottom)| *bottom).fold(0.0, f64::max);
        assert!(laid.bottoms[0].y >= lowest && laid.height > laid.bottoms[0].y + laid.bottoms[0].height);
        let (Spot::Frame(frame), Spot::Arrow { y, .. }) = (&laid.spots[2], &laid.spots[3]) else { panic!("a frame, then its message") };
        assert!(frame.y < *y && *y < frame.y + frame.height);
    }

    #[test]
    fn a_note_beside_a_participant_pushes_the_next_one_aside() {
        let laid = laid("sequenceDiagram\nparticipant A\nparticipant B\nNote right of A: a fairly long note here");
        let Spot::Note(note) = &laid.spots[0] else { panic!("a note") };
        assert!(note.x > laid.columns[0] && note.x + note.width < laid.columns[1]);
    }

    #[test]
    fn nothing_sticks_out_on_the_left() {
        let laid = laid("sequenceDiagram\nparticipant A\nNote left of A: a long note on the left side");
        let Spot::Note(note) = &laid.spots[0] else { panic!("a note") };
        assert!(note.x >= 0.0 && laid.tops[0].x >= 0.0 && laid.width >= note.x + note.width);
    }
}

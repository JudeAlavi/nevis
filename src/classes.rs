//! Mermaid class diagrams (```` ```mermaid ```` blocks starting with `classDiagram`): read
//! (classes with their attributes and methods, the relations between them, notes, namespaces),
//! laid out as a flowchart is, and drawn as UML draws them.

use gtk::{gdk, glib, graphene, gsk, pango, prelude::*};

use crate::flowchart::{self, Chart, Direction, Edge, End, Group, Line, Node, Shape};
use crate::{render, theme};

#[derive(Debug, PartialEq)]
pub struct Class {
    pub name: String,
    /// What is shown for its name (`Box~T~` is `Box<T>`).
    pub label: String,
    /// `<<interface>>` and the like, shown over its name.
    pub annotation: Option<String>,
    pub attributes: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct Relation {
    /// The classes (indices) at its two ends, as written left to right.
    pub from: usize,
    pub to: usize,
    pub line: Line,
    /// What each end has: an arrow, a triangle (inheritance), a diamond (composition) or a
    /// hollow one (aggregation).
    pub tail: End,
    pub head: End,
    pub label: String,
    /// How many at each end: `"1"`, `"many"`.
    pub tail_count: String,
    pub head_count: String,
}

#[derive(Debug, PartialEq)]
pub struct Diagram {
    pub direction: Direction,
    pub classes: Vec<Class>,
    pub relations: Vec<Relation>,
    /// Notes: about a class (index), or about the diagram.
    pub notes: Vec<(Option<usize>, String)>,
    /// Namespaces: a box round their classes (indices), outer ones first.
    pub namespaces: Vec<(String, Vec<usize>)>,
}

/// The class diagram in a Mermaid block, or nothing when it is another kind of diagram.
pub fn parse(source: &str) -> Option<Diagram> {
    let lines: Vec<&str> = source.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("%%")).collect();
    if !matches!(lines.first()?.split_whitespace().next()?, "classDiagram" | "classDiagram-v2") {
        return None;
    }
    let mut diagram = Diagram { direction: Direction::Down, classes: Vec::new(), relations: Vec::new(), notes: Vec::new(), namespaces: Vec::new() };
    // The class whose `{ … }` is being read, and the namespaces open.
    let mut body: Option<usize> = None;
    let mut open: Vec<usize> = Vec::new();
    for &line in &lines[1..] {
        if let Some(class) = body {
            if line == "}" {
                body = None;
            } else {
                diagram.member(class, line);
            }
            continue;
        }
        let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(word, rest)| (word, rest.trim()));
        match word {
            "direction" => {
                diagram.direction = match rest {
                    "LR" => Direction::Right,
                    "RL" => Direction::Left,
                    "BT" => Direction::Up,
                    _ => Direction::Down,
                };
            }
            "class" => {
                let class = diagram.class(rest.trim_end_matches('{').trim(), &open);
                // Declared in a namespace, it is in it, even if a relation named it before.
                if !diagram.namespaces.iter().any(|(_, members)| members.contains(&class)) {
                    for &space in &open {
                        diagram.namespaces[space].1.push(class);
                    }
                }
                if rest.ends_with('{') {
                    body = Some(class);
                }
            }
            "namespace" => {
                diagram.namespaces.push((rest.trim_end_matches('{').trim().to_owned(), Vec::new()));
                open.push(diagram.namespaces.len() - 1);
            }
            "}" => {
                open.pop();
            }
            "note" => match rest.strip_prefix("for ") {
                Some(about) => {
                    let (name, text) = about.split_once(char::is_whitespace).unwrap_or((about, ""));
                    let class = diagram.class(name, &open);
                    diagram.notes.push((Some(class), unquote(text)));
                }
                None => diagram.notes.push((None, unquote(rest))),
            },
            "style" | "classDef" | "cssClass" | "link" | "click" | "callback" | "title" | "accTitle" | "accDescr" => {}
            // `<<interface>> Name`.
            _ if line.starts_with("<<") => {
                if let Some((annotation, name)) = line[2..].split_once(">>") {
                    let class = diagram.class(name.trim(), &open);
                    diagram.classes[class].annotation = Some(annotation.trim().to_owned());
                }
            }
            _ => {
                if !diagram.relation(line, &open)
                    && let Some((name, member)) = line.split_once(':')
                {
                    let class = diagram.class(name.trim(), &open);
                    diagram.member(class, member.trim());
                }
            }
        }
    }
    Some(diagram)
}

/// Text without the quotes round it, `<br>` breaking a line.
fn unquote(text: &str) -> String {
    crate::flowchart::label(text)
}

impl Diagram {
    /// The class `text` names (`Name`, `Name~T~`, `Name["Label"]`, `Name:::style`): one named
    /// already, or a new one, in the namespaces `open`.
    fn class(&mut self, text: &str, open: &[usize]) -> usize {
        let text = text.split(":::").next().unwrap_or(text).trim();
        let (name, label) = match text.split_once('[') {
            Some((name, label)) => (name.trim(), Some(unquote(label.trim_end_matches(']')))),
            None => (text, None),
        };
        let (name, label) = match name.split_once('~') {
            Some((base, generic)) => (base.trim(), label.unwrap_or_else(|| format!("{}<{}>", base.trim(), generic.trim_end_matches('~')))),
            None => (name, label.unwrap_or_else(|| name.to_owned())),
        };
        if let Some(index) = self.classes.iter().position(|class| class.name == name) {
            return index;
        }
        self.classes.push(Class { name: name.to_owned(), label, annotation: None, attributes: Vec::new(), methods: Vec::new() });
        let index = self.classes.len() - 1;
        for &space in open {
            self.namespaces[space].1.push(index);
        }
        index
    }

    /// A line of a class's body: `<<annotation>>`, a method (it has brackets) or an attribute.
    fn member(&mut self, class: usize, text: &str) {
        let class = &mut self.classes[class];
        if let Some(annotation) = text.strip_prefix("<<").and_then(|text| text.strip_suffix(">>")) {
            class.annotation = Some(annotation.trim().to_owned());
        } else if text.contains('(') {
            class.methods.push(text.to_owned());
        } else if !text.is_empty() {
            class.attributes.push(text.to_owned());
        }
    }

    /// `A "1" *-- "many" B : label`, with any of UML's ends: whether `line` is one.
    fn relation(&mut self, line: &str, open: &[usize]) -> bool {
        let (link, label) = match line.split_once(':') {
            Some((link, label)) => (link, unquote(label)),
            None => (line, String::new()),
        };
        let words = tokens(link);
        let Some(at) = words.iter().position(|word| word.contains("--") || word.contains("..")) else { return false };
        let named = |words: &[String]| words.iter().find(|word| !word.starts_with('"')).cloned();
        let counted = |words: &[String]| words.iter().find(|word| word.starts_with('"')).map(|word| unquote(word)).unwrap_or_default();
        let (Some(left), Some(right)) = (named(&words[..at]), named(&words[at + 1..])) else { return false };
        let arrow = &words[at];
        let (line, middle) = if arrow.contains("..") { (Line::Dotted, "..") } else { (Line::Solid, "--") };
        let Some((tail, head)) = arrow.split_once(middle) else { return false };
        let end = |mark: &str| match mark.trim_matches(['-', '.']) {
            "<|" | "|>" => End::Triangle,
            "*" => End::Diamond,
            "o" => End::HollowDiamond,
            "<" | ">" => End::Arrow,
            _ => End::Plain,
        };
        let (from, to) = (self.class(&left, open), self.class(&right, open));
        let (tail_count, head_count) = (counted(&words[..at]), counted(&words[at + 1..]));
        self.relations.push(Relation { from, to, line, tail: end(tail), head: end(head), label, tail_count, head_count });
        true
    }
}

/// `text`'s words, a quoted one whole.
fn tokens(text: &str) -> Vec<String> {
    let (mut words, mut word, mut quoted) = (Vec::new(), String::new(), false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Room round a compartment's text, and round a relation's label.
const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 6.0;
/// Wider diagrams are shown smaller.
const WIDEST: f64 = 900.0;

/// A class as drawn: its name (and annotation), attributes and methods, laid out; its size.
struct Drawn {
    name: pango::Layout,
    attributes: Option<pango::Layout>,
    methods: Option<pango::Layout>,
    size: (f64, f64),
}

/// The diagram drawn as a picture; `text` lays out words in the rendered view's font.
pub fn picture(diagram: &Diagram, text: &dyn Fn(&str) -> pango::Layout) -> gtk::Picture {
    let lines = |lines: &[String]| {
        let layout = text("");
        layout.set_text(&lines.join("\n"));
        layout
    };
    let tall = |layout: &pango::Layout| f64::from(layout.pixel_size().1);
    let wide = |layout: &pango::Layout| f64::from(layout.pixel_size().0);
    let classes: Vec<Drawn> = diagram
        .classes
        .iter()
        .map(|class| {
            let name = text("");
            let annotation = class.annotation.as_ref().map(|a| format!("<small>\u{ab}{}\u{bb}</small>\n", glib::markup_escape_text(a))).unwrap_or_default();
            name.set_markup(&format!("{annotation}<b>{}</b>", glib::markup_escape_text(&class.label)));
            name.set_alignment(pango::Alignment::Center);
            // A class with members shows both compartments, an empty one thin.
            let members = !class.attributes.is_empty() || !class.methods.is_empty();
            let (attributes, methods) = if members { (Some(lines(&class.attributes)), Some(lines(&class.methods))) } else { (None, None) };
            let parts = [Some(&name), attributes.as_ref(), methods.as_ref()];
            let width = parts.iter().flatten().map(|layout| wide(layout)).fold(0.0, f64::max) + 2.0 * PAD_X;
            let height = parts.iter().flatten().map(|layout| tall(layout).max(1.0) + 2.0 * PAD_Y).sum();
            Drawn { name, attributes, methods, size: (width.max(80.0), height) }
        })
        .collect();
    let notes: Vec<pango::Layout> = diagram.notes.iter().map(|(_, note)| text(note)).collect();

    // Laid out as a flowchart: classes and notes its nodes, relations its edges, a note beside its
    // class on a dotted line.
    let mut chart = Chart {
        direction: diagram.direction,
        nodes: diagram.classes.iter().map(|class| Node { id: class.name.clone(), label: String::new(), shape: Shape::Box }).collect(),
        edges: diagram
            .relations
            .iter()
            .map(|relation| Edge {
                from: relation.from,
                to: relation.to,
                label: relation.label.clone(),
                line: relation.line,
                tail: relation.tail,
                head: relation.head,
                length: 1,
            })
            .collect(),
        groups: diagram.namespaces.iter().map(|(name, members)| Group { label: name.clone(), members: members.clone() }).collect(),
    };
    let mut sizes: Vec<(f64, f64)> = classes.iter().map(|class| class.size).collect();
    for ((about, _), layout) in diagram.notes.iter().zip(&notes) {
        chart.nodes.push(Node { id: String::new(), label: String::new(), shape: Shape::Box });
        sizes.push((wide(layout) + 2.0 * PAD_X, tall(layout) + 2.0 * PAD_Y));
        if let Some(class) = *about {
            let note = chart.nodes.len() - 1;
            chart.edges.push(Edge { from: class, to: note, label: String::new(), line: Line::Dotted, tail: End::Plain, head: End::Plain, length: 0 });
        }
    }
    let words: Vec<Option<pango::Layout>> = chart.edges.iter().map(|edge| (!edge.label.is_empty()).then(|| text(&edge.label))).collect();
    let labels: Vec<(f64, f64)> = words.iter().map(|word| word.as_ref().map_or((0.0, 0.0), |layout| (wide(layout) + 10.0, tall(layout) + 4.0))).collect();
    let laid = flowchart::layout(&chart, &sizes, &labels);

    let colors = theme::colors();
    let (line, border, fill, ink, ground, dim) = (
        render::rgba(colors.dim, 1.0),
        render::rgba(colors.marker, 1.0),
        render::rgba(colors.quote_background, 1.0),
        render::rgba(colors.text, 1.0),
        render::rgba(colors.background, 1.0),
        render::rgba(colors.quote, 1.0),
    );
    let snapshot = gtk::Snapshot::new();
    let (left, top, right, bottom) = flowchart::draw_groups(&snapshot, &chart, &laid, text);
    let put = |layout: &pango::Layout, (x, y): (f64, f64), ink: &gdk::RGBA| {
        snapshot.save();
        snapshot.translate(&graphene::Point::new(x as f32, y as f32));
        snapshot.append_layout(layout, ink);
        snapshot.restore();
    };
    // Relations, with their labels on a patch of the page and how many at each end.
    for (index, (edge, route)) in chart.edges.iter().zip(&laid.edges).enumerate() {
        flowchart::draw_edge(&snapshot, edge, &route.points, &line);
        if let Some(word) = &words[index] {
            let (w, h) = (wide(word), tall(word));
            snapshot.append_color(&ground, &graphene::Rect::new((route.label.0 - w / 2.0 - 4.0) as f32, (route.label.1 - h / 2.0) as f32, (w + 8.0) as f32, h as f32));
            put(word, (route.label.0 - w / 2.0, route.label.1 - h / 2.0), &dim);
        }
        let Some(relation) = diagram.relations.get(index) else { continue };
        let points = &route.points;
        for (count, end, next) in [(&relation.tail_count, points.first(), points.get(1)), (&relation.head_count, points.last(), points.len().checked_sub(2).and_then(|i| points.get(i)))] {
            let (Some(&end), Some(&next), false) = (end, next, count.is_empty()) else { continue };
            let (end, next) = ((end.0 as f32, end.1 as f32), (next.0 as f32, next.1 as f32));
            let along = flowchart::towards(end, next, 22.0);
            let layout = text(count);
            // Beside the line, not on it.
            let (dx, dy) = (next.0 - end.0, next.1 - end.1);
            let length = dx.hypot(dy).max(0.001);
            let aside = (-dy / length * 12.0, dx / length * 12.0);
            put(&layout, (f64::from(along.0 + aside.0) - wide(&layout) / 2.0, f64::from(along.1 + aside.1) - tall(&layout) / 2.0), &dim);
        }
    }
    // Classes: a box, its name (and annotation) on top, then a compartment for attributes and
    // one for methods.
    for (class, rect) in classes.iter().zip(&laid.nodes) {
        let frame = graphene::Rect::new(rect.x as f32, rect.y as f32, rect.width as f32, rect.height as f32);
        let outline = gsk::RoundedRect::from_rect(frame, 4.0);
        snapshot.push_rounded_clip(&outline);
        snapshot.append_color(&fill, &frame);
        snapshot.pop();
        snapshot.append_border(&outline, &[1.2; 4], &[border; 4]);
        let mut y = rect.y + PAD_Y;
        put(&class.name, (rect.x + (rect.width - wide(&class.name)) / 2.0, y), &ink);
        y += tall(&class.name) + PAD_Y;
        for part in [&class.attributes, &class.methods].into_iter().flatten() {
            snapshot.append_color(&border, &graphene::Rect::new(rect.x as f32, y as f32, rect.width as f32, 1.0));
            put(part, (rect.x + PAD_X, y + PAD_Y), &ink);
            y += tall(part).max(1.0) + 2.0 * PAD_Y;
        }
    }
    // Notes.
    for (layout, rect) in notes.iter().zip(&laid.nodes[classes.len()..]) {
        let frame = graphene::Rect::new(rect.x as f32, rect.y as f32, rect.width as f32, rect.height as f32);
        let outline = gsk::RoundedRect::from_rect(frame, 3.0);
        snapshot.push_rounded_clip(&outline);
        snapshot.append_color(&ground, &frame);
        snapshot.append_color(&render::rgba(colors.changed, 0.16), &frame);
        snapshot.pop();
        snapshot.append_border(&outline, &[1.0; 4], &[border; 4]);
        put(layout, (rect.x + PAD_X, rect.y + PAD_Y), &ink);
    }

    let (wide, tall) = (f64::from(right - left), f64::from(bottom - top));
    let picture = match snapshot.to_paintable(Some(&graphene::Size::new(wide as f32, tall as f32))) {
        Some(paintable) => gtk::Picture::for_paintable(&paintable),
        None => gtk::Picture::new(),
    };
    picture.set_can_shrink(true);
    let width = wide.min(WIDEST);
    picture.set_size_request(width as i32, (tall * width / wide.max(1.0)) as i32);
    picture.set_alternative_text(Some("Class diagram"));
    picture
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str, label: &str, annotation: Option<&str>, attributes: &[&str], methods: &[&str]) -> Class {
        let owned = |list: &[&str]| list.iter().map(|item| item.to_string()).collect();
        Class { name: name.to_owned(), label: label.to_owned(), annotation: annotation.map(str::to_owned), attributes: owned(attributes), methods: owned(methods) }
    }

    #[test]
    fn the_first_line_says_class_diagram() {
        assert!(parse("graph TD\nA-->B").is_none());
        assert!(parse("classDiagram").is_some());
    }

    #[test]
    fn classes_with_members_annotations_and_generics() {
        let source = "classDiagram\nclass Animal {\n  <<interface>>\n  +String name\n  +makeSound() void\n}\nclass Box~T~\nDuck : +swim()\nDuck : +String beak\n<<abstract>> Duck";
        let diagram = parse(source).unwrap();
        assert_eq!(
            diagram.classes,
            vec![
                class("Animal", "Animal", Some("interface"), &["+String name"], &["+makeSound() void"]),
                class("Box", "Box<T>", None, &[], &[]),
                class("Duck", "Duck", Some("abstract"), &["+String beak"], &["+swim()"]),
            ]
        );
    }

    #[test]
    fn relations_carry_their_ends_line_counts_and_label() {
        let source = "classDiagram\nAnimal <|-- Duck\nAnimal \"1\" *-- \"many\" Leg : has\nFoo ..> Bar : uses\nFoo --o Baz\nFoo ..|> Iface\nA -- B";
        let diagram = parse(source).unwrap();
        let names: Vec<&str> = diagram.classes.iter().map(|class| class.name.as_str()).collect();
        assert_eq!(names, ["Animal", "Duck", "Leg", "Foo", "Bar", "Baz", "Iface", "A", "B"]);
        let ends: Vec<(usize, usize, Line, End, End, &str)> =
            diagram.relations.iter().map(|r| (r.from, r.to, r.line, r.tail, r.head, r.label.as_str())).collect();
        assert_eq!(
            ends,
            vec![
                (0, 1, Line::Solid, End::Triangle, End::Plain, ""),
                (0, 2, Line::Solid, End::Diamond, End::Plain, "has"),
                (3, 4, Line::Dotted, End::Plain, End::Arrow, "uses"),
                (3, 5, Line::Solid, End::Plain, End::HollowDiamond, ""),
                (3, 6, Line::Dotted, End::Plain, End::Triangle, ""),
                (7, 8, Line::Solid, End::Plain, End::Plain, ""),
            ]
        );
        assert_eq!((diagram.relations[1].tail_count.as_str(), diagram.relations[1].head_count.as_str()), ("1", "many"));
    }

    #[test]
    fn notes_namespaces_and_direction() {
        let source = "classDiagram\ndirection LR\nTriangle --> Square\nnamespace Shapes {\n  class Square\n  class Circle\n}\nnote for Square \"has corners\"\nnote \"a general note\"";
        let diagram = parse(source).unwrap();
        assert_eq!(diagram.direction, Direction::Right);
        // Square was named by a relation before; declared in the namespace, it is in it.
        assert_eq!(diagram.namespaces, vec![("Shapes".to_owned(), vec![1, 2])]);
        assert_eq!(diagram.notes, vec![(Some(1), "has corners".to_owned()), (None, "a general note".to_owned())]);
    }
}

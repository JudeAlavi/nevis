//! Mermaid state diagrams (```` ```mermaid ```` blocks starting with `stateDiagram`): read into
//! a flowchart (states, moves between them, `[*]` as a start or an end, choices, forks and
//! joins, notes; states inside a state in a box), and drawn as one.

use std::collections::HashMap;

use crate::flowchart::{self, Chart, Direction, Edge, End, Group, Line, Node, Shape};

/// The state diagram in a Mermaid block as a flowchart, or nothing when it is another kind.
pub fn parse(source: &str) -> Option<Chart> {
    let lines: Vec<&str> = source.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("%%")).collect();
    if !matches!(lines.first()?.split_whitespace().next()?, "stateDiagram" | "stateDiagram-v2") {
        return None;
    }
    let chart = Chart { direction: Direction::Down, nodes: Vec::new(), edges: Vec::new(), groups: Vec::new() };
    let mut reading = Reading { chart, index: HashMap::new(), inner: inner_states(&lines[1..]), scopes: Vec::new() };
    let mut note: Option<(String, Vec<String>)> = None;
    for &line in &lines[1..] {
        // A note over several lines, to "end note".
        if let Some((state, text)) = &mut note {
            if line == "end note" {
                let (state, text) = (state.clone(), text.join("\n"));
                reading.note(&state, &text);
                note = None;
            } else {
                text.push(line.to_owned());
            }
            continue;
        }
        let (first, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(first, rest)| (first, rest.trim()));
        match first {
            "direction" if reading.scopes.is_empty() => {
                reading.chart.direction = match rest {
                    "LR" => Direction::Right,
                    "RL" => Direction::Left,
                    "BT" => Direction::Up,
                    _ => Direction::Down,
                };
            }
            "state" => reading.state(rest),
            "}" => {
                reading.scopes.pop();
            }
            "note" => match rest.split_once(':') {
                Some((place, text)) => reading.note(&noted(place), text.trim()),
                None => note = Some((noted(rest), Vec::new())),
            },
            "--" | "classDef" | "class" | "style" | "scale" | "title" | "accTitle" | "accDescr" | "hide" | "direction" => {}
            _ => reading.statement(line),
        }
    }
    Some(reading.chart)
}

/// The state a note is about: "right of A" is A.
fn noted(place: &str) -> String {
    let place = place.trim();
    let state = place.strip_prefix("right of").or_else(|| place.strip_prefix("left of")).unwrap_or(place);
    state.trim().to_owned()
}

/// The states with states inside (`state X {`), and whether each has its own start and end.
fn inner_states(lines: &[&str]) -> HashMap<String, (bool, bool)> {
    let mut found: HashMap<String, (bool, bool)> = HashMap::new();
    let mut scopes: Vec<String> = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("state ").and_then(|rest| rest.trim().strip_suffix('{')) {
            let name = declared(rest.trim()).0;
            found.entry(name.clone()).or_default();
            scopes.push(name);
        } else if *line == "}" {
            scopes.pop();
        } else if let Some(scope) = scopes.last()
            && let Some((from, to, _)) = transition(line)
        {
            let entry = found.entry(scope.clone()).or_default();
            entry.0 |= from == "[*]";
            entry.1 |= to == "[*]";
        }
    }
    found
}

/// `"Long name" as X` or `X`: the state's name, and its label if it has one of its own.
fn declared(text: &str) -> (String, Option<String>) {
    match text.split_once(" as ") {
        Some((label, name)) => (bare(name), Some(flowchart::label(label))),
        None => (bare(text), None),
    }
}

/// A state's name without a style (`X:::warm`).
fn bare(name: &str) -> String {
    name.split(":::").next().unwrap_or(name).trim().to_owned()
}

/// `A --> B` or `A --> B : label`: the two states and the label.
fn transition(line: &str) -> Option<(String, String, String)> {
    let (from, rest) = line.split_once("-->")?;
    let (to, label) = rest.split_once(':').unwrap_or((rest, ""));
    Some((bare(from), bare(to), flowchart::label(label)))
}

struct Reading {
    chart: Chart,
    /// Each node by what it stands for: `s:name` a state, `start:X`/`end:X` the `[*]`s in
    /// state X (`` for none), `note:n` a note.
    index: HashMap<String, usize>,
    /// The states with states inside: whether each has its own start and end.
    inner: HashMap<String, (bool, bool)>,
    /// Those open now, innermost last, with their box.
    scopes: Vec<(String, usize)>,
}

impl Reading {
    /// The node `key` stands for, made (with `label` and `shape`) the first time: in every box
    /// open, or for a state's own start and end, in its box.
    fn node(&mut self, key: &str, label: &str, shape: Shape) -> usize {
        if let Some(&index) = self.index.get(key) {
            return index;
        }
        self.chart.nodes.push(Node { id: key.to_owned(), label: label.to_owned(), shape });
        let index = self.chart.nodes.len() - 1;
        self.index.insert(key.to_owned(), index);
        let own = key.strip_prefix("start:").or_else(|| key.strip_prefix("end:")).filter(|state| !state.is_empty());
        let boxes: Vec<usize> = match own {
            Some(state) => self.scopes.iter().position(|(name, _)| name == state).map_or_else(Vec::new, |at| self.scopes[..=at].iter().map(|(_, group)| *group).collect()),
            None => self.scopes.iter().map(|(_, group)| *group).collect(),
        };
        for group in boxes {
            self.chart.groups[group].members.push(index);
        }
        index
    }

    /// A move's end at state `name`: `[*]` is the start (`from`) or the end of the state the
    /// move is in; a state with states inside is its own start (going in) or end (coming out),
    /// when it has them.
    fn end_of_move(&mut self, name: &str, from: bool) -> usize {
        let scope = self.scopes.last().map_or(String::new(), |(name, _)| name.clone());
        if name == "[*]" {
            return if from { self.node(&format!("start:{scope}"), "", Shape::Start) } else { self.node(&format!("end:{scope}"), "", Shape::End) };
        }
        match self.inner.get(name).copied() {
            Some((true, _)) if !from => self.node(&format!("start:{name}"), "", Shape::Start),
            Some((_, true)) if from => self.node(&format!("end:{name}"), "", Shape::End),
            _ => self.node(&format!("s:{name}"), name, Shape::Round),
        }
    }

    /// `"Long name" as X`, `X <<choice>>` (or fork, join), `X {`.
    fn state(&mut self, text: &str) {
        if let Some(inner) = text.strip_suffix('{') {
            let (name, label) = declared(inner.trim());
            self.chart.groups.push(Group { label: label.unwrap_or_else(|| name.clone()), members: Vec::new() });
            let group = self.chart.groups.len() - 1;
            self.scopes.push((name.clone(), group));
            // Its own start and end, if moves named them before: in its box (and those round it).
            for key in [format!("start:{name}"), format!("end:{name}")] {
                if let Some(&index) = self.index.get(&key) {
                    for (_, group) in &self.scopes {
                        self.chart.groups[*group].members.push(index);
                    }
                }
            }
            return;
        }
        if let Some((name, kind)) = text.split_once("<<") {
            let shape = if kind.starts_with("choice") { Shape::Diamond } else { Shape::Bar };
            let name = bare(name);
            let index = self.node(&format!("s:{name}"), "", shape);
            (self.chart.nodes[index].shape, self.chart.nodes[index].label) = (shape, String::new());
            return;
        }
        let (name, label) = declared(text);
        let index = self.node(&format!("s:{name}"), &name, Shape::Round);
        if let Some(label) = label {
            self.chart.nodes[index].label = label;
        }
    }

    /// `A --> B : label`, or `A : description` (shown under its name).
    fn statement(&mut self, line: &str) {
        if let Some((from, to, label)) = transition(line) {
            let (from, to) = (self.end_of_move(&from, true), self.end_of_move(&to, false));
            self.chart.edges.push(Edge { from, to, label, line: Line::Solid, tail: End::Plain, head: End::Arrow, length: 1 });
        } else if let Some((name, description)) = line.split_once(':') {
            let name = bare(name);
            let index = self.node(&format!("s:{name}"), &name, Shape::Round);
            let label = &mut self.chart.nodes[index].label;
            label.push('\n');
            label.push_str(&flowchart::label(description));
        }
    }

    /// A note about state `name`: a box hanging off it by a dotted line.
    fn note(&mut self, name: &str, text: &str) {
        let state = self.end_of_move(name, true);
        let key = format!("note:{}", self.chart.nodes.len());
        let note = self.node(&key, &flowchart::label(text), Shape::Box);
        // Beside it, on its rank (as Mermaid puts a note), not below it.
        self.chart.edges.push(Edge { from: state, to: note, label: String::new(), line: Line::Dotted, tail: End::Plain, head: End::Plain, length: 0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flowchart::{Direction, End, Group, Line, Shape};

    fn names(chart: &Chart) -> Vec<(&str, Shape)> {
        chart.nodes.iter().map(|node| (node.label.as_str(), node.shape)).collect()
    }

    fn moves(chart: &Chart) -> Vec<(usize, usize, &str)> {
        chart.edges.iter().map(|edge| (edge.from, edge.to, edge.label.as_str())).collect()
    }

    #[test]
    fn the_first_line_says_state_diagram() {
        assert!(parse("graph TD\nA-->B").is_none());
        assert!(parse("stateDiagram-v2").is_some());
        assert!(parse("%% a comment\nstateDiagram\n[*] --> A").is_some());
    }

    #[test]
    fn states_moves_and_where_it_starts_and_ends() {
        let chart =
            parse("stateDiagram-v2\n    [*] --> Still\n    Still --> Moving : push\n    Moving --> [*]\n    state \"Very still\" as Still\n    Moving : on wheels").unwrap();
        assert_eq!(names(&chart), vec![("", Shape::Start), ("Very still", Shape::Round), ("Moving\non wheels", Shape::Round), ("", Shape::End)]);
        assert_eq!(moves(&chart), vec![(0, 1, ""), (1, 2, "push"), (2, 3, "")]);
        assert_eq!(chart.direction, Direction::Down);
    }

    #[test]
    fn choices_forks_notes_and_states_inside_a_state() {
        let source = "stateDiagram\ndirection LR\nstate pick <<choice>>\nstate split <<fork>>\nA --> pick\npick --> B : yes\nnote right of A : a note\nstate Group {\n  [*] --> Inner\n  Inner --> [*]\n}\nB --> Group\nGroup --> A";
        let chart = parse(source).unwrap();
        assert_eq!(chart.direction, Direction::Right);
        assert_eq!(
            names(&chart),
            vec![
                ("", Shape::Diamond),
                ("", Shape::Bar),
                ("A", Shape::Round),
                ("B", Shape::Round),
                ("a note", Shape::Box),
                ("", Shape::Start),
                ("Inner", Shape::Round),
                ("", Shape::End),
            ]
        );
        // Into a state with states inside: to its start; out of it: from its end.
        assert_eq!(moves(&chart), vec![(2, 0, ""), (0, 3, "yes"), (2, 4, ""), (5, 6, ""), (6, 7, ""), (3, 5, ""), (7, 2, "")]);
        // A note hangs off its state by a dotted line.
        assert_eq!((chart.edges[2].line, chart.edges[2].head), (Line::Dotted, End::Plain));
        assert_eq!(chart.groups, vec![Group { label: "Group".to_owned(), members: vec![5, 6, 7] }]);
    }
}

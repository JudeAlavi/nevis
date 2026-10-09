//! Mermaid flowcharts (```` ```mermaid ```` blocks starting with `flowchart` or `graph`):
//! read, laid out in ranks along the arrows, and drawn. Sequence diagrams are in `sequence`;
//! other diagram types stay code.

use gtk::{gdk, graphene, gsk, pango, prelude::*};

use crate::theme;

/// Which way the arrows run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    Down,
    Up,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// `[text]`
    Box,
    /// `(text)`
    Round,
    /// `([text])`
    Stadium,
    /// `[[text]]`
    Subroutine,
    /// `[(text)]`
    Cylinder,
    /// `((text))`
    Circle,
    /// `{text}`
    Diamond,
    /// `{{text}}`
    Hexagon,
    /// `[/text/]` and the other slanted sides
    Slanted,
    /// `>text]`
    Flag,
}

#[derive(Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Line {
    Solid,
    Dotted,
    Thick,
    Hidden,
}

/// What an end of an edge has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End {
    Plain,
    Arrow,
    Circle,
    Cross,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    /// Indices into the chart's nodes.
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub line: Line,
    /// The ends at `from` and at `to`.
    pub tail: End,
    pub head: End,
    /// How many ranks it spans at least: 1, more for longer arrows (`--->`).
    pub length: usize,
}

#[derive(Debug, PartialEq)]
pub struct Chart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// A box on the canvas, in pixels from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// An edge's path, from its `from` end to its `to` end, and where its label is centred.
#[derive(Debug)]
pub struct Route {
    pub points: Vec<(f64, f64)>,
    pub label: (f64, f64),
}

/// Where everything goes.
#[derive(Debug)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    /// One box per node, one route per edge, in the chart's order.
    pub nodes: Vec<Rect>,
    pub edges: Vec<Route>,
}

/// Lays `chart` out in ranks along its arrows, given each node's size and each edge label's
/// size ((0, 0) for none), as (width, height).
pub fn layout(chart: &Chart, sizes: &[(f64, f64)], labels: &[(f64, f64)]) -> Layout {
    let across = matches!(chart.direction, Direction::Right | Direction::Left);
    // Laid out top-down; across the page every size is turned a quarter, and so is the result.
    let turned = |(width, height): (f64, f64)| if across { (height, width) } else { (width, height) };
    let backward = backward(chart);
    let rank = ranks(chart, &backward);
    let nodes = chart.nodes.len();

    // The nodes, then stand-ins where a longer edge passes a rank: each with its rank and size.
    let mut vertices: Vec<(usize, (f64, f64))> = (0..nodes).map(|node| (rank[node], turned(sizes[node]))).collect();
    // Each edge's vertices from its upper end to its lower end (none for a loop to itself).
    let mut chains = Vec::new();
    for (index, edge) in chart.edges.iter().enumerate() {
        if edge.from == edge.to {
            chains.push(Vec::new());
            continue;
        }
        let (upper, lower) = if backward[index] { (edge.to, edge.from) } else { (edge.from, edge.to) };
        let mut chain = vec![upper];
        // The label rides on the middle stand-in, which is as big as it.
        let middle = (rank[upper] + rank[lower]) / 2;
        for r in rank[upper] + 1..rank[lower] {
            vertices.push((r, if r == middle { turned(labels[index]) } else { (0.0, 0.0) }));
            chain.push(vertices.len() - 1);
        }
        chain.push(lower);
        chains.push(chain);
    }
    let mut rows = vec![Vec::new(); vertices.iter().map(|&(r, _)| r + 1).max().unwrap_or(0)];
    for (vertex, &(r, _)) in vertices.iter().enumerate() {
        rows[r].push(vertex);
    }
    let (mut up, mut down) = (vec![Vec::new(); vertices.len()], vec![Vec::new(); vertices.len()]);
    for chain in &chains {
        for pair in chain.windows(2) {
            down[pair[0]].push(pair[1]);
            up[pair[1]].push(pair[0]);
        }
    }
    order(&mut rows, &up, &down);
    let gap = |a: usize, b: usize| if a < nodes && b < nodes { NODE_GAP } else { PASS_GAP };
    let left = positions(&rows, &vertices, &up, &down, gap);

    // Below each rank, room for the labels of edges to the next one.
    let mut gaps = vec![RANK_GAP; rows.len()];
    for (index, chain) in chains.iter().enumerate().filter(|(_, chain)| chain.len() == 2) {
        let label = turned(labels[index]).1;
        if label > 0.0 {
            let r = vertices[chain[0]].0;
            gaps[r] = gaps[r].max(label + 2.0 * LABEL_ROOM);
        }
    }
    // Rows from the top, each as thick as its thickest vertex.
    let mut middles = vec![0.0; rows.len()];
    let mut top = MARGIN;
    for (r, row) in rows.iter().enumerate() {
        let thickness = row.iter().map(|&vertex| vertices[vertex].1 .1).fold(0.0, f64::max);
        middles[r] = top + thickness / 2.0;
        top += thickness + gaps[r];
    }
    let boxes: Vec<Rect> = vertices
        .iter()
        .enumerate()
        .map(|(vertex, &(r, (width, height)))| Rect { x: left[vertex], y: middles[r] - height / 2.0, width, height })
        .collect();
    let center = |rect: Rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    let edges: Vec<Route> = chart
        .edges
        .iter()
        .zip(&chains)
        .enumerate()
        .map(|(index, (edge, chain))| {
            if chain.is_empty() {
                return round_the_side(boxes[edge.from]);
            }
            let (upper, lower) = (boxes[chain[0]], boxes[chain[chain.len() - 1]]);
            let mut points: Vec<(f64, f64)> = chain.iter().map(|&vertex| center(boxes[vertex])).collect();
            let label = match chain.len() {
                // Halfway between the ranks; edges between the same two nodes bend apart.
                2 => {
                    let twins: Vec<usize> = (0..chains.len()).filter(|&other| chains[other] == *chain).collect();
                    let nth = twins.iter().position(|&other| other == index).unwrap_or(0);
                    let aside = (nth as f64 - (twins.len() - 1) as f64 / 2.0) * TWIN_GAP;
                    let middle = ((points[0].0 + points[1].0) / 2.0 + aside, (upper.y + upper.height + lower.y) / 2.0);
                    if aside != 0.0 {
                        points.insert(1, middle);
                    }
                    middle
                }
                _ => center(boxes[chain[chain.len() / 2]]),
            };
            // From the nodes' outlines, not their middles.
            let last = points.len() - 1;
            let (upper_node, lower_node) = (chain[0], chain[chain.len() - 1]);
            points[0] = outline_towards(chart.nodes[upper_node].shape, upper, points[1]);
            points[last] = outline_towards(chart.nodes[lower_node].shape, lower, points[last - 1]);
            if backward[index] {
                points.reverse();
            }
            Route { points, label }
        })
        .collect();
    // Big enough for everything, loops round the side included.
    let right = boxes.iter().map(|rect| rect.x + rect.width).chain(edges.iter().flat_map(|route| route.points.iter().map(|&(x, _)| x)));
    let width = right.fold(0.0, f64::max) + MARGIN;
    let height = top - gaps.last().unwrap_or(&0.0) + MARGIN;
    Layout { width, height, nodes: boxes[..nodes].to_vec(), edges }.facing(chart.direction)
}

/// Space between ranks, between neighbours on a rank, beside an edge passing a rank, between
/// edges joining the same two nodes, and around the drawing.
const RANK_GAP: f64 = 48.0;
const NODE_GAP: f64 = 32.0;
const PASS_GAP: f64 = 16.0;
const TWIN_GAP: f64 = 28.0;
/// Space between a label and the nodes above and below it.
const LABEL_ROOM: f64 = 8.0;
const MARGIN: f64 = 8.0;

/// Orders each row so that fewer edges cross: each vertex moves towards the average place of
/// its neighbours above, then below, a few times over; the best order seen wins.
fn order(rows: &mut [Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>]) {
    let mut place = vec![0.0; up.len()];
    let number = |rows: &[Vec<usize>], place: &mut [f64]| {
        for row in rows {
            for (index, &vertex) in row.iter().enumerate() {
                place[vertex] = index as f64;
            }
        }
    };
    number(rows, &mut place);
    let mut best = (crossings(rows, down, &place), rows.to_vec());
    for sweep in 0..8 {
        let (ranks, neighbours): (Vec<usize>, _) = if sweep % 2 == 0 {
            ((1..rows.len()).collect(), up)
        } else {
            ((0..rows.len().saturating_sub(1)).rev().collect(), down)
        };
        for r in ranks {
            let mut keyed: Vec<(f64, usize)> = rows[r]
                .iter()
                .map(|&vertex| {
                    let around = &neighbours[vertex];
                    let key = if around.is_empty() {
                        place[vertex]
                    } else {
                        around.iter().map(|&other| place[other]).sum::<f64>() / around.len() as f64
                    };
                    (key, vertex)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows[r] = keyed.into_iter().map(|(_, vertex)| vertex).collect();
            number(&rows[r..=r], &mut place);
        }
        let now = crossings(rows, down, &place);
        if now < best.0 {
            best = (now, rows.to_vec());
        }
    }
    rows.clone_from_slice(&best.1);
}

/// How many pairs of edges cross between neighbouring rows, given each vertex's `place`.
fn crossings(rows: &[Vec<usize>], down: &[Vec<usize>], place: &[f64]) -> usize {
    let mut count = 0;
    for row in rows {
        let spans: Vec<(f64, f64)> =
            row.iter().flat_map(|&vertex| down[vertex].iter().map(move |&below| (vertex, below))).map(|(a, b)| (place[a], place[b])).collect();
        for (index, a) in spans.iter().enumerate() {
            count += spans[index + 1..].iter().filter(|b| (a.0 - b.0) * (a.1 - b.1) < 0.0).count();
        }
    }
    count
}

/// Each vertex's left side: rows packed in order, then every vertex pulled towards the middle
/// of its neighbours (above, then below, a few times over) as far as the gaps allow.
fn positions(
    rows: &[Vec<usize>],
    vertices: &[(usize, (f64, f64))],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    gap: impl Fn(usize, usize) -> f64,
) -> Vec<f64> {
    let width = |vertex: usize| vertices[vertex].1 .0;
    let mut left = vec![0.0; vertices.len()];
    for row in rows {
        let mut x = 0.0;
        for (index, &vertex) in row.iter().enumerate() {
            if index > 0 {
                x += gap(row[index - 1], vertex);
            }
            left[vertex] = x;
            x += width(vertex);
        }
    }
    for sweep in 0..8 {
        let (ranks, neighbours): (Vec<usize>, _) =
            if sweep % 2 == 0 { ((0..rows.len()).collect(), up) } else { ((0..rows.len()).rev().collect(), down) };
        for row in ranks.into_iter().map(|r| &rows[r]) {
            let wanted: Vec<f64> = row
                .iter()
                .map(|&vertex| {
                    let around = &neighbours[vertex];
                    let middle = if around.is_empty() {
                        left[vertex] + width(vertex) / 2.0
                    } else {
                        around.iter().map(|&other| left[other] + width(other) / 2.0).sum::<f64>() / around.len() as f64
                    };
                    middle - width(vertex) / 2.0
                })
                .collect();
            // As close as the gaps allow, pushed from the left and from the right; halfway
            // between the two keeps both the order and the gaps.
            let count = row.len();
            let mut from_left = wanted.clone();
            for index in 1..count {
                let least = from_left[index - 1] + width(row[index - 1]) + gap(row[index - 1], row[index]);
                from_left[index] = from_left[index].max(least);
            }
            let mut from_right = wanted;
            for index in (0..count.saturating_sub(1)).rev() {
                let most = from_right[index + 1] - gap(row[index], row[index + 1]) - width(row[index]);
                from_right[index] = from_right[index].min(most);
            }
            for index in 0..count {
                left[row[index]] = (from_left[index] + from_right[index]) / 2.0;
            }
        }
    }
    let leftmost = left.iter().copied().fold(f64::INFINITY, f64::min);
    left.iter().map(|x| x - leftmost + MARGIN).collect()
}

/// Where a line from the middle of a node towards `toward` crosses its outline.
fn outline_towards(shape: Shape, rect: Rect, toward: (f64, f64)) -> (f64, f64) {
    let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let (dx, dy) = (toward.0 - cx, toward.1 - cy);
    if dx == 0.0 && dy == 0.0 {
        return (cx, cy);
    }
    let (a, b) = (rect.width / 2.0, rect.height / 2.0);
    let reach = match shape {
        Shape::Circle => a.min(b) / dx.hypot(dy),
        Shape::Diamond => 1.0 / (dx.abs() / a + dy.abs() / b),
        // Close enough for the other shapes: their box.
        _ => (a / dx.abs()).min(b / dy.abs()),
    };
    (cx + dx * reach, cy + dy * reach)
}

/// An arrow from a node back to itself: a loop round its right side.
fn round_the_side(node: Rect) -> Route {
    let (right, middle) = (node.x + node.width, node.y + node.height / 2.0);
    let (above, below) = (middle - node.height / 4.0, middle + node.height / 4.0);
    let out = right + LOOP;
    Route { points: vec![(right, above), (out, above), (out, below), (right, below)], label: (out, middle) }
}

/// How far a node's loop to itself reaches out.
const LOOP: f64 = 20.0;

/// Which edges point back up a loop (each loop has one), so that ranks can follow the rest.
fn backward(chart: &Chart) -> Vec<bool> {
    let mut backward = vec![false; chart.edges.len()];
    // Depth first, in the chart's order: an edge to a node still being explored closes a loop.
    let (mut seen, mut exploring) = (vec![false; chart.nodes.len()], vec![false; chart.nodes.len()]);
    for root in 0..chart.nodes.len() {
        if seen[root] {
            continue;
        }
        let mut path = vec![(root, 0)];
        (seen[root], exploring[root]) = (true, true);
        while let Some((node, next)) = path.last_mut() {
            let node = *node;
            let Some((index, edge)) = chart.edges.iter().enumerate().filter(|(_, edge)| edge.from == node).nth(*next) else {
                exploring[node] = false;
                path.pop();
                continue;
            };
            *next += 1;
            if exploring[edge.to] {
                backward[index] = true;
            } else if !seen[edge.to] {
                (seen[edge.to], exploring[edge.to]) = (true, true);
                path.push((edge.to, 0));
            }
        }
    }
    backward
}

/// Each node's rank: below every node with an arrow to it (by the arrow's length), the arrows
/// closing loops turned round.
fn ranks(chart: &Chart, backward: &[bool]) -> Vec<usize> {
    // (upper, lower, length) for every edge but loops to the same node.
    let links: Vec<(usize, usize, usize)> = chart
        .edges
        .iter()
        .zip(backward)
        .filter(|(edge, _)| edge.from != edge.to)
        .map(|(edge, &backward)| if backward { (edge.to, edge.from, edge.length) } else { (edge.from, edge.to, edge.length) })
        .collect();
    let mut rank = vec![0; chart.nodes.len()];
    let mut incoming = vec![0; chart.nodes.len()];
    for &(_, lower, _) in &links {
        incoming[lower] += 1;
    }
    let mut ready: std::collections::VecDeque<usize> = (0..rank.len()).filter(|&node| incoming[node] == 0).collect();
    while let Some(node) = ready.pop_front() {
        for &(_, lower, length) in links.iter().filter(|link| link.0 == node) {
            rank[lower] = rank[lower].max(rank[node] + length);
            incoming[lower] -= 1;
            if incoming[lower] == 0 {
                ready.push_back(lower);
            }
        }
    }
    rank
}

impl Layout {
    /// This top-down layout, turned to run in `direction`.
    fn facing(mut self, direction: Direction) -> Layout {
        let (width, height) = (self.width, self.height);
        let place = |(x, y): (f64, f64)| match direction {
            Direction::Down => (x, y),
            Direction::Up => (x, height - y),
            Direction::Right => (y, x),
            Direction::Left => (height - y, x),
        };
        for rect in &mut self.nodes {
            let (a, b) = (place((rect.x, rect.y)), place((rect.x + rect.width, rect.y + rect.height)));
            *rect = Rect { x: a.0.min(b.0), y: a.1.min(b.1), width: (a.0 - b.0).abs(), height: (a.1 - b.1).abs() };
        }
        for route in &mut self.edges {
            route.points.iter_mut().for_each(|point| *point = place(*point));
            route.label = place(route.label);
        }
        if matches!(direction, Direction::Right | Direction::Left) {
            (self.width, self.height) = (height, width);
        }
        self
    }
}

/// Room inside a node around its label, and around an edge's label.
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 9.0;
/// Labels wider than this wrap.
const WRAP: i32 = 220;
/// Arrowhead length; slant of `[/ /]` sides; height of a cylinder's top and bottom.
const ARROW: f32 = 9.0;
const SLANT: f64 = 12.0;
const CAP: f32 = 6.0;
/// Wider charts are shown smaller.
const WIDEST: f64 = 900.0;

/// The chart drawn as a picture; `text` lays out a label in the read view's font.
pub fn picture(chart: &Chart, text: &dyn Fn(&str) -> pango::Layout) -> gtk::Picture {
    let labelled = |label: &str| {
        let layout = text(label);
        layout.set_alignment(pango::Alignment::Center);
        if layout.pixel_size().0 > WRAP {
            layout.set_wrap(pango::WrapMode::WordChar);
            layout.set_width(WRAP * pango::SCALE);
        }
        let (_, extents) = layout.pixel_extents();
        (layout, extents)
    };
    let nodes: Vec<_> = chart.nodes.iter().map(|node| labelled(&node.label)).collect();
    let edges: Vec<_> = chart.edges.iter().map(|edge| (!edge.label.is_empty()).then(|| labelled(&edge.label))).collect();
    let sizes: Vec<(f64, f64)> = chart
        .nodes
        .iter()
        .zip(&nodes)
        .map(|(node, (_, extents))| outer_size(node.shape, extents.width() as f64, extents.height() as f64))
        .collect();
    let label_sizes: Vec<(f64, f64)> = edges
        .iter()
        .map(|label| label.as_ref().map_or((0.0, 0.0), |(_, e)| (e.width() as f64 + 10.0, e.height() as f64 + 4.0)))
        .collect();
    let laid = layout(chart, &sizes, &label_sizes);

    let color = |hex: &str| gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::WHITE);
    let (line, border, fill, ink, ground) =
        (color(&theme::colors().dim.to_hex()), color(&theme::colors().marker.to_hex()), color(&theme::colors().quote_background.to_hex()), color(&theme::colors().text.to_hex()), color(&theme::colors().background.to_hex()));
    let snapshot = gtk::Snapshot::new();
    let write = |layout: &pango::Layout, extents: &pango::Rectangle, (x, y): (f64, f64), ink: &gdk::RGBA| {
        snapshot.save();
        let left = x - extents.width() as f64 / 2.0 - extents.x() as f64;
        let top = y - extents.height() as f64 / 2.0 - extents.y() as f64;
        snapshot.translate(&graphene::Point::new(left as f32, top as f32));
        snapshot.append_layout(layout, ink);
        snapshot.restore();
    };
    // Edges first, under the nodes they join.
    for (edge, route) in chart.edges.iter().zip(&laid.edges) {
        if edge.line != Line::Hidden {
            draw_edge(&snapshot, edge, &route.points, &line);
        }
    }
    for ((node, rect), (layout, extents)) in chart.nodes.iter().zip(&laid.nodes).zip(&nodes) {
        let outline = outline(node.shape, *rect);
        snapshot.append_fill(&outline, gsk::FillRule::Winding, &fill);
        snapshot.append_stroke(&outline, &gsk::Stroke::new(1.2), &border);
        write(layout, extents, (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0 + cylinder_drop(node.shape)), &ink);
    }
    // Edge labels last, on a patch of the background.
    for ((edge, route), label) in chart.edges.iter().zip(&laid.edges).zip(&edges) {
        if let (Some((layout, extents)), false) = (label, edge.line == Line::Hidden) {
            let (w, h) = (extents.width() as f32 + 10.0, extents.height() as f32 + 4.0);
            let (x, y) = (route.label.0 as f32, route.label.1 as f32);
            let patch = gsk::RoundedRect::from_rect(graphene::Rect::new(x - w / 2.0, y - h / 2.0, w, h), 4.0);
            snapshot.push_rounded_clip(&patch);
            snapshot.append_color(&ground, patch.bounds());
            snapshot.pop();
            write(layout, extents, route.label, &color(&theme::colors().quote.to_hex()));
        }
    }

    let picture = match snapshot.to_paintable(Some(&graphene::Size::new(laid.width as f32, laid.height as f32))) {
        Some(paintable) => gtk::Picture::for_paintable(&paintable),
        None => gtk::Picture::new(),
    };
    picture.set_can_shrink(true);
    let width = laid.width.min(WIDEST);
    picture.set_size_request(width as i32, (laid.height * width / laid.width.max(1.0)) as i32);
    picture.set_alternative_text(Some("Flowchart"));
    picture
}

/// A node's size around a label `width` × `height`.
fn outer_size(shape: Shape, width: f64, height: f64) -> (f64, f64) {
    let (w, h) = (width + 2.0 * PAD_X, height + 2.0 * PAD_Y);
    match shape {
        Shape::Box | Shape::Round => (w, h),
        Shape::Stadium => (w + h / 2.0, h),
        Shape::Subroutine => (w + 16.0, h),
        Shape::Cylinder => (w, h + 2.0 * f64::from(CAP)),
        Shape::Slanted => (w + 2.0 * SLANT, h),
        Shape::Flag => (w + 14.0, h),
        Shape::Hexagon => (w + h, h),
        // Round the label's box.
        Shape::Circle => {
            let diameter = width.hypot(height) + 2.0 * PAD_Y;
            (diameter.max(h), diameter.max(h))
        }
        // A rhombus whose half-diagonals a and b hold the label: w/2a + h/2b ≤ 1.
        Shape::Diamond => {
            let a = width / 2.0 + height + PAD_X;
            let b = (height * a / (2.0 * a - width)).max(height) + PAD_Y / 2.0;
            (2.0 * a, 2.0 * b)
        }
    }
}

/// How far a cylinder's label sits below its middle, under the top's ellipse.
fn cylinder_drop(shape: Shape) -> f64 {
    if shape == Shape::Cylinder { f64::from(CAP) / 2.0 } else { 0.0 }
}

/// A node's outline.
fn outline(shape: Shape, rect: Rect) -> gsk::Path {
    let (x, y, w, h) = (rect.x as f32, rect.y as f32, rect.width as f32, rect.height as f32);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let path = gsk::PathBuilder::new();
    let rounded = |radius: f32| gsk::RoundedRect::from_rect(graphene::Rect::new(x, y, w, h), radius);
    let polygon = |points: &[(f32, f32)]| {
        path.move_to(points[0].0, points[0].1);
        for &(px, py) in &points[1..] {
            path.line_to(px, py);
        }
        path.close();
    };
    match shape {
        Shape::Box => path.add_rounded_rect(&rounded(3.0)),
        Shape::Round => path.add_rounded_rect(&rounded(10.0)),
        Shape::Stadium => path.add_rounded_rect(&rounded(h / 2.0)),
        Shape::Subroutine => {
            path.add_rounded_rect(&rounded(3.0));
            for side in [x + 8.0, x + w - 8.0] {
                path.move_to(side, y);
                path.line_to(side, y + h);
            }
        }
        Shape::Circle => path.add_circle(&graphene::Point::new(cx, cy), w.min(h) / 2.0),
        Shape::Diamond => polygon(&[(cx, y), (x + w, cy), (cx, y + h), (x, cy)]),
        Shape::Hexagon => {
            let inset = h / 2.0;
            polygon(&[(x + inset, y), (x + w - inset, y), (x + w, cy), (x + w - inset, y + h), (x + inset, y + h), (x, cy)]);
        }
        Shape::Slanted => {
            let slant = SLANT as f32;
            polygon(&[(x + slant, y), (x + w, y), (x + w - slant, y + h), (x, y + h)]);
        }
        Shape::Flag => polygon(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x + 12.0, cy)]),
        Shape::Cylinder => {
            // Sides and bottom, the back of the top; then the front of the top.
            let quarter = std::f32::consts::FRAC_1_SQRT_2;
            path.move_to(x, y + CAP);
            path.line_to(x, y + h - CAP);
            path.conic_to(x, y + h, cx, y + h, quarter);
            path.conic_to(x + w, y + h, x + w, y + h - CAP, quarter);
            path.line_to(x + w, y + CAP);
            path.conic_to(x + w, y, cx, y, quarter);
            path.conic_to(x, y, x, y + CAP, quarter);
            path.close();
            path.move_to(x, y + CAP);
            path.conic_to(x, y + 2.0 * CAP, cx, y + 2.0 * CAP, quarter);
            path.conic_to(x + w, y + 2.0 * CAP, x + w, y + CAP, quarter);
        }
    }
    path.to_path()
}

/// An edge along `points`: rounded at its bends, stopping short of its arrowheads.
fn draw_edge(snapshot: &gtk::Snapshot, edge: &Edge, points: &[(f64, f64)], color: &gdk::RGBA) {
    let mut points: Vec<(f32, f32)> = points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
    let n = points.len();
    if n < 2 {
        return;
    }
    let (head, tail) = ((points[n - 1], points[n - 2]), (points[0], points[1]));
    if edge.head == End::Arrow {
        points[n - 1] = towards(head.0, head.1, ARROW);
    }
    if edge.tail == End::Arrow {
        points[0] = towards(tail.0, tail.1, ARROW);
    }
    let path = gsk::PathBuilder::new();
    path.move_to(points[0].0, points[0].1);
    for bend in points.windows(3) {
        let (before, at, after) = (bend[0], bend[1], bend[2]);
        let radius = 10.0f32.min(distance(before, at) / 2.0).min(distance(at, after) / 2.0);
        let (enter, leave) = (towards(at, before, radius), towards(at, after, radius));
        path.line_to(enter.0, enter.1);
        path.quad_to(at.0, at.1, leave.0, leave.1);
    }
    path.line_to(points[n - 1].0, points[n - 1].1);
    let stroke = gsk::Stroke::new(if edge.line == Line::Thick { 3.0 } else { 1.4 });
    stroke.set_line_join(gsk::LineJoin::Round);
    if edge.line == Line::Dotted {
        stroke.set_dash(&[3.0, 4.0]);
    }
    snapshot.append_stroke(&path.to_path(), &stroke, color);
    draw_end(snapshot, edge.head, head.0, head.1, color);
    draw_end(snapshot, edge.tail, tail.0, tail.1, color);
}

/// The mark at an edge's end `tip`, coming from `from`.
fn draw_end(snapshot: &gtk::Snapshot, end: End, tip: (f32, f32), from: (f32, f32), color: &gdk::RGBA) {
    let length = distance(tip, from).max(0.001);
    let (dx, dy) = ((tip.0 - from.0) / length, (tip.1 - from.1) / length);
    let path = gsk::PathBuilder::new();
    match end {
        End::Plain => {}
        End::Arrow => {
            let base = (tip.0 - dx * ARROW, tip.1 - dy * ARROW);
            let side = (-dy * ARROW / 2.0, dx * ARROW / 2.0);
            path.move_to(tip.0, tip.1);
            path.line_to(base.0 + side.0, base.1 + side.1);
            path.line_to(base.0 - side.0, base.1 - side.1);
            path.close();
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, color);
        }
        End::Circle => {
            path.add_circle(&graphene::Point::new(tip.0 - dx * 4.0, tip.1 - dy * 4.0), 4.0);
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, color);
        }
        End::Cross => {
            let (cx, cy, r) = (tip.0 - dx * 5.0, tip.1 - dy * 5.0, 4.0);
            path.move_to(cx - r, cy - r);
            path.line_to(cx + r, cy + r);
            path.move_to(cx + r, cy - r);
            path.line_to(cx - r, cy + r);
            snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(1.6), color);
        }
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The point `by` from `from` on the way to `to`.
fn towards(from: (f32, f32), to: (f32, f32), by: f32) -> (f32, f32) {
    let length = distance(from, to).max(0.001);
    (from.0 + (to.0 - from.0) * by / length, from.1 + (to.1 - from.1) * by / length)
}

/// The flowchart in a Mermaid block, or nothing when it is another kind of diagram.
pub fn parse(source: &str) -> Option<Chart> {
    let mut lines = source.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("%%"));
    // "graph LR", maybe followed by statements after a semicolon.
    let first = lines.next()?;
    let (header, rest) = first.split_once(';').unwrap_or((first, ""));
    let mut header = header.split_whitespace();
    if !matches!(header.next()?, "flowchart" | "graph") {
        return None;
    }
    let direction = match header.next() {
        Some("LR") => Direction::Right,
        Some("RL") => Direction::Left,
        Some("BT") => Direction::Up,
        _ => Direction::Down,
    };
    let mut chart = Chart { direction, nodes: Vec::new(), edges: Vec::new() };
    for statement in std::iter::once(rest).chain(lines).flat_map(statements) {
        // Styling, interaction and grouping: nothing to draw here (a group's nodes still are).
        let first = statement.split_whitespace().next().unwrap_or_default();
        let skipped = ["classDef", "class", "style", "linkStyle", "click", "subgraph", "end", "direction", "accTitle", "accDescr"];
        if !statement.starts_with("%%") && !skipped.contains(&first) {
            chart.statement(statement);
        }
    }
    Some(chart)
}

/// A line's statements: split at semicolons outside quotes and brackets.
fn statements(line: &str) -> impl Iterator<Item = &str> {
    let (mut parts, mut start, mut depth, mut quoted) = (Vec::new(), 0, 0i32, false);
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                parts.push(&line[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&line[start..]);
    parts.into_iter().map(str::trim).filter(|part| !part.is_empty())
}

impl Chart {
    /// Reads one line: nodes, and the links between them.
    fn statement(&mut self, line: &str) {
        let mut scan = Scanner { text: line, at: 0 };
        let mut previous = self.node_group(&mut scan);
        while !previous.is_empty() {
            scan.skip_spaces();
            let Some(link) = scan.link() else { break };
            scan.skip_spaces();
            let next = self.node_group(&mut scan);
            for &from in &previous {
                for &to in &next {
                    self.edges.push(Edge { from, to, ..link.clone() });
                }
            }
            previous = next;
        }
    }

    /// One node, or several joined by `&`; their indices.
    fn node_group(&mut self, scan: &mut Scanner) -> Vec<usize> {
        let mut group = Vec::new();
        while let Some(node) = self.node(scan) {
            group.push(node);
            scan.skip_spaces();
            if !scan.eat("&") {
                break;
            }
            scan.skip_spaces();
        }
        group
    }

    /// A node's id and maybe its shape and label; its index (added if new).
    fn node(&mut self, scan: &mut Scanner) -> Option<usize> {
        let id = scan.take_while(|c| c.is_alphanumeric() || c == '_');
        if id.is_empty() {
            return None;
        }
        let shaped = scan.shape();
        // A class to style it with: ":::name".
        if scan.eat(":::") {
            scan.take_while(|c| c.is_alphanumeric() || c == '_' || c == '-');
        }
        if let Some(index) = self.nodes.iter().position(|node| node.id == id) {
            if let Some((shape, label)) = shaped {
                self.nodes[index].shape = shape;
                self.nodes[index].label = label;
            }
            return Some(index);
        }
        let (shape, label) = shaped.unwrap_or_else(|| (Shape::Box, id.to_owned()));
        self.nodes.push(Node { id: id.to_owned(), label, shape });
        Some(self.nodes.len() - 1)
    }
}

/// Reads a statement from left to right.
struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn eat(&mut self, prefix: &str) -> bool {
        let found = self.rest().starts_with(prefix);
        if found {
            self.at += prefix.len();
        }
        found
    }

    fn skip_spaces(&mut self) {
        self.take_while(char::is_whitespace);
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &'a str {
        let rest = self.rest();
        let len = rest.find(|c| !keep(c)).unwrap_or(rest.len());
        self.at += len;
        &rest[..len]
    }

    /// The shape and label right after a node's id, if it has them.
    fn shape(&mut self) -> Option<(Shape, String)> {
        // Longer openings first: "((" before "(".
        const SHAPES: [(&str, &[&str], Shape); 12] = [
            ("(((", &[")))"], Shape::Circle),
            ("((", &["))"], Shape::Circle),
            ("([", &["])"], Shape::Stadium),
            ("(", &[")"], Shape::Round),
            ("[[", &["]]"], Shape::Subroutine),
            ("[(", &[")]"], Shape::Cylinder),
            ("[/", &["/]", "\\]"], Shape::Slanted),
            ("[\\", &["\\]", "/]"], Shape::Slanted),
            ("[", &["]"], Shape::Box),
            ("{{", &["}}"], Shape::Hexagon),
            ("{", &["}"], Shape::Diamond),
            (">", &["]"], Shape::Flag),
        ];
        let (open, closes, shape) = SHAPES.iter().find(|(open, _, _)| self.rest().starts_with(open))?;
        let start = self.at + open.len();
        let inside = &self.text[start..];
        // A quoted label may hold the closing brackets.
        let after_quote = inside.strip_prefix('"').map_or(0, |rest| rest.find('"').map_or(0, |end| end + 2));
        let (end, close) = closes
            .iter()
            .filter_map(|close| inside[after_quote..].find(close).map(|end| (after_quote + end, close)))
            .min()?;
        self.at = start + end + close.len();
        Some((*shape, label(&inside[..end])))
    }

    /// The link at the cursor, in any of Mermaid's spellings: `-->`, `-.->`, `==>`, `---`,
    /// `--o`, `<-->`, `~~~`, `-- text -->`, `-->|text|` …
    fn link(&mut self) -> Option<Edge> {
        let start = self.at;
        let tail = self.end(true);
        let mut body = self.take_while(|c| matches!(c, '-' | '.' | '=' | '~'));
        if body.len() < 2 {
            self.at = start;
            return None;
        }
        let line = match body {
            _ if body.contains('~') => Line::Hidden,
            _ if body.contains('=') => Line::Thick,
            _ if body.contains('.') => Line::Dotted,
            _ => Line::Solid,
        };
        let mut head = self.end(false);
        let mut label = String::new();
        // Text inside the link: "-- text -->", "-. text .->", "== text ==>".
        let closing = match body {
            "--" => Some("--"),
            "-." => Some(".-"),
            "==" => Some("=="),
            _ => None,
        };
        if let Some(closing) = closing.filter(|_| head == End::Plain && self.rest().starts_with(char::is_whitespace))
            && let Some(end) = self.rest().find(closing)
        {
            label = self.rest()[..end].trim().to_owned();
            self.at += end;
            body = self.take_while(|c| matches!(c, '-' | '.' | '='));
            head = self.end(false);
        }
        // Text after the arrow: "-->|text|".
        if self.eat("|")
            && let Some(end) = self.rest().find('|')
        {
            label = self.rest()[..end].trim().to_owned();
            self.at += end + 1;
        }
        // Longer arrows span more ranks: "-->" one, "--->" two; dotted ones count their dots.
        let length = match line {
            Line::Dotted => body.matches('.').count(),
            _ if head == End::Plain => body.len().saturating_sub(2),
            _ => body.len() - 1,
        };
        Some(Edge { from: 0, to: 0, label: self::label(&label), line, tail, head, length: length.max(1) })
    }

    /// An end mark at the cursor: before the line (`tail`: `<`, `o`, `x`) or after it (`>`,
    /// and `o` or `x` standing alone, not starting a name).
    fn end(&mut self, tail: bool) -> End {
        let mut chars = self.rest().chars();
        let (mark, next) = (chars.next(), chars.next());
        let end = match (mark, tail) {
            (Some('<'), true) | (Some('>'), false) => End::Arrow,
            (Some('o'), _) => End::Circle,
            (Some('x'), _) => End::Cross,
            _ => return End::Plain,
        };
        let alone = match (end, tail) {
            (End::Arrow, _) => true,
            (_, true) => next.is_some_and(|c| matches!(c, '-' | '.' | '=')),
            (_, false) => next.is_none_or(|c| c.is_whitespace() || c == '|'),
        };
        if !alone {
            return End::Plain;
        }
        self.at += 1;
        end
    }
}

/// A label as it shows: without its quotes, `<br>` as a new line.
fn label(raw: &str) -> String {
    let text = raw.trim();
    let text = text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(text);
    let text = text.strip_prefix('`').and_then(|t| t.strip_suffix('`')).unwrap_or(text);
    text.replace("<br/>", "\n").replace("<br />", "\n").replace("<br>", "\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_says_flowchart_and_which_way() {
        assert_eq!(parse("graph LR\n").map(|c| c.direction), Some(Direction::Right));
        assert_eq!(parse("%% a comment\nflowchart TD").map(|c| c.direction), Some(Direction::Down));
        assert_eq!(parse("flowchart").map(|c| c.direction), Some(Direction::Down));
        assert_eq!(parse("graph BT").map(|c| c.direction), Some(Direction::Up));
        assert_eq!(parse("flowchart RL").map(|c| c.direction), Some(Direction::Left));
        assert_eq!(parse("sequenceDiagram\n  A->>B: hi"), None);
    }

    fn node(id: &str, label: &str, shape: Shape) -> Node {
        Node { id: id.to_owned(), label: label.to_owned(), shape }
    }

    fn pairs(chart: &Chart) -> Vec<(usize, usize)> {
        chart.edges.iter().map(|edge| (edge.from, edge.to)).collect()
    }

    #[test]
    fn nodes_take_label_and_shape_from_their_brackets() {
        let chart = parse(
            "flowchart TD
            A[Start] --> B(Round) --> C{Decide?}
            D((Hub)) --- E([Pill])
            F[[Sub]] --> G[(Store)] --> H{{Prepare}} --> I[/Input/] --> J>Flag]
            K",
        )
        .unwrap();
        assert_eq!(
            chart.nodes,
            [
                node("A", "Start", Shape::Box),
                node("B", "Round", Shape::Round),
                node("C", "Decide?", Shape::Diamond),
                node("D", "Hub", Shape::Circle),
                node("E", "Pill", Shape::Stadium),
                node("F", "Sub", Shape::Subroutine),
                node("G", "Store", Shape::Cylinder),
                node("H", "Prepare", Shape::Hexagon),
                node("I", "Input", Shape::Slanted),
                node("J", "Flag", Shape::Flag),
                node("K", "K", Shape::Box),
            ]
        );
        // A chain links each neighbouring pair.
        assert_eq!(pairs(&chart), [(0, 1), (1, 2), (3, 4), (5, 6), (6, 7), (7, 8), (8, 9)]);
    }

    #[test]
    fn links_carry_their_line_ends_label_and_length() {
        let chart = parse(
            "graph LR
            A -- yes --> B
            A -->|no| C
            A -.-> D
            A ==> E
            A --- F
            A --o G
            A <--> H
            A -. maybe .-> I
            A == sure ==> J
            A ---> K
            A ~~~ L
            A --x M",
        )
        .unwrap();
        let links: Vec<_> =
            chart.edges.iter().map(|edge| (edge.label.as_str(), edge.line, edge.tail, edge.head, edge.length)).collect();
        use End::*;
        use Line::*;
        assert_eq!(
            links,
            [
                ("yes", Solid, Plain, Arrow, 1),
                ("no", Solid, Plain, Arrow, 1),
                ("", Dotted, Plain, Arrow, 1),
                ("", Thick, Plain, Arrow, 1),
                ("", Solid, Plain, Plain, 1),
                ("", Solid, Plain, Circle, 1),
                ("", Solid, Arrow, Arrow, 1),
                ("maybe", Dotted, Plain, Arrow, 1),
                ("sure", Thick, Plain, Arrow, 1),
                ("", Solid, Plain, Arrow, 2),
                ("", Hidden, Plain, Plain, 1),
                ("", Solid, Plain, Cross, 1),
            ]
        );
    }

    #[test]
    fn groups_semicolons_quotes_and_lines_that_draw_nothing() {
        let chart = parse(
            "flowchart TD;
            A & B --> C & D
            subgraph one [The first]
              C --> E[\"Hello (world) [x]\"];  E --> F[\"خط اول<br>خط دوم\"]
            end
            classDef warm fill:#f96
            style A fill:#333
            click A callback
            %% a comment
            G:::warm --> H
            H[Later label]",
        )
        .unwrap();
        assert_eq!(pairs(&chart), [(0, 2), (0, 3), (1, 2), (1, 3), (2, 4), (4, 5), (6, 7)]);
        assert_eq!(chart.nodes[4].label, "Hello (world) [x]");
        assert_eq!(chart.nodes[5].label, "خط اول\nخط دوم");
        // A label given later still counts.
        assert_eq!(chart.nodes[7], node("H", "Later label", Shape::Box));
        assert_eq!(chart.nodes.len(), 8);
        // A whole chart on one line.
        assert_eq!(pairs(&parse("graph LR; A-->B; B-->C").unwrap()), [(0, 1), (1, 2)]);
    }

    /// `chart` laid out with every node 80×40 and labels 50×20.
    fn laid_out(chart: &Chart) -> Layout {
        let labels: Vec<_> = chart.edges.iter().map(|e| if e.label.is_empty() { (0.0, 0.0) } else { (50.0, 20.0) }).collect();
        layout(chart, &vec![(80.0, 40.0); chart.nodes.len()], &labels)
    }

    #[test]
    fn ranks_follow_the_arrows_whichever_way_they_point() {
        let down = laid_out(&parse("graph TD\nA --> B --> C\nA --> D").unwrap());
        let [a, b, c, d] = [0, 1, 2, 3].map(|i| down.nodes[i]);
        assert!(a.y + a.height < b.y && b.y + b.height < c.y);
        // Both of A's arrows end on the same rank.
        assert_eq!(b.y, d.y);

        let right = laid_out(&parse("graph LR\nA --> B").unwrap());
        assert!(right.nodes[0].x + right.nodes[0].width < right.nodes[1].x);
        let up = laid_out(&parse("graph BT\nA --> B").unwrap());
        assert!(up.nodes[1].y + up.nodes[1].height < up.nodes[0].y);
        let left = laid_out(&parse("graph RL\nA --> B").unwrap());
        assert!(left.nodes[1].x + left.nodes[1].width < left.nodes[0].x);
        // Sizes stay as given, whichever the direction.
        assert_eq!((left.nodes[0].width, left.nodes[0].height), (80.0, 40.0));
    }

    fn inside(layout: &Layout, (x, y): (f64, f64)) -> bool {
        (0.0..=layout.width).contains(&x) && (0.0..=layout.height).contains(&y)
    }

    #[test]
    fn cycles_are_laid_out_like_the_rest() {
        let laid = laid_out(&parse("graph TD\nA --> B --> C --> A\nC --> C").unwrap());
        let [a, b, c] = [0, 1, 2].map(|i| laid.nodes[i]);
        // The arrow closing the loop is the one pointing back up.
        assert!(a.y + a.height < b.y && b.y + b.height < c.y);
        for route in &laid.edges {
            assert!(route.points.len() >= 2 && route.points.iter().all(|&point| inside(&laid, point)));
        }
        // C's arrow to itself goes round its right side.
        assert!(laid.edges[3].points.iter().all(|&(x, _)| x >= c.x + c.width));
    }

    #[test]
    fn long_arrows_pass_the_ranks_between_beside_their_nodes() {
        let laid = laid_out(&parse("graph TD\nA --> B --> C\nA --> C").unwrap());
        let b = laid.nodes[1];
        // A to C bends at B's rank, to one side of B.
        let bend = laid.edges[2].points.iter().find(|&&(_, y)| (b.y..=b.y + b.height).contains(&y));
        assert!(bend.is_some_and(|&(x, _)| !(b.x..=b.x + b.width).contains(&x)));
    }

    #[test]
    fn labels_get_room_between_the_ranks() {
        // The 50×20 label fits between A and B, along the arrow either way.
        let down = laid_out(&parse("graph TD\nA -- label --> B").unwrap());
        let (a, b, (_, y)) = (down.nodes[0], down.nodes[1], down.edges[0].label);
        assert!(a.y + a.height <= y - 10.0 && y + 10.0 <= b.y);
        let right = laid_out(&parse("graph LR\nA -- label --> B").unwrap());
        let (a, b, (x, _)) = (right.nodes[0], right.nodes[1], right.edges[0].label);
        assert!(a.x + a.width <= x - 25.0 && x + 25.0 <= b.x);
    }

    #[test]
    fn nothing_overlaps_in_a_busy_chart() {
        let source = "A --> B & C & D\nB --> E\nC --> E & F\nD --> F --> G\nA --> G\nE -- back --> A\nG --> H & I & J";
        for direction in ["TD", "LR"] {
            let laid = laid_out(&parse(&format!("graph {direction}\n{source}")).unwrap());
            for (index, a) in laid.nodes.iter().enumerate() {
                assert!(inside(&laid, (a.x, a.y)) && inside(&laid, (a.x + a.width, a.y + a.height)));
                for b in &laid.nodes[index + 1..] {
                    let apart = a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y;
                    assert!(apart, "{direction}: {a:?} overlaps {b:?}");
                }
            }
        }
    }
}

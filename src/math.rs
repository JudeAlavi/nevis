//! Formulas (`$…$`, `$$…$$`) as text: LaTeX turned into Unicode, with raised and lowered
//! runs for superscripts and subscripts. Light on purpose: nothing is stacked.

/// A stretch of a formula and where it sits: `rise` in fifths of an em (a superscript is 2,
/// a subscript −1), `depth` how many scripts deep it is (each level smaller).
#[derive(Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub rise: i8,
    pub depth: u8,
}

/// How `latex` (what is between the dollar signs) is shown.
pub fn runs(latex: &str) -> Vec<Run> {
    let atoms = Parser { src: latex, at: 0 }.atoms(None);
    let mut out = Vec::new();
    write(&atoms, 0, 0, &mut out);
    out
}

/// How an atom is spaced from its neighbours, as in TeX.
#[derive(Clone, Copy, PartialEq)]
enum Class {
    Ord,
    /// A function name or a big operator: sin, ∑.
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

/// A piece of a formula, before it is written out.
enum Atom {
    /// One thing: a letter, a number, a symbol.
    Text(String, Class),
    /// What was in braces.
    Group(Vec<Atom>),
    /// An atom with a subscript and a superscript, either maybe empty.
    Scripts { base: Box<Atom>, sub: Vec<Atom>, sup: Vec<Atom> },
    Fraction(Vec<Atom>, Vec<Atom>),
    /// A root, maybe with an index (√ when there's none).
    Root(Vec<Atom>, Vec<Atom>),
    /// A combining mark over the last character, or over `each` one.
    Accent { mark: char, each: bool, over: Vec<Atom> },
    /// A space command: it shows, but is not an atom to space from.
    Space(&'static str),
    /// The end of a row (`\\`).
    Newline,
}

impl Atom {
    fn class(&self) -> Class {
        match self {
            Atom::Text(_, class) => *class,
            Atom::Scripts { base, .. } => base.class(),
            Atom::Group(_) | Atom::Fraction(..) | Atom::Root(..) | Atom::Accent { .. } | Atom::Space(_)
            | Atom::Newline => Class::Ord,
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.at..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }

    /// The atoms up to the end, or up to `close` (a closing brace or bracket).
    fn atoms(&mut self, close: Option<char>) -> Vec<Atom> {
        let mut atoms = Vec::new();
        while let Some(c) = self.bump() {
            match c {
                c if Some(c) == close => break,
                '^' | '_' => {
                    let script = self.argument();
                    let base = atoms.pop().unwrap_or(Atom::Text(String::new(), Class::Ord));
                    let (base, mut sub, mut sup) = match base {
                        Atom::Scripts { base, sub, sup } => (base, sub, sup),
                        base => (Box::new(base), Vec::new(), Vec::new()),
                    };
                    if c == '^' { sup.extend(script) } else { sub.extend(script) }
                    atoms.push(Atom::Scripts { base, sub, sup });
                }
                c => atoms.extend(self.atom(c)),
            }
        }
        atoms
    }

    /// What a script applies: the braced group, or the one character or command that follows.
    fn argument(&mut self) -> Vec<Atom> {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        match self.bump() {
            Some('{') => self.atoms(Some('}')),
            Some(c @ '0'..='9') => vec![Atom::Text(c.to_string(), Class::Ord)],
            Some(c) => self.atom(c).into_iter().collect(),
            None => Vec::new(),
        }
    }

    /// The atom starting with `c` (just read), if it shows at all.
    fn atom(&mut self, c: char) -> Option<Atom> {
        let atom = match c {
            // Spaces typed in a formula don't show; spacing comes from what's around.
            c if c.is_whitespace() => return None,
            '{' => Atom::Group(self.atoms(Some('}'))),
            '}' => return None,
            '0'..='9' => {
                let start = self.at - 1;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.bump();
                }
                Atom::Text(self.src[start..self.at].to_owned(), Class::Ord)
            }
            'a'..='z' | 'A'..='Z' => Atom::Text(italic(c).to_string(), Class::Ord),
            '\\' => return self.command_atom(),
            '~' => Atom::Space("\u{A0}"),
            // Cells of a matrix or of aligned rows.
            '&' => Atom::Space("\u{2003}"),
            '+' => Atom::Text("+".to_owned(), Class::Bin),
            '-' => Atom::Text("−".to_owned(), Class::Bin),
            '*' => Atom::Text("∗".to_owned(), Class::Bin),
            '=' | '<' | '>' | ':' => Atom::Text(c.to_string(), Class::Rel),
            '(' | '[' => Atom::Text(c.to_string(), Class::Open),
            ')' | ']' => Atom::Text(c.to_string(), Class::Close),
            ',' | ';' => Atom::Text(c.to_string(), Class::Punct),
            '\'' => Atom::Text("′".to_owned(), Class::Ord),
            c => Atom::Text(c.to_string(), Class::Ord),
        };
        Some(atom)
    }

    /// The atom for the command whose backslash was just read, if it shows at all.
    fn command_atom(&mut self) -> Option<Atom> {
        let atom = match self.command() {
                "frac" | "dfrac" | "tfrac" | "cfrac" => Atom::Fraction(self.argument(), self.argument()),
                "sqrt" => {
                    let index = if self.peek() == Some('[') {
                        self.bump();
                        self.atoms(Some(']'))
                    } else {
                        Vec::new()
                    };
                    Atom::Root(index, self.argument())
                }
                // Words, as typed.
                "text" | "textrm" | "textit" | "textbf" | "textsf" | "texttt" | "mbox" => {
                    Atom::Text(self.raw_argument().to_owned(), Class::Ord)
                }
                "operatorname" => Atom::Text(self.raw_argument().split_whitespace().collect(), Class::Op),
                font @ ("mathrm" | "mathit" | "mathbf" | "boldsymbol" | "mathbb" | "mathcal" | "mathscr"
                | "mathfrak" | "mathsf" | "mathtt") => {
                    let text = self.raw_argument().chars().filter(|c| !c.is_whitespace()).map(|c| letter(c, font)).collect();
                    Atom::Text(text, Class::Ord)
                }
            // Accents: combining marks.
            "vec" => self.accent('\u{20D7}', false),
            "hat" | "widehat" => self.accent('\u{302}', false),
            "bar" => self.accent('\u{304}', false),
            "tilde" | "widetilde" => self.accent('\u{303}', false),
            "dot" => self.accent('\u{307}', false),
            "ddot" => self.accent('\u{308}', false),
            "check" => self.accent('\u{30C}', false),
            "breve" => self.accent('\u{306}', false),
            "acute" => self.accent('\u{301}', false),
            "grave" => self.accent('\u{300}', false),
            "overline" => self.accent('\u{305}', true),
            "underline" => self.accent('\u{332}', true),
            // Sized delimiters: only the delimiter shows.
            "left" | "bigl" | "Bigl" | "biggl" | "Biggl" => return self.delimiter(Class::Open),
            "right" | "bigr" | "Bigr" | "biggr" | "Biggr" => return self.delimiter(Class::Close),
            "middle" | "big" | "Big" | "bigg" | "Bigg" => return self.delimiter(Class::Ord),
            // Spaces.
            "," | "thinspace" => Atom::Space("\u{2009}"),
            ":" | ">" | ";" => Atom::Space("\u{2005}"),
            "enspace" => Atom::Space("\u{2002}"),
            "quad" => Atom::Space("\u{2003}"),
            "qquad" => Atom::Space("\u{2003}\u{2003}"),
            " " => Atom::Space(" "),
            "\\" => Atom::Newline,
            // Environments: rows of cells, a matrix in its brackets.
            "begin" => {
                let name = self.raw_argument();
                if name == "array" {
                    self.raw_argument();
                }
                let open = match name {
                    "pmatrix" => "(",
                    "bmatrix" => "[",
                    "Bmatrix" | "cases" => "{",
                    "vmatrix" => "|",
                    "Vmatrix" => "‖",
                    _ => return None,
                };
                Atom::Text(open.to_owned(), Class::Open)
            }
            "end" => {
                let close = match self.raw_argument() {
                    "pmatrix" => ")",
                    "bmatrix" => "]",
                    "Bmatrix" => "}",
                    "vmatrix" => "|",
                    "Vmatrix" => "‖",
                    _ => return None,
                };
                Atom::Text(close.to_owned(), Class::Close)
            }
            // Styles and sizes that make no difference here.
            "!" | "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" | "limits" | "nolimits"
            | "nonumber" | "notag" => return None,
            name => match symbol(name) {
                Some((text, class)) => Atom::Text(text.to_owned(), class),
                None => Atom::Text(format!("\\{name}"), Class::Ord),
            },
        };
        Some(atom)
    }

    fn accent(&mut self, mark: char, each: bool) -> Atom {
        Atom::Accent { mark, each, over: self.argument() }
    }

    /// The delimiter after \left, \right and the like (none for "."), as `class`.
    fn delimiter(&mut self, class: Class) -> Option<Atom> {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        let text = match self.bump()? {
            '.' => return None,
            '\\' => symbol(self.command())?.0.to_owned(),
            c => c.to_string(),
        };
        Some(Atom::Text(text, class))
    }

    /// The source of the braced group (or the one character) that follows.
    fn raw_argument(&mut self) -> &'a str {
        let src = self.src;
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        if self.peek() != Some('{') {
            let start = self.at;
            self.bump();
            return &src[start..self.at];
        }
        self.bump();
        let (start, mut open) = (self.at, 1);
        while let Some(c) = self.bump() {
            open += match c {
                '{' => 1,
                '}' => -1,
                _ => 0,
            };
            if open == 0 {
                return &src[start..self.at - 1];
            }
        }
        &src[start..]
    }

    /// The name of the command whose backslash was just read: its letters, or one other
    /// character (`\{`, `\,`).
    fn command(&mut self) -> &'a str {
        let (src, start) = (self.src, self.at);
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.bump();
        }
        if self.at == start {
            self.bump();
        }
        &src[start..self.at]
    }
}

/// Writes `atoms` at `rise` and `depth` (see `Run`).
fn write(atoms: &[Atom], rise: i8, depth: u8, out: &mut Vec<Run>) {
    let mut before: Option<Class> = None;
    for atom in atoms {
        match atom {
            Atom::Space(space) => {
                push(out, space, rise, depth);
                continue;
            }
            // A new row starts afresh.
            Atom::Newline => {
                push(out, "\n", rise, depth);
                before = None;
                continue;
            }
            _ => {}
        }
        // An operation sign with nothing to its left is a sign: −1, (−x), x = −y.
        let class = match (atom.class(), before) {
            (Class::Bin, None | Some(Class::Bin | Class::Rel | Class::Open | Class::Punct | Class::Op)) => Class::Ord,
            (class, _) => class,
        };
        if depth == 0 && before.is_some_and(|before| spaced(before, class)) {
            push(out, " ", rise, depth);
        }
        write_atom(atom, rise, depth, out);
        before = Some(class);
    }
}

fn write_atom(atom: &Atom, rise: i8, depth: u8, out: &mut Vec<Run>) {
    match atom {
        Atom::Text(text, _) => push(out, text, rise, depth),
        Atom::Group(atoms) => write(atoms, rise, depth, out),
        Atom::Scripts { base, sub, sup } => {
            write_atom(base, rise, depth, out);
            write(sub, rise - 1, depth + 1, out);
            write(sup, rise + 2, depth + 1, out);
        }
        Atom::Fraction(numerator, denominator) => {
            write_part(numerator, rise, depth, out);
            push(out, "/", rise, depth);
            write_part(denominator, rise, depth, out);
        }
        Atom::Accent { mark, each, over } => {
            let mut marked = Vec::new();
            write(over, rise, depth, &mut marked);
            if *each {
                for run in &mut marked {
                    run.text = run.text.chars().flat_map(|c| [c, *mark]).collect();
                }
            } else if let Some(last) = marked.last_mut() {
                last.text.push(*mark);
            }
            for run in marked {
                push(out, &run.text, run.rise, run.depth);
            }
        }
        Atom::Space(space) => push(out, space, rise, depth),
        Atom::Newline => push(out, "\n", rise, depth),
        Atom::Root(index, body) => {
            match plain_index(index) {
                Some("3") => push(out, "∛", rise, depth),
                Some("4") => push(out, "∜", rise, depth),
                _ => {
                    write(index, rise + 2, depth + 1, out);
                    push(out, "√", rise, depth);
                }
            }
            write_part(body, rise, depth, out);
        }
    }
}

/// Writes part of a fraction or root, in parentheses when it is more than one thing.
fn write_part(atoms: &[Atom], rise: i8, depth: u8, out: &mut Vec<Run>) {
    if single(atoms) {
        write(atoms, rise, depth, out);
    } else {
        push(out, "(", rise, depth);
        write(atoms, rise, depth, out);
        push(out, ")", rise, depth);
    }
}

/// Whether `atoms` are one thing: a letter, a number, a symbol with its scripts, a fraction.
fn single(atoms: &[Atom]) -> bool {
    match atoms {
        [Atom::Group(inner)] => single(inner),
        [_] => true,
        _ => false,
    }
}

/// A root's index when it is a single piece of text, like 3.
fn plain_index(index: &[Atom]) -> Option<&str> {
    match index {
        [Atom::Text(text, _)] => Some(text),
        _ => None,
    }
}

/// Whether a space goes between atoms of these classes (scripts are never spaced).
fn spaced(before: Class, after: Class) -> bool {
    use Class::*;
    match (before, after) {
        (Rel, Rel) | (Open, _) | (_, Close | Punct) => false,
        (_, Rel | Bin) | (Rel | Bin, _) | (Punct, _) => true,
        (Ord | Close, Op) | (Op, Ord | Op) => true,
        _ => false,
    }
}

/// Adds `text` to the last run when it sits at the same place, else starts a run.
fn push(out: &mut Vec<Run>, text: &str, rise: i8, depth: u8) {
    match out.last_mut() {
        Some(last) if last.rise == rise && last.depth == depth => last.text.push_str(text),
        _ => out.push(Run { text: text.to_owned(), rise, depth }),
    }
}

/// What a command without arguments shows, and how it is spaced.
fn symbol(name: &str) -> Option<(&'static str, Class)> {
    use Class::*;
    let symbol = match name {
        // Greek
        "alpha" => ("α", Ord), "beta" => ("β", Ord), "gamma" => ("γ", Ord), "delta" => ("δ", Ord),
        "epsilon" => ("ϵ", Ord), "varepsilon" => ("ε", Ord), "zeta" => ("ζ", Ord), "eta" => ("η", Ord),
        "theta" => ("θ", Ord), "vartheta" => ("ϑ", Ord), "iota" => ("ι", Ord), "kappa" => ("κ", Ord),
        "lambda" => ("λ", Ord), "mu" => ("μ", Ord), "nu" => ("ν", Ord), "xi" => ("ξ", Ord),
        "pi" => ("π", Ord), "varpi" => ("ϖ", Ord), "rho" => ("ρ", Ord), "varrho" => ("ϱ", Ord),
        "sigma" => ("σ", Ord), "varsigma" => ("ς", Ord), "tau" => ("τ", Ord), "upsilon" => ("υ", Ord),
        "phi" => ("ϕ", Ord), "varphi" => ("φ", Ord), "chi" => ("χ", Ord), "psi" => ("ψ", Ord),
        "omega" => ("ω", Ord), "Gamma" => ("Γ", Ord), "Delta" => ("Δ", Ord), "Theta" => ("Θ", Ord),
        "Lambda" => ("Λ", Ord), "Xi" => ("Ξ", Ord), "Pi" => ("Π", Ord), "Sigma" => ("Σ", Ord),
        "Upsilon" => ("Υ", Ord), "Phi" => ("Φ", Ord), "Psi" => ("Ψ", Ord), "Omega" => ("Ω", Ord),
        // Other symbols
        "infty" => ("∞", Ord), "partial" => ("∂", Ord), "nabla" => ("∇", Ord), "emptyset" => ("∅", Ord),
        "varnothing" => ("∅", Ord), "hbar" => ("ℏ", Ord), "ell" => ("ℓ", Ord), "Re" => ("ℜ", Ord),
        "Im" => ("ℑ", Ord), "aleph" => ("ℵ", Ord), "forall" => ("∀", Ord), "exists" => ("∃", Ord),
        "nexists" => ("∄", Ord), "neg" | "lnot" => ("¬", Ord), "prime" => ("′", Ord), "angle" => ("∠", Ord),
        "triangle" => ("△", Ord), "degree" => ("°", Ord), "ldots" | "dots" => ("…", Ord),
        "cdots" => ("⋯", Ord), "vdots" => ("⋮", Ord), "ddots" => ("⋱", Ord), "top" => ("⊤", Ord),
        "bot" => ("⊥", Ord), "backslash" => ("\\", Ord), "%" => ("%", Ord), "#" => ("#", Ord),
        "&" => ("&", Ord), "$" => ("$", Ord), "_" => ("_", Ord), "|" => ("‖", Ord),
        // Operations
        "pm" => ("±", Bin), "mp" => ("∓", Bin), "times" => ("×", Bin), "div" => ("÷", Bin),
        "cdot" => ("⋅", Bin), "ast" => ("∗", Bin), "star" => ("⋆", Bin), "circ" => ("∘", Bin),
        "bullet" => ("∙", Bin), "cap" => ("∩", Bin), "cup" => ("∪", Bin), "setminus" => ("∖", Bin),
        "oplus" => ("⊕", Bin), "ominus" => ("⊖", Bin), "otimes" => ("⊗", Bin), "odot" => ("⊙", Bin),
        "wedge" | "land" => ("∧", Bin), "vee" | "lor" => ("∨", Bin),
        // Relations
        "le" | "leq" => ("≤", Rel), "ge" | "geq" => ("≥", Rel), "ne" | "neq" => ("≠", Rel),
        "approx" => ("≈", Rel), "equiv" => ("≡", Rel), "sim" => ("∼", Rel), "simeq" => ("≃", Rel),
        "cong" => ("≅", Rel), "propto" => ("∝", Rel), "ll" => ("≪", Rel), "gg" => ("≫", Rel),
        "in" => ("∈", Rel), "notin" => ("∉", Rel), "ni" => ("∋", Rel), "subset" => ("⊂", Rel),
        "subseteq" => ("⊆", Rel), "supset" => ("⊃", Rel), "supseteq" => ("⊇", Rel),
        "to" | "rightarrow" => ("→", Rel), "leftarrow" | "gets" => ("←", Rel),
        "leftrightarrow" => ("↔", Rel), "Rightarrow" => ("⇒", Rel), "Leftarrow" => ("⇐", Rel),
        "Leftrightarrow" => ("⇔", Rel), "implies" => ("⟹", Rel), "impliedby" => ("⟸", Rel),
        "iff" => ("⟺", Rel), "mapsto" => ("↦", Rel), "perp" => ("⊥", Rel), "parallel" => ("∥", Rel),
        "mid" => ("∣", Rel), "models" => ("⊨", Rel), "vdash" => ("⊢", Rel), "coloneqq" => ("≔", Rel),
        // Big operators
        "sum" => ("∑", Op), "prod" => ("∏", Op), "coprod" => ("∐", Op), "int" => ("∫", Op),
        "iint" => ("∬", Op), "iiint" => ("∭", Op), "oint" => ("∮", Op), "bigcup" => ("⋃", Op),
        "bigcap" => ("⋂", Op), "bigoplus" => ("⨁", Op), "bigotimes" => ("⨂", Op),
        // Function names, upright
        "sin" => ("sin", Op), "cos" => ("cos", Op), "tan" => ("tan", Op), "cot" => ("cot", Op),
        "sec" => ("sec", Op), "csc" => ("csc", Op), "arcsin" => ("arcsin", Op), "arccos" => ("arccos", Op),
        "arctan" => ("arctan", Op), "sinh" => ("sinh", Op), "cosh" => ("cosh", Op), "tanh" => ("tanh", Op),
        "log" => ("log", Op), "ln" => ("ln", Op), "lg" => ("lg", Op), "exp" => ("exp", Op),
        "lim" => ("lim", Op), "limsup" => ("lim sup", Op), "liminf" => ("lim inf", Op), "max" => ("max", Op),
        "min" => ("min", Op), "sup" => ("sup", Op), "inf" => ("inf", Op), "det" => ("det", Op),
        "dim" => ("dim", Op), "ker" => ("ker", Op), "deg" => ("deg", Op), "gcd" => ("gcd", Op),
        "arg" => ("arg", Op), "Pr" => ("Pr", Op), "hom" => ("hom", Op),
        // Delimiters
        "{" | "lbrace" => ("{", Open), "}" | "rbrace" => ("}", Close), "langle" => ("⟨", Open),
        "rangle" => ("⟩", Close), "lfloor" => ("⌊", Open), "rfloor" => ("⌋", Close),
        "lceil" => ("⌈", Open), "rceil" => ("⌉", Close), "lvert" => ("|", Open), "rvert" => ("|", Close),
        "lVert" => ("‖", Open), "rVert" => ("‖", Close), "vert" => ("|", Ord), "Vert" => ("‖", Ord),
        _ => return None,
    };
    Some(symbol)
}

/// A letter or digit in the alphabet of a font command: \mathbb{R} is ℝ.
fn letter(c: char, font: &str) -> char {
    // Letters that had a character of their own before the mathematical alphabets.
    let older = match (font, c) {
        ("mathbb", 'C') => 'ℂ', ("mathbb", 'H') => 'ℍ', ("mathbb", 'N') => 'ℕ', ("mathbb", 'P') => 'ℙ',
        ("mathbb", 'Q') => 'ℚ', ("mathbb", 'R') => 'ℝ', ("mathbb", 'Z') => 'ℤ',
        ("mathcal" | "mathscr", 'B') => 'ℬ', ("mathcal" | "mathscr", 'E') => 'ℰ',
        ("mathcal" | "mathscr", 'F') => 'ℱ', ("mathcal" | "mathscr", 'H') => 'ℋ',
        ("mathcal" | "mathscr", 'I') => 'ℐ', ("mathcal" | "mathscr", 'L') => 'ℒ',
        ("mathcal" | "mathscr", 'M') => 'ℳ', ("mathcal" | "mathscr", 'R') => 'ℛ',
        ("mathcal" | "mathscr", 'e') => 'ℯ', ("mathcal" | "mathscr", 'g') => 'ℊ',
        ("mathcal" | "mathscr", 'o') => 'ℴ',
        ("mathfrak", 'C') => 'ℭ', ("mathfrak", 'H') => 'ℌ', ("mathfrak", 'I') => 'ℑ',
        ("mathfrak", 'R') => 'ℜ', ("mathfrak", 'Z') => 'ℨ',
        _ => '\0',
    };
    if older != '\0' {
        return older;
    }
    // Where the font's A, a and 0 are.
    let (upper, lower, digit) = match font {
        "mathit" => return italic(c),
        "mathbf" | "boldsymbol" => (0x1D400, 0x1D41A, Some(0x1D7CE)),
        "mathbb" => (0x1D538, 0x1D552, Some(0x1D7D8)),
        "mathcal" | "mathscr" => (0x1D49C, 0x1D4B6, None),
        "mathfrak" => (0x1D504, 0x1D51E, None),
        "mathsf" => (0x1D5A0, 0x1D5BA, Some(0x1D7E2)),
        "mathtt" => (0x1D670, 0x1D68A, Some(0x1D7F6)),
        _ => return c,
    };
    let code = match c {
        'A'..='Z' => upper + (c as u32 - 'A' as u32),
        'a'..='z' => lower + (c as u32 - 'a' as u32),
        '0'..='9' if digit.is_some() => digit.unwrap_or_default() + (c as u32 - '0' as u32),
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}

/// The mathematical italic form of a Latin letter (U+1D434…; ℎ has its own code point).
fn italic(c: char) -> char {
    let code = match c {
        'h' => return 'ℎ',
        'a'..='z' => 0x1D44E + (c as u32 - 'a' as u32),
        'A'..='Z' => 0x1D434 + (c as u32 - 'A' as u32),
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(text: &str) -> Run {
        Run { text: text.to_owned(), rise: 0, depth: 0 }
    }

    #[test]
    fn letters_are_math_italic_and_operators_get_spaces_however_it_was_typed() {
        assert_eq!(runs("x+2=y"), [base("𝑥 + 2 = 𝑦")]);
        assert_eq!(runs("  x +   2 = y "), [base("𝑥 + 2 = 𝑦")]);
        // A minus sign, and a sign in front of a number is not an operation.
        assert_eq!(runs("-a-b"), [base("−𝑎 − 𝑏")]);
    }

    #[test]
    fn commands_become_symbols_spaced_by_what_they_are() {
        assert_eq!(runs(r"\alpha\le\infty"), [base("α ≤ ∞")]);
        assert_eq!(runs(r"a\cdot b\times c"), [base("𝑎 ⋅ 𝑏 × 𝑐")]);
        // Function names stay upright, with a space before what they apply to.
        assert_eq!(runs(r"2\sin x"), [base("2 sin 𝑥")]);
        assert_eq!(runs(r"\log(n)"), [base("log(𝑛)")]);
        // A comma is followed by a space; a command ends at its last letter.
        assert_eq!(runs(r"f(x,y)"), [base("𝑓(𝑥, 𝑦)")]);
        assert_eq!(runs(r"2\pi r"), [base("2π𝑟")]);
    }

    fn sup(text: &str) -> Run {
        Run { text: text.to_owned(), rise: 2, depth: 1 }
    }

    fn sub(text: &str) -> Run {
        Run { text: text.to_owned(), rise: -1, depth: 1 }
    }

    #[test]
    fn scripts_are_raised_or_lowered_and_smaller() {
        assert_eq!(runs("x^2"), [base("𝑥"), sup("2")]);
        // A script is one character unless braced: x^12 is 𝑥¹ followed by 2.
        assert_eq!(runs("x^12"), [base("𝑥"), sup("1"), base("2")]);
        assert_eq!(runs("a_{i,j}"), [base("𝑎"), sub("𝑖,𝑗")]);
        // Below and above at once; nothing is spaced inside a script.
        assert_eq!(runs("x_i^{n+1} = 0"), [base("𝑥"), sub("𝑖"), sup("𝑛+1"), base(" = 0")]);
        // A script's script sits further out, and smaller.
        assert_eq!(runs("e^{x^2}"), [base("𝑒"), sup("𝑥"), Run { text: "2".to_owned(), rise: 4, depth: 2 }]);
        assert_eq!(runs(r"\sum_{i=1}^n i"), [base("∑"), sub("𝑖=1"), sup("𝑛"), base(" 𝑖")]);
    }

    #[test]
    fn fractions_go_on_one_line_and_roots_get_their_sign() {
        assert_eq!(runs(r"\frac{1}{2}"), [base("1/2")]);
        // A part made of more than one thing gets parentheses.
        assert_eq!(runs(r"\frac{a+b}{2c}"), [base("(𝑎 + 𝑏)/(2𝑐)")]);
        assert_eq!(runs(r"\dfrac 1x"), [base("1/𝑥")]);
        assert_eq!(runs(r"\sqrt{x}"), [base("√𝑥")]);
        assert_eq!(runs(r"\sqrt{x^2+1}"), [base("√(𝑥"), sup("2"), base(" + 1)")]);
        // Cube and fourth roots have their own signs; other indices sit raised before √.
        assert_eq!(runs(r"\sqrt[3]{8}"), [base("∛8")]);
        assert_eq!(runs(r"\sqrt[n]{x}"), [sup("𝑛"), base("√𝑥")]);
    }

    #[test]
    fn words_fonts_escapes_and_unknown_commands() {
        // \text keeps its words and spaces, upright.
        assert_eq!(runs(r"x \text{ if } x>0"), [base("𝑥 if 𝑥 > 0")]);
        assert_eq!(runs(r"\mathrm{d}x"), [base("d𝑥")]);
        assert_eq!(runs(r"x\in\mathbb{R}, \mathbf{v}, \mathcal{L}"), [base("𝑥 ∈ ℝ, 𝐯, ℒ")]);
        assert_eq!(runs(r"\{x\}"), [base("{𝑥}")]);
        // A command nevis doesn't know stays as it was typed.
        assert_eq!(runs(r"\binomial{n}"), [base(r"\binomial𝑛")]);
    }

    #[test]
    fn accents_sized_delimiters_and_spacing_commands() {
        assert_eq!(runs(r"\vec{v} + \hat x + \bar{y}"), [base("𝑣\u{20D7} + 𝑥\u{302} + 𝑦\u{304}")]);
        assert_eq!(runs(r"\overline{AB}"), [base("𝐴\u{305}𝐵\u{305}")]);
        // \left and \right only size the delimiter that follows; \left. shows none.
        assert_eq!(runs(r"\left( \frac{a}{b} \right)"), [base("(𝑎/𝑏)")]);
        assert_eq!(runs(r"\left. x \right|_0"), [base("𝑥|"), sub("0")]);
        assert_eq!(runs(r"a\,b\quad c\;d"), [base("𝑎\u{2009}𝑏\u{2003}𝑐\u{2005}𝑑")]);
        assert_eq!(runs(r"\displaystyle x"), [base("𝑥")]);
    }

    #[test]
    fn environments_become_rows() {
        // Cells an em apart, rows on lines of their own; a matrix keeps its brackets.
        assert_eq!(runs(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"), [base("(𝑎\u{2003}𝑏\n𝑐\u{2003}𝑑)")]);
        // A row starts afresh: its minus is a sign.
        assert_eq!(
            runs(r"|x| = \begin{cases} x & x \ge 0 \\ -x & x < 0 \end{cases}"),
            [base("|𝑥| = {𝑥\u{2003}𝑥 ≥ 0\n−𝑥\u{2003}𝑥 < 0")]
        );
        assert_eq!(runs(r"\begin{array}{cc} 1 & 2 \end{array}"), [base("1\u{2003}2")]);
    }
}

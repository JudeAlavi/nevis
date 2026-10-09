# nevis

A fast, minimal notepad and Markdown editor. GTK4 + GtkSourceView 5, Rust.
Persian and other right-to-left text works per paragraph, mixed freely with English.

## Use

```
nevis notes.md          # open (or create) a file, in a window of its own, where you are
nevis a.md b.md         # several files: one window, a tab each
nevis ~/Notes           # a folder: its files in a sidebar, Ctrl+P to jump, Ctrl+Shift+F to search
nevis                   # nevis as you left it
```

Files opened from outside (a file manager, a terminal) get a window of their own on the
workspace you're on; a file already open comes forward instead. Inside nevis, everything opens
in tabs. When nevis isn't open, a file opened from outside shows on its own: the notes from
last time stay set aside, still kept, until a plain `nevis` brings them back. (Settings can
send files from outside to a tab in the window in front instead, after the last session.)

**Notes are never lost.** Closing nevis (Ctrl+Q, or closing its last window) asks nothing:
every tab comes back the next time it opens, unsaved text included, with the cursor where it
was. Unsaved text is written aside a second after you stop typing, so even a crash loses at
most that second. Closing a single tab with unsaved changes asks first (Save, Don't Save,
Cancel). A note that was never saved is named after its first line.

**Tabs.** Inside a window, files open in tabs; the tab bar appears with the second one (or
always: Settings). Each tab shows the number Alt+number picks it by (Alt+9 is the last tab).
A dot on a tab means unsaved changes; it turns into × under the pointer.
`+` or Ctrl+T opens an empty note, Ctrl+W or a middle click closes a tab, Ctrl+Shift+T brings
the last one back. A click in the sidebar, the switcher or the search shows the file in place
of the tab in front (when that has no unsaved changes); Ctrl+click opens it in a new tab.

**Markdown.** Markdown files open rendered (or as source: Settings): headings, lists, checkboxes, quotes, callouts, code
with colours, tables, images, links, math (`$…$`, `$$…$$`, drawn as text: symbols, raised and
lowered scripts, fractions on one line) and Mermaid flowcharts (drawn as diagrams; other
Mermaid diagrams stay code). Ctrl+E or a double-click switches to the source and back. The
source is styled too: headings sized by level, bold/italic/links/code shown as such, the
markup characters (`#`, `**`, `>`, `](url)`, `$`) dimmed but kept visible, and fenced code
coloured by its language (```` ```rust ````, ```` ```py ````…), as in the rendered view. In the source,
Enter continues a list (`- `, `3. ` → `4. `, `- [ ] `) or a quote, and Enter on an empty item
ends it. Click a link to follow it (`[[wiki]]`, relative `.md`, `#heading`, `[[Note#Heading]]`)
or a checkbox to tick it.

**Links, as in Obsidian.** A `[[link]]` or a relative link opens the note beside this one if
there is one, and otherwise the note of that name anywhere in the vault (the folder holding
`.obsidian`, or the folder the window shows), case aside; a path (`[[Projects/Plans]]`) has to
match whole folder names, and the shortest path wins a tie. Links to notes that don't exist
yet are fainter; following one starts the note. Resting the pointer on a link in the
rendered view shows the start of its note. Typing `[[` offers the vault's notes (and today's,
tomorrow's and yesterday's daily notes), `[[Note#` its headings, `#` the vault's tags and
`> [!` the callout types: Up and Down choose, Enter or Tab puts it in, Esc closes. Ctrl+P finds
headings and tags as well as files (`#` first for tags). Renaming a note in the sidebar
offers to update the links to it, keeping their headings and words.

**Notes.** Ctrl+R lists the note's headings, indented by level: type to narrow them, Enter
goes there (in either view). In a folder window, Ctrl+D opens today's note,
`2026-10-08.md`, making it if needed: in a Daily or Journal subfolder if there is one, or
where Obsidian's daily-notes settings (`.obsidian/daily-notes.json`) put it, named the way
they say. Under a rendered note, *Linked from* lists the notes in the folder that link to it
(`[[Name]]`, `[[Name#Heading]]` or a relative link, followed the way a click follows them),
each with the lines that do. Pasting a screenshot (Ctrl+V with an image copied) saves it in
`attachments/` beside the note and links it; a note with no file yet keeps its images aside
until its first save brings them beside it. Ctrl+K turns the selected words into a link,
with the cursor where the address goes, and pasting an address over selected words links
them. An empty note opens ready for typing.

**Code.** Other files get syntax highlighting for the 180 languages GtkSourceView knows,
picked by the file's name, or by how it starts (`#!/usr/bin/env bash`) when it has no
extension. Plain `.txt` stays plain.

**Folders.** The sidebar shows the folder's notes as a tree, with folder and note icons (drawn
by nevis in the theme's colours: no icon theme to load) and indent guides; the note in front
is shown in it, its folders opened. `+` beside the folder's name makes a new note. Right-click
a file or folder to make a new note there, rename it or move it to the trash; open notes
follow a rename. Ctrl+Shift+F searches the text of every note
in the folder. The search is Persian-aware: Arabic ي/ى/ك match Persian ی/ک, vowel marks and
half-spaces are ignored, and ۱۴۰۳ matches 1403.

**Themes.** *System* (the default) takes its colours from the GTK theme: the named colours in
`~/.config/gtk-4.0/gtk.css` and what it imports, which is where matugen, noctalia and pywal
write theirs. It follows them live, so a new wallpaper repaints nevis without a restart.
Settings (Ctrl+,) → Theme → Change… picks another: Adwaita, Ayu (Mirage, Dark, Light), Catppuccin (Mocha, Latte), Dracula,
Everforest, GitHub (Dark, Light), Gruvbox (Dark, Light), Kanagawa, Nord, One Dark, Rosé Pine
(and Dawn), Solarized (Dark, Light), Tokyo Night. Moving through the list shows each at once;
Enter keeps it, Esc goes back. Every palette's text, links, code and syntax colours are kept
readable on its background (WCAG contrast), nudged where a palette falls short.

**Spelling.** With spell checking on (Settings), misspelled words are underlined; right-click
one for corrections, to add it to the dictionary, or to change the language (kept for next
time). Code, addresses and Persian are skipped, since there's no Persian dictionary in the
Arch repositories. It needs `libspelling` and a dictionary or two (`hunspell-en_us`,
`hunspell-it`), and is loaded only once turned on.

**Settings.** Ctrl+, (or right-click → Settings…) shows them all on one page: theme, text
size, text width (the window's full width, or a readable column in the middle, as in
Obsidian), word wrap, line numbers, spell checking, list continuation, how Markdown files
open, saving automatically, where files opened from outside go, and when the tab bar shows. A change applies at once, in every window, and is
kept for next time; the shortcuts (Ctrl+Plus, Alt+Z, Ctrl+Shift+L) change the same settings.
The right-click menu keeps only what's about the file at hand.

nevis keeps up with files: changes made by other programs are reloaded (or, with unsaved
edits, you're asked), and each file reopens where its cursor was.

| Keys | Action |
|---|---|
| Ctrl+E | rendered view ⇄ source |
| Ctrl+T / Ctrl+W / Ctrl+Shift+T | new tab / close tab / reopen closed tab |
| Ctrl+Tab, Ctrl+Shift+Tab, Alt+1…9 | next / previous / numbered tab (Alt+9: last) |
| Ctrl+Shift+Page Up / Page Down | move the tab left / right |
| Ctrl+N | new note: named, in a folder window; elsewhere an empty tab |
| Ctrl+Shift+N / Ctrl+Shift+W / Ctrl+Q | new window / close window / quit, keeping every tab |
| Ctrl+O / Ctrl+Shift+O | open file / open folder |
| Ctrl+B / Ctrl+P / Ctrl+Shift+F | folder sidebar / go to a file, heading or tag / search the folder |
| Ctrl+S / Ctrl+Shift+S | save / save as |
| Ctrl+F / Ctrl+H | find / replace (Enter, Shift+Enter, F3, Shift+F3 to move; Esc closes; `Aa` case, `ab` whole words, `.*` regular expressions) |
| Ctrl+R / Ctrl+D / Ctrl+K | go to a heading / today's note / make a link |
| Ctrl+, | settings |
| Ctrl+Plus / Ctrl+Minus / Ctrl+0, Ctrl+scroll | text size |
| Ctrl+Shift+L / Alt+Z | line numbers / word wrap |
| Ctrl+? (Ctrl+Shift+/), F1 | keyboard shortcut cheat sheet |
| Right-click, Shift+F10 | menu |

There is no header bar: the window title (`• name — folder`, `•` = unsaved) shows in the bar.

## Install

```
./install.sh            # builds, installs ~/.local/bin/{nevis,nevis-app}, desktop entry, user service
NEVIS_DEFAULT=0 ./install.sh   # same, without becoming the default app for .txt/.md
```

Or as a pacman package built from this checkout (into `/usr`; use one or the other, since
`~/.local` copies take precedence over the package's):

```
cd packaging/arch && makepkg -si
systemctl --user enable --now nevis.service   # optional, see below
```

Two programs: `nevis-app` is the editor; `nevis` is a small launcher (no GTK) that hands
files to the running editor over D-Bus, or starts `nevis-app` when none is running.

The first install enables `nevis.service`, which keeps `nevis-app` running with no windows
(it prepares fonts and the renderer a few seconds after login). Later installs leave the
service as it is; restart it to pick up a new build: `systemctl --user restart nevis.service`
(open tabs come back afterwards, unsaved text included).

Undo: `systemctl --user disable --now nevis.service`, and to restore the previous default app
`xdg-mime default org.xfce.mousepad.desktop text/plain text/markdown text/x-markdown`.

Where things live: the session and drafts of unsaved text in `~/.local/state/nevis/` (with
cursor positions and the window size), and images pasted into notes not saved yet in
`attachments/`; settings in `~/.config/nevis/settings`, the theme chosen in
`~/.config/nevis/theme` and the spelling language in `~/.config/nevis/spell-language`; the colour scheme made from it in
`~/.local/share/nevis/styles/`.

## Speed

Measured on the G531GV in power-saver mode, with the service running unless noted. Timings
on this laptop swing by ±50 ms from run to run and day to day; these are medians.

| | text on screen |
|---|---|
| a small note (2026-10-08) | ~130 ms (the build before tabs and themes: ~105 ms the same day) |
| a small note, with ten tabs restored from the last session | ~205 ms |
| `samples/math.md`, the first formulas drawn in a session | ~300 ms (glyphs at script sizes) |
| `samples/languages.md` (15 scripts, 2026-10-07) | ~365 ms (676 ms before fonts were pre-loaded) |
| cold start, no service | ~850 ms, of which ~600 ms is GTK's own startup |
| folder search, 5,000 notes (56 MB), whole folder | ~250 ms |

Design choices that keep it there: no icons anywhere (GTK's first icon lookup costs ~300 ms
on this machine; it is also why the tab bar is nevis's own and not a GtkNotebook, whose scroll
arrows cost ~0.4 s); the launcher doesn't load GTK; the service warms up GTK's renderer and
the fonts before the first file, then (one per idle moment) the fallback fonts for emoji, CJK,
Hindi, Thai, Hebrew, Georgian, Armenian and math; restored tabs load, and join the window,
only when first shown; the find bar, sidebar, switcher, search, Settings and theme picker are
built on first use; search skips the
case lookup for Arabic-script letters (most of its time) and reads files on up to four
threads; highlighting and code colours are applied right after the text is first shown; and
the source styling only re-tags what changed on each edit.

## Development

```
cargo test --bin nevis-app      # the pure rules: styling, cursor memory, listing, switcher, anchors,
                                # links, note names, folder search, math, flowcharts, sessions,
                                # lists, colours, GTK colours, palettes, outline, daily notes,
                                # backlinks, pasted images, link resolution, renaming, tags
cargo run --release --example baseline   # an empty GTK4 window: the floor for any GTK app here
```

Dev hooks (environment variables, no effect otherwise; `nevis` passes them to `nevis-app`):

- `NEVIS_BENCH=1 NEVIS_T0=$(date +%s%N) nevis file.md`: print checkpoints and time to text on screen, then quit.
- `NEVIS_SNAPSHOT=out.png nevis file.md`: save a picture of the window, then quit.
- `NEVIS_ACTIONS=win.reading,win.switch-to:/path/a.md` (with the above): run window actions
  first, 150 ms apart; `:` gives a string argument. Useful ones: `win.follow:Note#heading`,
  `win.quick-open`, `win.search`, `win.new-note`, `win.new-tab`, `win.settings`, `win.theme`,
  `win.outline`, `win.daily`, `win.link`,
  `win.note-rename:/path/a.md`.
- `NEVIS_TYPE=text` (with the above): type the text where the focus is, replacing the selection;
  in a field a final newline presses Enter. Styling self-checks after edits.
- `NEVIS_PASTE_IMAGE=file.png` (with the above): then paste that image at the cursor, in place
  of the clipboard (which test runs share with the desktop).
- `NEVIS_WAIT=ms` (with the above): wait longer before typing and the picture (default 500).
- `NEVIS_SETTLE=ms` (with the above): wait longer after typing, before the picture (default 300).
- `NEVIS_SESSION=1` (with the above): keep and restore the session, which measurement runs
  otherwise leave alone. Pair it with `XDG_STATE_HOME=/tmp/x` to keep test runs out of the real
  session and cursor memory, and `XDG_CONFIG_HOME` to test themes and GTK colours.
- `NEVIS_APP_ID=dev.shirvani.NevisTest`: a separate instance, leaving the real service alone.
- `NEVIS_TRACE=1 nevis-app --gapplication-service`: log checkpoints and when each window's text is on screen.
- `samples/`: `languages.md` (every script and direction case), `math.md`, `diagrams.md`.

## Next

- Mermaid sequence diagrams; tabs dragged to reorder.

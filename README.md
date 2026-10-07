# nevis

A fast, minimal text and Markdown editor for one file at a time. GTK4 + GtkSourceView 5, Rust.
Persian and other right-to-left text works per paragraph, mixed freely with English.

## Use

```
nevis notes.md          # open (or create) a file; one window per file
nevis                   # empty window
```

Markdown files open rendered (headings, lists, checkboxes, quotes, code, tables, images,
links). Ctrl+E or a double-click switches to the source and back. Click a link to follow it
(`[[wiki]]` and relative `.md` links open in nevis) or a checkbox to tick it.

| Keys | Action |
|---|---|
| Ctrl+E | rendered view ⇄ source |
| Ctrl+N / Ctrl+O | new window / open |
| Ctrl+S / Ctrl+Shift+S | save / save as |
| Ctrl+W / Ctrl+Q | close window / close all windows |
| Ctrl+F / Ctrl+H | find / replace (Enter, Shift+Enter, F3, Shift+F3 to move; Esc closes) |
| Ctrl+Plus / Ctrl+Minus / Ctrl+0, Ctrl+scroll | zoom |
| Ctrl+Shift+L | line numbers (source view; on by default) |
| Alt+Z | word wrap |
| Ctrl+? (Ctrl+Shift+/), F1 | keyboard shortcut cheat sheet |
| Right-click, Shift+F10 | menu |

There is no header bar: the window title (`• name — folder`, `•` = unsaved) shows in the bar.

## Install

```
./install.sh            # builds, installs ~/.local/bin/nevis, desktop entry, user service
NEVIS_DEFAULT=0 ./install.sh   # same, without becoming the default app for .txt/.md
```

The installer enables `nevis.service`, which keeps one nevis process running with no windows
(`nevis --gapplication-service`). Opening a file then skips GTK's startup.

Undo: `systemctl --user disable --now nevis.service`, and to restore the previous default app
`xdg-mime default org.xfce.mousepad.desktop text/plain text/markdown text/x-markdown`.

## Speed

Measured on the G531GV in power-saver mode (2026-10-07):

| | text on screen |
|---|---|
| file opened through the running service | ~135–160 ms |
| cold start (service not running) | ~840–890 ms, of which ~600 ms is GTK's own startup |

Design choices that keep it there: no icons anywhere on the startup path (GTK's first icon
lookup costs ~300 ms on this machine), the find bar is built on first use, and syntax
highlighting is applied right after the text is first shown.

Dev hooks (environment variables, no effect otherwise):

- `NEVIS_BENCH=1 NEVIS_T0=$(date +%s%N) nevis file.md`: print startup checkpoints and time to text on screen, then quit.
- `NEVIS_SNAPSHOT=out.png nevis file.md`: save a picture of the window, then quit.
- `NEVIS_ACTIONS=win.shortcuts` (with the above): run window actions first, e.g. to picture the cheat sheet.
- `samples/languages.md`: every script and direction case worth eyeballing after a rendering change.
- `NEVIS_TRACE=1 nevis --gapplication-service`: log when each window's text is on screen.
- `cargo run --release --example baseline`: time an empty GTK4 window, the floor for any GTK app here.

## Next

- Markdown styling while editing: per-level headings, dimmed `**`/`#` markers.
- Read mode: syntax-highlighted code blocks, `#heading` links, hanging indents for wrapped list items.
- Reload on external change, remember cursor position, PKGBUILD.
- A tiny launcher that hands files to the service without loading GTK (~95 ms → ~10 ms).
- Later: folder sidebar, Mermaid, math.

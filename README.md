# nevis

A simple, extremely fast GUI text editor, written in Rust with GTK 4. Inspired by Obsidian,
Mousepad, Notepad and micro.

I wanted something to edit text without opening a heavy application like VS Code or Zed.

## Features

- **Fast**: a note is on screen in about 130 ms on a 2019 laptop, as a small background
  service keeps nevis ready.
- **Nothing lost**: tabs come back as you left them, unsaved text included; a crash loses at
  most the last second of typing.
- **Markdown, as in Obsidian**: a rendered view and Live Preview editing, with `[[links]]`,
  callouts, tags, embedded notes, backlinks and daily notes, in any folder or Obsidian vault.
- **Math and diagrams**: formulas (`$…$`) and Mermaid flowcharts, sequence, state and class
  diagrams and Gantt charts, drawn in the note.
- **Folders**: a sidebar, Ctrl+P to jump to any note, heading or tag, and a search through
  every note.
- **Persian and other right-to-left text**, mixed freely with English.
- **Code**: syntax highlighting for every language GtkSourceView knows.
- **Themes**: your GTK theme's colours (matugen, noctalia, pywal), followed live, or a built-in
  one such as Catppuccin, Gruvbox or Tokyo Night.

## Build and install

nevis runs on Linux with GTK 4.14 or later and GtkSourceView 5. Spell checking also needs
libspelling and a Hunspell dictionary.

From a release: download `nevis-<version>-x86_64-linux.tar.gz` from
[Releases](https://github.com/Soheil7799/nevis/releases), unpack it and run `./install.sh` in
it. It is built on Ubuntu 24.04, so it runs there and on anything newer (Debian 13, Fedora 40,
Arch…).

From source, with Rust installed: run `./install.sh` in a checkout; it builds nevis first.
On Arch, `cd packaging/arch && makepkg -si` builds a package instead.

`install.sh` puts `nevis` and `nevis-app` in `~/.local/bin`, adds a desktop entry and a
systemd user service that keeps nevis ready, and makes nevis the default app for `.txt` and
`.md` files (`NEVIS_DEFAULT=0 ./install.sh` leaves those alone). After installing a newer
version, `systemctl --user restart nevis.service` switches to it; your tabs come back.

## Use

```
nevis notes.md      # open (or create) a file
nevis a.md b.md     # several files, a tab each
nevis ~/Notes       # a folder, its notes in a sidebar
nevis               # nevis as you left it
```

Ctrl+E switches a Markdown note between the rendered view and its source. Ctrl+Shift+P finds
any command by name, F1 lists the keyboard shortcuts and Ctrl+, opens the settings.

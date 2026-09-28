# Obelus

A code reader, in a terminal or in a window of its own. **It doesn't want
you to type.**

In the AI era every line you read is a line you did not write. Reading is the
product here; editing is incidental. The point is to join what a language server
knows about the code to what git knows about its history — jump to a definition
from inside a diff, ask for a symbol's history rather than a file's — which no
terminal tool does today.

An *obelus* (÷, †) is the mark a scholar put in a manuscript's margin to flag a
line as doubtful. A reader's mark, made while reading.

[sunli829.github.io/obelus](https://sunli829.github.io/obelus/) — what it does,
how to use it, and why it is shaped this way. Also in
[中文](https://sunli829.github.io/obelus/zh/).

## Getting it

There is one script per shell, and which you want is which shell you are in.
On Linux and macOS:

```
curl -fsSL https://raw.githubusercontent.com/sunli829/obelus/master/contrib/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/sunli829/obelus/master/contrib/install.ps1 | iex
```

Either one takes the latest release, checks what it downloaded against the
release's own `SHA256SUMS`, and puts `ob` on your PATH.

**`obg`, the same reader in a window**, is a second binary and is asked for
by name. It is gvim's relation to vim rather than a second program: the same
grid and the same keys, drawn with a font it carries instead of the
terminal's, and the chords a terminal flattens into one byte arrive as
themselves. Everything after `-s --` goes to the script:

```
curl -fsSL .../contrib/install.sh | sh -s -- --bin obg    # the window
curl -fsSL .../contrib/install.sh | sh -s -- --bin both   # both of them
```

A pipe into `iex` has nowhere to put an argument, so on Windows the script
is fetched first:

```powershell
irm https://raw.githubusercontent.com/sunli829/obelus/master/contrib/install.ps1 -OutFile install.ps1
.\install.ps1 -Binary obg      # or -Binary both
```

`--dir` says where to put it, and `--help` lists the rest.

On Linux, `obg` from the script is a binary and nothing else: a launcher
needs a desktop entry and an icon beside it, and those are in the `.deb`,
the `.rpm` and the AppImage rather than here. On a desktop, install one of
those instead. There is no `obg` for musl at all — the window finds Vulkan,
Wayland and X11 by `dlopen`, which a static binary cannot do — and the
script says so rather than leaving you a thing that will not start.

[The releases](https://github.com/sunli829/obelus/releases) carry an archive
for every platform as well as a `.deb`, an `.rpm`, an AppImage and a macOS
`.app`.

Or from source, which needs nothing but a stable Rust:

```
cargo install --path crates/obelus-cli    # installs as `ob`
cargo install --path crates/obelus-gui    # and as `obg`, in a window
```

```
ob                   # the welcome screen, on this directory
ob src/main.rs       # read a file — its repository is the project
ob ../another-repo   # a directory: that is the project
obg src/main.rs      # the same file, in a window instead
```

A Nerd Font and a language server are both optional: without either, reading
still works.

## Building

```
cargo build                     # stable
cargo +nightly fmt              # nightly only: the rustfmt options require it
cargo clippy --all-features --all-targets
cargo test
UPDATE_FIXTURES=1 cargo test    # after an intended rendering change
cargo test -- --ignored         # the slow real-server tests, and the diff sweep
```

Tests assert on the terminal cell grid, colours included: a widget can write
every character correctly and paint none of them, and an assertion on the text
cannot tell.

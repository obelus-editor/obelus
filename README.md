# Obelus

A code editor that fits how I work, **with the agent built in** — in a
window of its own or in a terminal.

It is not trying to be a powerful editor: no multiple cursors, no macros.
In the AI era most lines you read are lines you did not write, so much of
the time in it is spent reading what an agent changed, beside the code it
changed. The point is to join what a language server
knows about the code to what git knows about its history — jump to a definition
from inside a diff, ask for a symbol's history rather than a file's — which no
terminal tool does today.

An *obelus* (÷, †) is the mark a scholar put in a manuscript's margin to flag a
line as doubtful. A reader's mark, made while reading.

[obelus-editor.github.io/obelus](https://obelus-editor.github.io/obelus/) — what it does,
how to use it, and why it is shaped this way. Also in
[中文](https://obelus-editor.github.io/obelus/zh/).

## Getting it

There is one script per shell, and which you want is which shell you are in.
On Linux and macOS:

```
curl -fsSL https://raw.githubusercontent.com/obelus-editor/obelus/master/contrib/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/obelus-editor/obelus/master/contrib/install.ps1 | iex
```

Either one takes the latest release, checks what it downloaded against the
release's own `SHA256SUMS`, and puts **`obg`** on your PATH — Obelus in a
window of its own.

**`ob`, the same editor in a terminal**, is the other binary and is asked
for by name. The two are gvim's relation to vim rather than two programs:
the same grid, the same views and the same keys. What the window has is the
presses a terminal flattens into one byte — `ctrl+i` apart from `Tab`,
`ctrl+[` apart from `Escape` — and the marks compiled in rather than guessed
at from whichever font the terminal was given. What the terminal has is that
it is already open, and that it works over ssh. Everything after `-s --`
goes to the script:

```
curl -fsSL .../contrib/install.sh | sh -s -- --bin ob      # the terminal
curl -fsSL .../contrib/install.sh | sh -s -- --bin both    # both of them
```

A pipe into `iex` has nowhere to put an argument, so on Windows the script
is fetched first:

```powershell
irm https://raw.githubusercontent.com/obelus-editor/obelus/master/contrib/install.ps1 -OutFile install.ps1
.\install.ps1 -Binary ob       # or -Binary both
```

`--dir` says where to put it, and `--help` lists the rest.

On Windows the script puts Obelus in the Start menu as well as on your
PATH — the icon is in the binary, so the shortcut is the whole of it, and
`-NoShortcut` leaves the menu alone. On Linux the same is a desktop entry
for each of `obg` and `ob` that was installed — `ob`'s opens in a terminal
— and an icon, under `~/.local/share`, which every launcher reads:
omarchy's `Apps` menu among them. `--no-desktop` leaves them out. `-Uninstall`
takes back the binaries and the Start menu entry, and the directory and its
PATH entry where nothing else is left in it: it is the half that matters
there, because a PATH and a Start menu are what a reader cannot simply
delete. `--uninstall`, with the same `--dir` it was installed with, is the
same half of the shell script — `ob` and `obg` out of `--dir`, the desktop
entries and their icon, and, on omarchy, the theme template and the link to
what omarchy renders — and it leaves the PATH, which it never changed. Neither script touches your settings. And on musl
there is no `obg` to install at all — the window finds Vulkan, Wayland and X11 by `dlopen`, which a static
binary cannot do — so the script says so and installs `ob`.

[The releases](https://github.com/obelus-editor/obelus/releases) carry an archive
for every platform as well as a `.deb`, an `.rpm`, an AppImage and a macOS
`.app`.

Or from source, which needs nothing but a stable Rust:

```
cargo install --path crates/obelus-gui    # installs as `obg`, in a window
cargo install --path crates/obelus-cli    # and as `ob`, in the terminal
```

```
obg                  # the welcome screen, on this directory
obg src/main.rs      # read a file — its repository is the project
obg ../another-repo  # a directory: that is the project
ob src/main.rs       # the same file, in the terminal instead
```

A language server is optional, and without one reading still works. A Nerd
Font matters only in the terminal — `obg` carries the marks in the binary,
so it never asks.

## Building

```
cargo build --workspace                  # stable
cargo +nightly fmt --all                 # nightly only: the rustfmt options require it
cargo clippy --workspace --all-features --all-targets
cargo test --workspace
UPDATE_FIXTURES=1 cargo test --workspace # after an intended rendering change
cargo test --workspace -- --ignored      # the slow real-server tests, and the diff sweep
```

Tests assert on the terminal cell grid, colours included: a widget can write
every character correctly and paint none of them, and an assertion on the text
cannot tell.

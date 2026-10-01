#!/bin/sh
# Fetches a released Obelus and puts it somewhere on your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/obelus-editor/obelus/master/contrib/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --bin ob --dir /usr/local/bin
#   curl -fsSL .../install.sh | sh -s -- --uninstall
#
# POSIX sh, because the one machine this has to work on is somebody else's.
# What it needs is `uname`, `tar`, `curl` or `wget`, and something that
# computes a sha256 -- and it says which one is missing rather than failing
# half-way through.
#
# It verifies what it downloaded against the release's own SHA256SUMS. A
# script that pipes into a shell and then installs an unchecked binary has
# asked for trust twice and earned it once.
set -eu

repository=obelus-editor/obelus
# The window, because that is the Obelus to meet first: the presses a
# terminal cannot report arrive as themselves and the marks are in the
# binary rather than guessed at from somebody else's font. The terminal one
# is the same reader and is asked for by name.
binaries=obg
# What `--bin` said, which matters in one place: a machine that cannot have
# the window at all -- see `musl` below.
asked=
tag=
directory=${OBELUS_INSTALL_DIR:-$HOME/.local/bin}
# Whether to hand omarchy Obelus's theme template, where this is running on
# omarchy. Done unasked because a desktop that themes every program is a
# reader who meant Obelus too, and refusable because it writes two files
# outside `--dir`.
omarchy=yes
# Whether to put what was installed in the desktop's launcher, which on
# Linux is a desktop entry each and an icon. Done unasked for the reason
# Windows gets a Start menu entry: a launcher is where a reader starts
# things, and a binary only a shell can find is half an install of one --
# for `ob` too, which a desktop opens in a terminal of its own choosing.
# Refusable because it writes files outside `--dir`.
desktop=yes
# Whether this is the taking-away rather than the putting-there.
removing=no

# The two files the omarchy half writes. They and the desktop half's are
# the only things this puts outside `--dir`, and so the ones `--uninstall`
# has to know the names of -- which is why they are named here rather than
# where they are written.
# `rendered` is where omarchy renders the theme: the link points *into*
# that directory rather than at a copy, because the whole of it is replaced
# when a theme is set.
templates=$HOME/.config/omarchy/themed
template_file=$templates/obelus.toml.tpl
themes=$HOME/.config/obelus/themes
link=$themes/omarchy.toml
rendered=$HOME/.local/state/omarchy/current/theme/obelus.toml

# The files the desktop half writes, named here for the same reason: an
# entry per binary, and the icon both of them draw. Under the reader's own
# data directory, which every launcher reads -- the GNOME and KDE ones, and
# omarchy's `Apps`, which is the desktop entries and nothing else, so this
# is how Obelus gets into that menu too.
data=${XDG_DATA_HOME:-$HOME/.local/share}
applications=$data/applications
icon=$data/icons/hicolor/scalable/apps/obelus.svg

say() { printf '%s\n' "$*"; }
die() { printf 'obelus: %s\n' "$*" >&2; exit 1; }

# Which entry is which binary's. The window's keeps the name the packages
# give it, so that an install from here and one from a `.deb` are the same
# entry rather than two.
entry_of() {
    case $1 in
        obg) printf '%s/obelus.desktop' "$applications" ;;
        ob) printf '%s/obelus-terminal.desktop' "$applications" ;;
    esac
}

# What an entry starts, as its `Exec` line. The whole path rather than the
# name, because a launcher does not read the PATH a shell profile sets, and
# `--dir` need not be on it at all. Asked by the install and by
# `--uninstall` alike, which is how the second knows an entry is this
# install's: one that starts some other `obg` is somebody else's.
exec_line() { printf 'Exec="%s/%s" %%F' "$directory" "$1"; }

usage() {
    cat <<'USAGE'
Usage: install.sh [options]

  --bin obg|ob|both   Which to install. `obg` is the window and `ob` is
                      the terminal. The default is `obg`.
  --version vX.Y.Z    A release to install. The default is the latest.
  --dir PATH          Where to put it. The default is ~/.local/bin, or
                      $OBELUS_INSTALL_DIR where that is set.
  --no-omarchy        Do not install Obelus's omarchy theme template, which
                      is otherwise installed where omarchy is found. With
                      --uninstall, leave it where it is.
  --no-desktop        Do not put what is installed in the desktop's
                      launcher, which is otherwise done on Linux. With
                      --uninstall, leave it there.
  --uninstall         Take it away again: `ob` and `obg` out of --dir,
                      their entries in the launcher, and the omarchy theme
                      template and its link. Nothing else -- your settings
                      are yours.
  --help              This.
USAGE
}

while [ $# -gt 0 ]; do
    case $1 in
        --bin) asked=${2:-}; binaries=$asked; shift 2 ;;
        --version) tag=${2:-}; shift 2 ;;
        --dir) directory=${2:-}; shift 2 ;;
        --no-omarchy) omarchy=no; shift ;;
        --no-desktop) desktop=no; shift ;;
        --uninstall) removing=yes; shift ;;
        --help|-h) usage; exit 0 ;;
        *) die "$1 is not an option this understands. --help says what is." ;;
    esac
done

# Taking it away again, which fetches nothing and checks nothing: what is on
# the machine is the whole of what this is about. So it happens before the
# curl, the wget and the sha256 this would otherwise insist on -- a machine
# that has lost one of those can still be a machine with an Obelus to remove.
#
# What it may take back is what it put there, which is the rule the Windows
# script's half follows too. Not the settings: those are the reader's own,
# and a script that removed them would be taking away the thing the install
# was for. Nor `--dir`, which on this side is `~/.local/bin` and full of
# everything else the reader keeps there -- and nor the PATH, because the
# install said to add it rather than adding it.
take_it_away() {
    removed=0
    theme_went=no

    say "Taking Obelus out of $directory"

    for binary in ob obg; do
        [ -f "$directory/$binary" ] || continue
        if rm -f "$directory/$binary"; then
            say "  removed $directory/$binary"
            removed=$((removed + 1))
        else
            say "  $directory/$binary could not be removed"
        fi
    done

    # Each entry, and only where it starts the binary this is taking away
    # -- the rule the Windows script's shortcut follows. The icon goes once
    # neither entry is left and not otherwise: an entry left behind for a
    # second install still draws it.
    if [ "$desktop" = yes ]; then
        entries_went=no
        for binary in ob obg; do
            entry=$(entry_of "$binary")
            [ -f "$entry" ] || continue
            if ! grep -qxF "$(exec_line "$binary")" "$entry"; then
                say "  left $entry alone: it starts $(sed -n 's/^Exec=//p' "$entry")"
            elif rm -f "$entry"; then
                say "  removed $entry"
                removed=$((removed + 1))
                entries_went=yes
            else
                say "  $entry could not be removed"
            fi
        done
        if [ "$entries_went" = yes ] && [ ! -f "$(entry_of ob)" ] && [ ! -f "$(entry_of obg)" ]; then
            if [ -f "$icon" ] && rm -f "$icon"; then
                say "  removed $icon"
                removed=$((removed + 1))
            fi
        fi
        if [ "$entries_went" = yes ] && command -v update-desktop-database > /dev/null 2>&1; then
            update-desktop-database "$applications" > /dev/null 2>&1 || true
        fi
    fi

    if [ "$omarchy" = yes ]; then
        # The link, and only where it is this Obelus's. Anything else of
        # that name is the reader's own theme file -- which the install
        # would not write over either -- and a dangling link is still one
        # of ours, since that is the ordinary state of it between a theme
        # being set and the next.
        if [ -L "$link" ]; then
            points=$(readlink "$link" 2> /dev/null || true)
            if [ "$points" != "$rendered" ]; then
                say "  left $link alone: it points at $points"
            elif rm -f "$link"; then
                say "  removed $link"
                removed=$((removed + 1))
                theme_went=yes
            else
                say "  $link could not be removed"
            fi
        elif [ -e "$link" ]; then
            say "  left $link alone: it is a file this did not write"
        fi

        # The template, which is Obelus's own file at a name Obelus chose --
        # the install writes over whatever wears that name for the same
        # reason.
        if [ -f "$template_file" ]; then
            if rm -f "$template_file"; then
                say "  removed $template_file"
                removed=$((removed + 1))
            else
                say "  $template_file could not be removed"
            fi
        fi

        # Said, not done. Which theme a reader is on is a line of their
        # settings, and this script has no more business editing it on the
        # way out than it had on the way in -- but a name with nothing
        # behind it is worth a word, because Obelus's own answer to one is a
        # mark on that line and nothing on the screen changing colour.
        #
        # Only where the link went just now. It is the link that answers
        # to the name `omarchy`, not the template -- one left behind
        # because it is the reader's own file is a theme that still reads.
        # And a reader who has uninstalled already would otherwise be told
        # about a line they were told about last time.
        if [ "$theme_went" = yes ] && grep -q '^[[:space:]]*theme[[:space:]]*=[[:space:]]*"omarchy"' \
            "$HOME/.config/obelus/config.toml" 2> /dev/null; then
            say '
~/.config/obelus/config.toml still says theme = "omarchy", which now names a
theme that is not there. Obelus marks that line and keeps the colours it has.'
        fi
    fi

    # Because a script that removed nothing and said nothing is one the
    # reader cannot tell from a script that failed.
    if [ "$removed" -eq 0 ]; then
        say "There is nothing of Obelus's in $directory to remove."
    fi
}

if [ "$removing" = yes ]; then
    take_it_away
    exit 0
fi

case $binaries in
    ob|obg) ;;
    both) binaries='ob obg' ;;
    *) die "--bin takes ob, obg or both, and was given $binaries" ;;
esac

# Whether a download can be watched. A bar is drawn with carriage returns,
# which is a picture on a terminal and a thousand lines of one in a log, a
# pipe or somebody's CI -- so it is asked of stderr, where the bar goes,
# rather than of stdout, which is already down a pipe into `sh`.
if [ -t 2 ]; then
    watched=yes
else
    watched=no
fi

# What fetches. Whichever is here, asked to fail on a 404 rather than to
# write the page saying so into the file.
#
# Two of them, because the two kinds of download are not alike: SHA256SUMS
# and a theme template are a moment and say nothing, and a binary is tens of
# megabytes over somebody else's line -- long enough that a script saying
# only `fetching obg-...tar.xz` is a script the reader cannot tell from one
# that has hung. So `fetch_watched` is the one the archives go through, and
# it is the same fetch with its quiet taken off.
if command -v curl > /dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    # The same fetch with the bar asked for and `-s` taken off -- and `-S`
    # goes with it, because showing the error is all `-S` does and it only
    # has anything to do while `-s` is there to be argued with.
    if [ "$watched" = yes ]; then
        fetch_watched() { curl -fL --progress-bar "$1" -o "$2"; }
    else
        fetch_watched() { fetch "$1" "$2"; }
    fi
    # Quiet where `fetch` is not: every way this can fail has a sentence of
    # its own below, and curl's own `(22) The requested URL returned error`
    # would arrive in front of it.
    redirect() { curl -fsL -o /dev/null -w '%{url_effective}' "$1"; }
elif command -v wget > /dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
    # `--show-progress` is wget 1.16 and later, and an older wget -- or
    # busybox's, which is the wget on a machine small enough to have one --
    # refuses an option it does not know and downloads nothing at all. So it
    # is asked for rather than assumed, and a wget without it fetches
    # quietly, which is what this did before.
    if [ "$watched" = yes ] && wget --help 2>&1 | grep -q -- --show-progress; then
        fetch_watched() { wget -q --show-progress -O "$2" "$1"; }
    else
        fetch_watched() { fetch "$1" "$2"; }
    fi
    redirect() { wget -qO /dev/null -S "$1" 2>&1 | sed -n 's/^ *Location: *//p' | tail -1; }
else
    die 'neither curl nor wget is installed, and one of them has to be'
fi

# The two spellings of the same program.
if command -v sha256sum > /dev/null 2>&1; then
    sum() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum > /dev/null 2>&1; then
    sum() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    die 'neither sha256sum nor shasum is installed, so nothing here can be checked'
fi

# Which build. The tarballs are named by the target triple, so this is the
# whole of the platform detection.
system=$(uname -s)
machine=$(uname -m)
case $system in
    Linux)
        case $machine in
            x86_64|amd64) architecture=x86_64 ;;
            aarch64|arm64) architecture=aarch64 ;;
            *) die "there is no Linux build for $machine" ;;
        esac
        # A glibc binary on a musl system does not start, and the message it
        # gives says nothing about why, so ask before rather than after.
        if (ldd --version 2>&1 || true) | grep -qi musl; then
            libc=musl
        else
            libc=gnu
        fi
        target=$architecture-unknown-linux-$libc
        # The window finds Vulkan, Wayland and X11 by `dlopen`, which a
        # static musl binary cannot do, so there is no musl `obg` to fetch.
        #
        # Asked for by name that is an error, because the reader said which
        # one they wanted. Arrived at as the default it is not: they said
        # nothing, and what this machine can have is the terminal one. A
        # default that refused to install anything would be the script
        # holding out for a binary that does not exist.
        if [ "$libc" = musl ]; then
            case " $binaries " in
                *' obg '*)
                    if [ "$asked" = obg ]; then
                        die 'obg is not built for musl, because a static binary cannot dlopen the drivers it needs'
                    fi
                    say 'obg is not built for musl -- a static binary cannot dlopen the drivers a window needs -- so this is ob, the same reader in a terminal.'
                    binaries=ob
                    ;;
            esac
        fi
        ;;
    Darwin)
        # One binary for both architectures, so nothing is asked about this
        # machine's.
        target=universal2-apple-darwin
        ;;
    *)
        die "$system is not a system this installs on. Windows has install.ps1."
        ;;
esac

# The latest release, asked of the redirect rather than of the API: the API
# counts unauthenticated requests against an address and this does not.
if [ -z "$tag" ]; then
    # `|| true`, because a repository with no releases answers 404 and a
    # command substitution that fails ends the script where it stands --
    # which is one line before the sentence that says what happened.
    latest=$(redirect "https://github.com/$repository/releases/latest" || true)
    tag=${latest##*/}
    # Written as a test rather than as `A && B || C`, which reads as
    # if-then-else and is not one: shellcheck says so, and here it is right
    # about what the line looks like even though the two are the same.
    # `latest` is what the last segment is when there was no redirect to
    # follow, which is the repository having no release at all.
    if [ -z "$tag" ] || [ "$tag" = latest ]; then
        die 'there is no published release yet'
    fi
fi

work=$(mktemp -d)
# shellcheck disable=SC2064 # `work` is wanted as it is now, not as it may be
trap "rm -rf '$work'" EXIT INT TERM

say "Obelus $tag for $target"

sums=$work/SHA256SUMS
fetch "https://github.com/$repository/releases/download/$tag/SHA256SUMS" "$sums" \
    || die "$tag has no SHA256SUMS, so there is nothing to check a download against"

mkdir -p "$directory" || die "$directory cannot be made"

for binary in $binaries; do
    name=$binary-${tag#v}-$target
    archive=$work/$name.tar.xz

    say "  fetching $name.tar.xz"
    fetch_watched "https://github.com/$repository/releases/download/$tag/$name.tar.xz" "$archive" \
        || die "$tag has no $name.tar.xz"

    # `awk` and not `grep`, so that the name is compared and not matched:
    # a version has dots in it, and a dot in a pattern is any character.
    wanted=$(awk -v want="$name.tar.xz" '$2 == want || $2 == "*" want { print $1 }' "$sums")
    [ -n "$wanted" ] || die "SHA256SUMS says nothing about $name.tar.xz"
    got=$(sum "$archive")
    [ "$got" = "$wanted" ] || die "$name.tar.xz is not what the release says it is: $got, where SHA256SUMS says $wanted"

    tar -xJf "$archive" -C "$work" || die 'unpacking failed -- this needs a tar that reads xz'
    install -m 755 "$work/$name/$binary" "$directory/$binary" 2>/dev/null \
        || { cp "$work/$name/$binary" "$directory/$binary" && chmod 755 "$directory/$binary"; } \
        || die "$directory/$binary could not be written"
    say "  installed $directory/$binary"
done

# One of the release's text files, fetched into `$work` and checked like
# everything else this downloads: a text file rather than a binary, and the
# rule is about what the release says it is. Saying what stopped it and
# answering no, because neither half that asks is a reason to fail an
# install whose binary is in by now. `$2` is where in the repository the
# file is, for a release made before it was one of the release's.
fetch_checked() {
    want=$(awk -v want="$1" '$2 == want || $2 == "*" want { print $1 }' "$sums")
    if [ -z "$want" ]; then
        say "  $tag has no $1 -- $2 in the repository has it"
        return 1
    fi
    fetch "https://github.com/$repository/releases/download/$tag/$1" "$work/$1" \
        || { say "  $1 could not be fetched"; return 1; }
    have=$(sum "$work/$1")
    [ "$have" = "$want" ] \
        || { say "  $1 is not what the release says it is: $have, where SHA256SUMS says $want"; return 1; }
}

# What was installed, in the desktop's launcher: an entry for each, and the
# icon they and the window all ask for by name (the window by
# `StartupWMClass`, which is what matches the two). `ob`'s entry asks the
# launcher for a terminal rather than naming one. Linux's half alone -- a
# Mac has the `.app` for this.
the_desktop_entries() {
    say '
So that it can be started from the desktop'"'"'s launcher:'

    # A launcher splits `Exec` on its own quoting rules, where these five
    # mean something and need escaping twice over. A `--dir` with one in it
    # is rare enough to be told about rather than escaped.
    case $directory in
        *[\"\`\$\\%]*)
            say "  $directory has a character a desktop entry would have to escape, so there is none"
            return
            ;;
    esac

    fetch_checked obelus.svg contrib/desktop/ || return
    mkdir -p "$applications" "${icon%/*}" || { say "  $applications could not be made"; return; }
    # Written over where it is already there, like the theme template: it is
    # Obelus's own file at a name Obelus chose, and an earlier install into
    # another `--dir` is the entry that should now start this one.
    cp "$work/obelus.svg" "$icon" || { say "  $icon could not be written"; return; }
    say "  $icon"

    for binary in $binaries; do
        entry=$(entry_of "$binary")
        name=${entry##*/}
        fetch_checked "$name" contrib/desktop/ || continue
        awk -v exec_line="$(exec_line "$binary")" '/^Exec=/ { print exec_line; next } { print }' \
            "$work/$name" > "$entry" \
            || { say "  $entry could not be written"; continue; }
        say "  $entry"
    done

    # The entry says which files it opens, and what answers that question is
    # a cache beside it -- rebuilt where the program that does it is here.
    if command -v update-desktop-database > /dev/null 2>&1; then
        update-desktop-database "$applications" > /dev/null 2>&1 || true
    fi
}

if [ "$desktop" = yes ] && [ "$system" = Linux ]; then
    the_desktop_entries
fi

# omarchy themes every program on the desktop at once -- one palette, a
# template per program, a directory swapped into place -- and Obelus's part
# of that is a template in the directory omarchy reads templates from and a
# link at what it renders. The same three steps as
# `contrib/omarchy/README.md`, which is where the rest of this is explained.
#
# Nothing fails the install: the binary is in by now, and a desktop's
# colours are not a reason to say the install did not happen. So each step
# says what stopped it and gives up, rather than dying.
the_omarchy_theme() {
    say '
omarchy is here, so Obelus can be themed with the rest of the desktop:'

    template=$work/obelus.toml.tpl
    fetch_checked obelus.toml.tpl contrib/omarchy/ || return

    mkdir -p "$templates" "$themes" || { say "  $templates could not be made"; return; }
    # Written over where it is already there. The template is Obelus's own
    # file at a name Obelus chose, and an install is asking for this
    # version's mapping of it.
    cp "$template" "$template_file" \
        || { say "  $template_file could not be written"; return; }
    say "  $template_file"

    # A dangling link is a link (`-L` is true where `-e` is false), and it is
    # the ordinary state of this one between a theme being set and the next.
    # Anything else wearing that name is somebody's own theme file and is not
    # this script's to replace.
    if [ -L "$link" ] || [ ! -e "$link" ]; then
        ln -sfn "$rendered" "$link" || { say "  $link could not be made"; return; }
        say "  $link -> what omarchy renders"
    else
        say "  $link is a file this did not write, so it is left as it is"
        return
    fi

    # Rendered once, so that the link points at something now rather than
    # after the reader next changes theme. Setting the theme they are already
    # on, so the screen keeps the colours it has.
    current=$(omarchy-theme-current 2>/dev/null || true)
    if [ -n "$current" ] && omarchy-theme-set "$current" > /dev/null 2>&1; then
        say "  rendered for $current"
    else
        say '  omarchy-theme-set, on the theme you are on, renders it the first time'
    fi

    # Said rather than done, like the PATH below: which theme a reader is on
    # is a line of their settings, and this one has no business choosing it.
    if grep -q '^[[:space:]]*theme[[:space:]]*=[[:space:]]*"omarchy"' \
        "$HOME/.config/obelus/config.toml" 2>/dev/null; then
        return
    fi
    say '
Then, in ~/.config/obelus/config.toml, so that every theme you set from then
on themes Obelus as well:

    theme = "omarchy"'
}

# `~/.config/omarchy` alone is a directory a reader may have kept after
# moving off, so what is asked for is the two commands this actually uses.
if [ "$omarchy" = yes ] \
    && command -v omarchy-theme-current > /dev/null 2>&1 \
    && command -v omarchy-theme-set > /dev/null 2>&1; then
    the_omarchy_theme
fi

# Said rather than done: a script that edits somebody's shell profile is a
# script that has decided which of their four they use.
case ":$PATH:" in
    *":$directory:"*) ;;
    *) say "
$directory is not on your PATH. Add it:

    export PATH=\"$directory:\$PATH\"" ;;
esac

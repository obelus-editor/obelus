#!/bin/sh
# Fetches a released Obelus and puts it somewhere on your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/sunli829/obelus/master/contrib/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --bin ob --dir /usr/local/bin
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

repository=sunli829/obelus
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

say() { printf '%s\n' "$*"; }
die() { printf 'obelus: %s\n' "$*" >&2; exit 1; }

usage() {
    cat <<'USAGE'
Usage: install.sh [options]

  --bin obg|ob|both   Which to install. `obg` is the window and `ob` is
                      the terminal. The default is `obg`.
  --version vX.Y.Z    A release to install. The default is the latest.
  --dir PATH          Where to put it. The default is ~/.local/bin, or
                      $OBELUS_INSTALL_DIR where that is set.
  --help              This.
USAGE
}

while [ $# -gt 0 ]; do
    case $1 in
        --bin) asked=${2:-}; binaries=$asked; shift 2 ;;
        --version) tag=${2:-}; shift 2 ;;
        --dir) directory=${2:-}; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) die "$1 is not an option this understands. --help says what is." ;;
    esac
done

case $binaries in
    ob|obg) ;;
    both) binaries='ob obg' ;;
    *) die "--bin takes ob, obg or both, and was given $binaries" ;;
esac

# What fetches. Whichever is here, asked to fail on a 404 rather than to
# write the page saying so into the file.
if command -v curl > /dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    # Quiet where `fetch` is not: every way this can fail has a sentence of
    # its own below, and curl's own `(22) The requested URL returned error`
    # would arrive in front of it.
    redirect() { curl -fsL -o /dev/null -w '%{url_effective}' "$1"; }
elif command -v wget > /dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
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
    [ -n "$tag" ] && [ "$tag" != latest ] || die 'there is no published release yet'
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
    fetch "https://github.com/$repository/releases/download/$tag/$name.tar.xz" "$archive" \
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

# Said rather than done: a script that edits somebody's shell profile is a
# script that has decided which of their four they use.
case ":$PATH:" in
    *":$directory:"*) ;;
    *) say "
$directory is not on your PATH. Add it:

    export PATH=\"$directory:\$PATH\"" ;;
esac

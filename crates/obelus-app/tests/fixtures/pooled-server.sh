#!/bin/sh
# A language server that is only a witness: it writes down where it was told
# the machine's pool of build jobs is, beside the project it was started in,
# and then says back whatever it is sent -- which is all `cat` does for every
# other stand-in.
printf '%s' "${CARGO_MAKEFLAGS:-none}" > pooled
exec cat

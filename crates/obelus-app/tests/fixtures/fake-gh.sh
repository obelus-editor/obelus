#!/bin/sh
# Stands in for `gh pr list --json ...`: prints what the file named first
# says, so that a test can change the answer between two askings.
#
# A file holding `signed-out` is answered the way `gh` answers when nobody
# has signed in -- its words on stderr and exit 4 -- so that what Obelus
# makes of that is tested against the real shape of it.
answer="$1"
if [ "$(cat "$answer")" = "signed-out" ]; then
    echo "To get started with GitHub CLI, please run:  gh auth login" >&2
    exit 4
fi
cat "$answer"

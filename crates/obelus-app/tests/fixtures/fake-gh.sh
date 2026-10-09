#!/bin/sh
# Stands in for `gh`: prints what the file named first says, so that a test
# can change the answer between two askings.
#
# `gh pr list` is answered from that file. `gh pr view N` is answered from
# the file beside it named `<that file>.view.N`, and with nothing said at
# all where there is none.
#
# Two answers are `gh`'s refusals rather than lists, each in the words and
# with the code the real one gives, so that what Obelus makes of them is
# tested against the real shape of them: `signed-out`, and `not-github`
# for a checkout whose remotes are somewhere else -- which also names
# `gh auth login`, and is not about signing in.
answer="$1"
if [ "$3" = "view" ]; then
    if [ -f "$answer.view.$4" ]; then
        answer="$answer.view.$4"
    else
        echo "{}"
        exit 0
    fi
fi
case "$(cat "$answer")" in
    signed-out)
        echo "To get started with GitHub CLI, please run:  gh auth login" >&2
        exit 4
        ;;
    not-github)
        echo "none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use \`gh auth login\`" >&2
        exit 1
        ;;
esac
cat "$answer"

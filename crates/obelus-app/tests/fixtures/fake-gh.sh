#!/bin/sh
# Stands in for `gh pr list --json ...`: prints what the file named first
# says, so that a test can change the answer between two askings.
#
# Two answers are `gh`'s refusals rather than lists, each in the words and
# with the code the real one gives, so that what Obelus makes of them is
# tested against the real shape of them: `signed-out`, and `not-github`
# for a checkout whose remotes are somewhere else -- which also names
# `gh auth login`, and is not about signing in.
answer="$1"
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

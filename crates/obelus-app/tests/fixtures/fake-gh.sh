#!/bin/sh
# Stands in for `gh`: prints what the file named first says, so that a test
# can change the answer between two askings.
#
# `gh api graphql` asking for the pull requests is answered from that file,
# which holds the rows of one page, each the shape GitHub sends a node in.
# Asking for the issues is answered from the file beside it named
# `<that file>.issues`, and with no issues where there is none. Either is
# printed as the page `gh --paginate` prints -- an object, and the next one
# straight after it with not even a newline between, as the real one does
# -- and where `<file>.page2` is beside it, that is a second page, printed
# only once `<that file>.go` is there, so that a test can look at the list
# between the two. `gh pr view N` and `gh issue view N` are answered from
# the file beside it named `<that file>.view.N`, and with nothing said at
# all where there is none.
#
# Two answers are `gh`'s refusals rather than lists, each in the words and
# with the code the real one gives, so that what Obelus makes of them is
# tested against the real shape of them: `signed-out`, and `not-github`
# for a checkout whose remotes are somewhere else -- which also names
# `gh auth login`, and is not about signing in.
answer="$1"
go="$1.go"
page() {
    printf '{"data":{"repository":{"list":{"nodes":%s,"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}' "$(cat "$1")"
}
if [ "$2" = "api" ]; then
    case "$*" in
        *list:issues*)
            if [ -f "$answer.issues" ]; then
                answer="$answer.issues"
            else
                echo '{"data":{"repository":{"list":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}'
                exit 0
            fi
            ;;
    esac
fi
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
        echo "error parsing \"owner\" value: none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use \`gh auth login\`" >&2
        exit 1
        ;;
esac
if [ "$2" != "api" ]; then
    cat "$answer"
    exit 0
fi
page "$answer"
if [ -f "$answer.page2" ]; then
    while [ ! -f "$go" ]; do
        sleep 0.05
    done
    page "$answer.page2"
fi

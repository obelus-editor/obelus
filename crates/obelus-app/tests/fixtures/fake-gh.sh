#!/bin/sh
# Stands in for `gh`: prints what the file named first says, so that a test
# can change the answer between two askings.
#
# `gh api graphql` asking for the open pull requests is answered from that
# file, which holds the rows of one page, each the shape GitHub sends a node
# in. A cursor `pageN` is answered from `<that file>.pageN`, and a page says
# there is another after it while the next of those is there. Asking what
# has changed is answered from `<that file>.changed`, whose rows carry their
# `state`, and asking GitHub's search from `<that file>.found` and its own
# pages. The issues are the same files with `.issues` after the name. Any of
# them that is not there is a page of nothing. How many there are in all is
# `<that file>.total` where it is there, and otherwise how many rows its
# pages hold. `gh pr view N` and `gh issue view N` are answered from the
# file beside it named `<that file>.view.N`, and with nothing said at all
# where there is none. Every `gh api` asked is written down, a line each,
# in `<that file>.log`.
#
# Three answers are `gh`'s refusals rather than lists, each in the words and
# with the code the real one gives, so that what Obelus makes of them is
# tested against the real shape of them: `signed-out`, `not-github` for a
# checkout whose remotes are somewhere else -- which also names `gh auth
# login`, and is not about signing in -- and `eof`, the connection dropping.
answer="$1"
if [ "$3" = "view" ]; then
    if [ -f "$answer.view.$4" ]; then
        cat "$answer.view.$4"
    else
        echo "{}"
    fi
    exit 0
fi
echo "$*" >> "$answer.log"

base="$answer"
case "$*" in
    *issues\(*|*"on Issue"*) base="$answer.issues" ;;
esac
shape=list
case "$*" in
    *search\(*) shape=found; base="$base.found" ;;
    *open:*) shape=changed; base="$base.changed" ;;
esac
at=1
file="$base"
for word in "$@"; do
    case "$word" in
        cursor=page*)
            at="${word#cursor=page}"
            file="$base.page$at"
            ;;
    esac
done

rows="[]"
if [ -f "$file" ]; then
    case "$(cat "$file")" in
        signed-out)
            echo "To get started with GitHub CLI, please run:  gh auth login" >&2
            exit 4
            ;;
        not-github)
            echo "error parsing \"owner\" value: none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use \`gh auth login\`" >&2
            exit 1
            ;;
        eof)
            echo "Post \"https://api.github.com/graphql\": EOF" >&2
            exit 1
            ;;
    esac
    rows="$(cat "$file")"
fi
if [ -f "$base.page$((at + 1))" ]; then
    more="true,\"endCursor\":\"page$((at + 1))\""
else
    more="false,\"endCursor\":null"
fi
# Counted a page at a time rather than through `"$base".page*`: on Windows
# the path is written with backslashes, which a pattern reads as escapes,
# and the pages past the first went uncounted.
count() {
    if [ -f "$1" ]; then
        grep -o '"number"' "$1" | wc -l | tr -d ' '
    else
        echo 0
    fi
}
if [ -f "$base.total" ]; then
    total="$(cat "$base.total")"
else
    total="$(count "$base")"
    n=2
    while [ -f "$base.page$n" ]; do
        total=$((total + $(count "$base.page$n")))
        n=$((n + 1))
    done
fi
info="\"pageInfo\":{\"hasNextPage\":$more}"
case "$shape" in
    list) printf '{"data":{"repository":{"list":{"totalCount":%s,"nodes":%s,%s}}}}' "$total" "$rows" "$info" ;;
    changed) printf '{"data":{"repository":{"open":{"totalCount":%s},"list":{"nodes":%s,%s}}}}' "$total" "$rows" "$info" ;;
    found) printf '{"data":{"search":{"issueCount":%s,"nodes":%s,%s}}}' "$total" "$rows" "$info" ;;
esac

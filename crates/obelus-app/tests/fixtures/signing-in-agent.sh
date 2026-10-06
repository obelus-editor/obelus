#!/bin/sh
# An agent nobody has signed in to, for testing how Obelus signs a reader in.
#
# `sh`, for the reason `fake-agent.sh` is: a test that needs anything more
# is a test that stops running on somebody's machine.
#
#   sh signing-in-agent.sh <marker>           the agent
#   sh signing-in-agent.sh <marker> --login   its sign-in, which asks for a
#                                             code and takes `right`
#
# Signed in is the marker existing. The agent offers two ways in, and only
# to a client that said it can run a sign-in in a terminal -- which is what
# claude-agent-acp does: a client that says nothing is offered nothing.
#
#   by-terminal   this script again, with `--login` on the end: a terminal
#                 way in names only what goes after the agent's own
#                 command, so whatever runs it has to know how the agent was
#                 started
#   by-agent      `authenticate`, which signs in on its own
#   by-nothing    a program in `_meta`, the older way, that is not there
#
# Until then `session/new` and `session/prompt` answer -32000, with words of
# its own, which is what an agent that wants a sign-in says -- and a prompt
# starting `/refuse` does whatever the marker says, the way a sign-in that
# has run out does. Every request
# is written down beside the marker, one method a line, so a test can see
# what was asked again once the reader was in.

marker=$1
log="$marker.log"

if [ "$2" = '--login' ]; then
    printf 'Code: '
    read -r code
    if [ "$code" = 'right' ]; then
        : >"$marker"
        printf 'Signed in\n'
        exit 0
    fi
    printf 'Not that code\n'
    exit 1
fi

id_of() {
    printf '%s' "$1" | sed -En 's/.*"id":("[^"]*"|[0-9]*).*/\1/p'
}

# Which conversation a request is about, read out of the request: several
# can be open at once, and each is answered about itself.
session_of() {
    printf '%s' "$1" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p'
}

opened=0

refuse() {
    printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"Sign in to the fake first"}}\n' "$(id_of "$1")"
}

while IFS= read -r line; do
    method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
    if [ -n "$method" ]; then
        printf '%s\n' "$method" >>"$log"
    fi
    case "$line" in
        *'"method":"initialize"'*)
            case "$line" in
                *'"auth":{"terminal":true'*)
                    ways='[{"id":"by-terminal","name":"Type a code","description":"In a terminal","type":"terminal","args":["--login"]},{"id":"by-agent","name":"Let it sign in"},{"id":"by-nothing","name":"Run what is not there","_meta":{"terminal-auth":{"command":"/nonexistent/obelus-sign-in","label":"Nothing"}}}]'
                    ;;
                *) ways='[]' ;;
            esac
            printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":%s,"agentInfo":{"name":"signing-in","title":"Fake","version":"0.1"}}}\n' "$(id_of "$line")" "$ways"
            ;;
        *'"method":"authenticate"'*)
            : >"$marker"
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/new"'*)
            if [ ! -e "$marker" ]; then
                refuse "$line"
                continue
            fi
            opened=$((opened + 1))
            printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s-%s"}}\n' "$(id_of "$line")" "$opened"
            ;;
        *'"method":"session/prompt"'*'"text":"/refuse'*)
            # Signed in once and not now: a turn that finds the reader is
            # not in any more, the way a token that has run out does.
            refuse "$line"
            ;;
        *'"method":"session/prompt"'*)
            if [ ! -e "$marker" ]; then
                refuse "$line"
                continue
            fi
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"glad you are in"}}}}\n' "$(session_of "$line")"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id_of "$line")"
            ;;
        *'"id":'*'"method":'*)
            printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"not here"}}\n' "$(id_of "$line")"
            ;;
    esac
done

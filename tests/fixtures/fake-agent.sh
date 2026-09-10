#!/bin/sh
# An agent, for testing obelus's side of the Agent Client Protocol.
#
# Written in `sh` on purpose: a test that needs python, or node, or a second
# Rust binary, is a test that stops running on somebody's machine. What it
# needs of a shell is `read`, `case` and `sed`, which is POSIX.
#
# It plays one conversation:
#
#   initialize            -> what it is, and protocol version 1
#   session/new           -> a session
#   session/prompt        -> it thinks, says something, reads a file through
#                            obelus, uses a tool, and asks permission; the
#                            turn ends once the answer to that arrives
#   session/prompt        -> with "slowly" in it: nothing at all, so the turn
#                            stays in flight until it is cancelled
#   session/cancel        -> the turn ends, cancelled
#
# Every reply's id is read out of the request rather than assumed, because
# the point of the exercise is that obelus's numbering is its own business.

id_of() {
    printf '%s' "$1" | sed -n 's/.*"id":\([0-9]*\).*/\1/p'
}

turn=''

while IFS= read -r line; do
    case "$line" in
        *'"method":"initialize"'*)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentInfo":{"name":"Fake Agent","version":"0.1"}}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/new"'*)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s-1"}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/prompt"'*'slowly'*)
            # Asked to take its time: it says nothing and answers nothing,
            # so the turn stays in flight until obelus cancels it.
            turn=$(id_of "$line")
            ;;
        *'"method":"session/prompt"'*)
            turn=$(id_of "$line")
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"working it out"}}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"it is "}}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a rust file"}}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read the file","status":"in_progress"}}}\n'
            printf '{"jsonrpc":"2.0","id":900,"method":"fs/read_text_file","params":{"sessionId":"s-1","path":"tests/fixtures/read-me.txt"}}\n'
            ;;
        *'"id":900'*)
            # What obelus handed back, quoted into a chunk so the test can
            # see that it was the buffer's text and not the disk's.
            # Up to the first quote or backslash: the answer ends with an
            # escaped newline, and what the test looks for is the words.
            text=$(printf '%s' "$line" | sed -n 's/.*"content":"\([^"\\]*\).*/\1/p')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" saying %s"}}}}\n' "$text"
            printf '{"jsonrpc":"2.0","id":901,"method":"session/request_permission","params":{"sessionId":"s-1","toolCall":{"toolCallId":"t1","title":"Run the tests"},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n'
            ;;
        *'"id":901'*)
            case "$line" in
                *'"optionId":"once"'*) allowed='allowed' ;;
                *) allowed='refused' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" and I was %s"}}}}\n' "$allowed"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            ;;
        *'"method":"session/cancel"'*)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$turn"
            ;;
    esac
done

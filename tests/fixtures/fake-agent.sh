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
#   session/new           -> a session, two modes, three slash commands and two
#                            settings
#   session/set_mode      -> taken
#   session/set_config_option
#                         -> taken, and every setting again with the new value
#   session/prompt "/..." -> says which command it ran, and ends the turn
#   session/prompt        -> it thinks, says something, reads a file through
#                            obelus, tries to write one (which obelus
#                            refuses), uses a tool, and asks permission; the
#                            turn ends once the answer to that arrives
#   session/prompt        -> with "quickly" in it: it puts itself on the
#                            faster model and says so, unasked
#   session/prompt        -> with "slowly" in it: nothing at all, so the turn
#                            stays in flight until it is cancelled
#   session/cancel        -> the turn ends, cancelled
#
# Every reply's id is read out of the request rather than assumed, because
# the point of the exercise is that obelus's numbering is its own business.

# The id, verbatim: a number stays a number and a string keeps its quotes.
# Real clients number requests however they like -- the protocol's own crate
# uses uuids -- and an answer has to carry back exactly what came in.
id_of() {
    printf '%s' "$1" | sed -n 's/.*"id":\("[^"]*"\|[0-9]*\).*/\1/p'
}

turn=''

# What its settings are on. Changed by `session/set_config_option` and read
# back out by `options`: an agent's settings are state, and a client that
# sets one and is told the old value back has been lied to.
model='fast'
allow='false'
switches=''

# Every setting it offers, as the protocol's own list.
#
# The switch is only offered to a client that said it can show one, which is
# what that capability is for: obelus promises `boolean: {}` in the
# handshake, and a client that stops promising it stops being offered the
# row.
options() {
    printf '[{"id":"model","name":"Model","description":"Which model it thinks with","type":"select","currentValue":"%s","options":[{"value":"fast","name":"Fast","description":"Fast"},{"value":"careful","name":"Careful","description":"Slower, and better"}]}' "$model"
    if [ -n "$switches" ]; then
        printf ',{"id":"allow_all","name":"Allow everything","type":"boolean","currentValue":%s}' "$allow"
    fi
    printf ']'
}

while IFS= read -r line; do
    case "$line" in
        *'"method":"initialize"'*)
            # What the client promised. obelus says it reads files and does
            # not write them, and an agent decides what to ask for from
            # exactly this -- so a client that said something else is a
            # different client, and says so through the name it is given
            # back.
            case "$line" in
                *'"readTextFile":true'*'"writeTextFile":false'*) me='Fake Agent' ;;
                *) me='Wrong Client' ;;
            esac
            case "$line" in
                *'"boolean":{}'*) switches='yes' ;;
                *) switches='' ;;
            esac
            printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentInfo":{"name":"%s","version":"0.1"}}}\n' "$(id_of "$line")" "$me"
            ;;
        *'"method":"session/new"'*)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s-1","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"ask first"},{"id":"code","name":"write code"}]},"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            # What it takes with a slash, which agents send once the
            # session is ready.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact","description":"Summarise the conversation"},{"name":"cost","description":"What this has cost","input":{"hint":"currency"}},{"name":"model","description":"Which model to use"}]}}}\n'
            ;;
        *'"method":"session/set_config_option"'*)
            which=$(printf '%s' "$line" | sed -n 's/.*"configId":"\([^"]*\)".*/\1/p')
            # A value id keeps its quotes here and is stripped below; a
            # switch arrives as `true` or `false`, which is the value.
            got=$(printf '%s' "$line" | sed -n 's/.*"value":\("[^"]*"\|true\|false\).*/\1/p')
            case "$which" in
                model) model=$(printf '%s' "$got" | tr -d '"') ;;
                allow_all) allow="$got" ;;
            esac
            printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            ;;
        *'"method":"session/set_mode"'*)
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/prompt"'*'"text":"/'*)
            # A command: the text starts with a slash, and everything after
            # the name is the command's own input.
            turn=$(id_of "$line")
            asked=$(printf '%s' "$line" | sed -n 's/.*"text":"\/\([^" ]*\).*/\1/p')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ran %s"}}}}\n' "$asked"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            ;;
        *'"method":"session/prompt"'*'quickly'*)
            # It changes a setting of its own accord and says so, which is
            # the other direction that path runs in: agents pick a model to
            # suit what they were asked and tell the client afterwards.
            turn=$(id_of "$line")
            model='fast'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"config_option_update","configOptions":%s}}}\n' "$(options)"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
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
            # And a write, which obelus refuses: it said so in the
            # handshake, and an agent that asks anyway gets an error.
            printf '{"jsonrpc":"2.0","id":902,"method":"fs/write_text_file","params":{"sessionId":"s-1","path":"tests/fixtures/read-me.txt","content":"no"}}\n'
            ;;
        *'"id":902'*)
            case "$line" in
                *'"error"'*) wrote='refused to write' ;;
                *) wrote='wrote the file' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" and it %s"}}}}\n' "$wrote"
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
            # Only when there is a turn to cancel. A cancellation that
            # arrives with nothing in flight is a no-op, and answering it
            # with an id nobody sent is a message no client can read.
            if [ -n "$turn" ]; then
                printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$turn"
                turn=''
            fi
            ;;
    esac
done

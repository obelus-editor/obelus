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
#   session/new           -> a session, two modes, a dozen slash commands (more
#                            than the list of them is tall) and two
#                            settings. Run it with `mode-as-option` and it
#                            offers its mode both ways at once, the way an
#                            agent part-way through the protocol's change
#                            does.
#   session/set_mode      -> taken
#   session/set_config_option
#                         -> taken, and every setting again with the new value
#   session/prompt "/..." -> says which command it ran, and ends the turn
#   session/prompt        -> it thinks, says something, reads a file through
#                            obelus, tries to write one (which obelus
#                            refuses), uses a tool, and asks permission; the
#                            turn ends once the answer to that arrives
#   session/prompt "/ask" -> asks the reader three things through
#                            `elicitation/create` -- one of a list, a switch,
#                            and a number -- and says what came back
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
forms=''

# Whether it offers its mode the new way as well as the old.
#
# A real agent part-way through the protocol's change does exactly this: a
# config option with `category: "mode"`, *and* the `modes` field the
# dedicated methods use, so that clients on either side of the change
# understand it. Asked for with an argument, because both shapes have to be
# tested and an agent is only ever one of them.
both_ways=''
# Whether it refuses to change its mode. An agent that says no to
# `session/set_mode` says it with an error and nothing else -- there is no
# answer to that request to carry the mode it is in -- so a client that
# showed the new one before asking has to take it back.
refuses=''
# Whether it offers anything about itself at all. Plenty of agents have
# nothing to configure, and a client has to say so rather than draw an
# empty row.
bare=''
# Whether it asks the reader something the moment the session opens, before
# anything has been said to it. Agents do: a login, a workspace to use. What
# it is here for is that the reader may have walked away from the
# conversation by then.
at_once=''
for word in "$@"; do
    case "$word" in
        mode-as-option) both_ways='yes' ;;
        refuse-mode) refuses='yes' ;;
        nothing-to-change) bare='yes' ;;
        asks-at-once) at_once='yes' ;;
    esac
done

# What its settings are on. Changed by `session/set_config_option` and read
# back out by `options`: an agent's settings are state, and a client that
# sets one and is told the old value back has been lied to.
model='fast'
allow='false'
way='ask'
switches=''

# Every setting it offers, as the protocol's own list.
#
# The switch is only offered to a client that said it can show one, which is
# what that capability is for: obelus promises `boolean: {}` in the
# handshake, and a client that stops promising it stops being offered the
# row.
options() {
    printf '['
    if [ -n "$both_ways" ]; then
        # First in the list as well, because an agent puts the mode where a
        # reader looks for it -- and the client is asked to keep the order.
        printf '{"id":"way","name":"Way of working","category":"mode","type":"select","currentValue":"%s","options":[{"value":"ask","name":"ask first"},{"value":"code","name":"write code"}]},' "$way"
    fi
    printf '{"id":"model","name":"Model","description":"Which model it thinks with","type":"select","currentValue":"%s","options":[{"value":"fast","name":"Fast","description":"Fast"},{"value":"careful","name":"Careful","description":"Slower, and better"}]}' "$model"
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
            # Whether it may ask the reader anything at all. A client that
            # does not advertise a form cannot be sent one, so obelus's
            # promise is what decides between the question and the excuse.
            case "$line" in
                *'"form":{}'*) forms='yes' ;;
                *) forms='' ;;
            esac
            printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentInfo":{"name":"%s","version":"0.1"}}}\n' "$(id_of "$line")" "$me"
            ;;
        *'"method":"session/new"'*)
            if [ -n "$bare" ]; then
                printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s-1"}}\n' "$(id_of "$line")"
            else
                printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s-1","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"ask first"},{"id":"code","name":"write code"}]},"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            fi
            # And, where it was asked to, a question before anything has
            # been said to it.
            if [ -n "$at_once" ]; then
                printf '{"jsonrpc":"2.0","id":906,"method":"elicitation/create","params":{"mode":"form","sessionId":"s-1","message":"which workspace am I in","requestedSchema":{"type":"object","properties":{"where":{"type":"string","title":"Where"}},"required":["where"]}}}\n'
            fi
            # What it takes with a slash, which agents send once the
            # session is ready.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact","description":"Summarise the conversation"},{"name":"cost","description":"What this has cost","input":{"hint":"currency"}},{"name":"model","description":"Which model to use"},{"name":"ask","description":"Ask the reader something"},{"name":"help","description":"What it takes"},{"name":"init","description":"Start again"},{"name":"login","description":"Say who you are"},{"name":"quit","description":"Stop"},{"name":"reset","description":"Forget the session"},{"name":"share","description":"Send it somewhere"},{"name":"theme","description":"Its own colours"},{"name":"usage","description":"What it has spent"}]}}}\n'
            ;;
        *'"method":"session/set_config_option"'*)
            which=$(printf '%s' "$line" | sed -n 's/.*"configId":"\([^"]*\)".*/\1/p')
            # A value id keeps its quotes here and is stripped below; a
            # switch arrives as `true` or `false`, which is the value.
            got=$(printf '%s' "$line" | sed -n 's/.*"value":\("[^"]*"\|true\|false\).*/\1/p')
            case "$which" in
                model) model=$(printf '%s' "$got" | tr -d '"') ;;
                allow_all) allow="$got" ;;
                way) way=$(printf '%s' "$got" | tr -d '"') ;;
            esac
            printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            ;;
        *'"method":"session/set_mode"'*)
            if [ -n "$refuses" ]; then
                printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"not that one"}}\n' "$(id_of "$line")"
            else
                printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$(id_of "$line")"
            fi
            ;;
        *'"method":"session/prompt"'*'"text":"/pick'*)
            # A form shaped like the one a real agent sends when it asks
            # "what would you like to do": one choice whose options carry a
            # line about themselves, and a free-text field for an answer
            # that is not on the list.
            turn=$(id_of "$line")
            printf '{"jsonrpc":"2.0","id":904,"method":"elicitation/create","params":{"mode":"form","sessionId":"s-1","message":"what would you like to do","requestedSchema":{"type":"object","properties":{"task":{"type":"string","title":"Task","oneOf":[{"const":"report","title":"Write the weekly report","description":"Gather the git changes of the week and write them up"},{"const":"review","title":"Review the code","description":"Read the current diff for bugs and simplifications"},{"const":"build","title":"Carry on with obelus","description":"Write code in this repository"},{"const":"survey","title":"Survey the repository","description":"Read the recent commits and describe where things stand"}]},"other":{"type":"string","title":"Other","description":"Type your own answer instead of choosing one above"}},"required":["task"]}}}\n'
            ;;
        *'"id":904'*)
            # The form's response is an object: a choice must use the
            # option's id, and leaving "Other" empty must omit its key.
            # Its keys arrive sorted rather than in the order the card was
            # filled in, which is what a JSON object is.
            # Keeping both in the reply makes the UI test fail if either
            # side stops being true.
            case "$line" in
                *'"action":"accept"'*'"other":'*'"task":"review"'*)
                    said='you picked review and something else'
                    ;;
                *'"action":"accept"'*'"task":"review"'*)
                    said='you picked review and nothing else'
                    ;;
                *) said='you did not pick review' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            ;;
        *'"id":906'*)
            case "$line" in
                *'"action":"accept"'*) said='you are somewhere' ;;
                *) said='you would not say where' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            ;;
        *'"method":"session/prompt"'*'"text":"/many'*)
            # A turn with a run of tool calls of one kind in it, which is
            # what an agent looking around a repository actually does: a
            # client that draws thirty of these has drawn a log.
            turn=$(id_of "$line")
            for name in app acp buffer ui; do
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call","toolCallId":"r-%s","title":"Read src/%s","kind":"read","status":"completed","locations":[{"path":"%s/tests/fixtures/many_lines.rs","line":4}]}}}\n' "$name" "$name" "$PWD"
            done
            # And one that failed, after them: the run it belongs to is not
            # the same run, because what failed is not a read.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call","toolCallId":"x-1","title":"Run the tests","kind":"execute","status":"failed"}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"that is where it is"}}}}\n'
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            ;;
        *'"method":"session/prompt"'*'"text":"/nowhere'*)
            # A tool call naming a file that is not there, which is what an
            # agent that deleted one -- or made one up -- sends.
            turn=$(id_of "$line")
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call","toolCallId":"g-1","title":"Read the missing file","kind":"read","status":"completed","locations":[{"path":"%s/tests/fixtures/not-here.rs","line":2}]}}}\n' "$PWD"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            ;;
        *'"method":"session/prompt"'*'"text":"/die'*)
            # An agent that stops in the middle of a turn: a crash, a kill,
            # a `/quit` of its own. The client is left with a handle to a
            # conversation that has ended, and what it does about that is
            # the point of the test.
            exit 3
            ;;
        *'"method":"session/prompt"'*'"text":"/several'*)
            # A form with a multi-select on it: several answers at once,
            # which the schema carries as an array of the ids, plus the
            # free-text field an agent pairs with one to catch what the
            # list does not cover.
            turn=$(id_of "$line")
            printf '{"jsonrpc":"2.0","id":905,"method":"elicitation/create","params":{"mode":"form","sessionId":"s-1","message":"which parts should I look at","requestedSchema":{"type":"object","properties":{"areas":{"type":"array","title":"Areas","minItems":2,"items":{"anyOf":[{"const":"app","title":"src/app","description":"the application"},{"const":"acp","title":"src/acp","description":"the agent link"},{"const":"ui","title":"src/ui","description":"the screen"}]}},"other":{"type":"string","title":"Other","description":"Anywhere else it should look"}},"required":["areas"]}}}\n'
            ;;
        *'"id":905'*)
            case "$line" in
                *'"action":"accept"'*'"areas":["acp","ui"]'*'"other":"tests'*)
                    said='you picked two and said where else'
                    ;;
                *'"action":"accept"'*'"areas":["acp","ui"]'*)
                    said='you picked two'
                    ;;
                *) said='you picked something else' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            ;;
        *'"method":"session/prompt"'*'"text":"/ask'*)
            turn=$(id_of "$line")
            if [ -z "$forms" ]; then
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"this client cannot be asked"}}}}\n'
                printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
            else
                printf '{"jsonrpc":"2.0","id":903,"method":"elicitation/create","params":{"mode":"form","sessionId":"s-1","message":"which way should I do it","requestedSchema":{"type":"object","properties":{"how":{"type":"string","title":"How","oneOf":[{"const":"fast","title":"Quickly"},{"const":"careful","title":"Carefully","description":"and slowly"}]},"sure":{"type":"boolean","title":"Sure"},"times":{"type":"integer","title":"Times","minimum":1,"maximum":9}},"required":["how","sure","times"]}}}\n'
            fi
            ;;
        *'"id":903'*)
            case "$line" in
                *'"action":"accept"'*)
                    how=$(printf '%s' "$line" | sed -n 's/.*"how":"\([^"]*\)".*/\1/p')
                    sure=$(printf '%s' "$line" | sed -n 's/.*"sure":\(true\|false\).*/\1/p')
                    times=$(printf '%s' "$line" | sed -n 's/.*"times":\([0-9.]*\).*/\1/p')
                    # In brackets, so a test can say exactly what came
                    # back: a whole number sent as a float would otherwise
                    # read the same as far as the words go.
                    said="you said [$how] [$sure] [$times]"
                    ;;
                *) said='you would not say' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn"
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
            printf '{"jsonrpc":"2.0","id":901,"method":"session/request_permission","params":{"sessionId":"s-1","toolCall":{"toolCallId":"t1","title":"Run the tests","content":[{"type":"content","content":{"type":"text","text":"cargo test --all-features"}}],"locations":[{"path":"/tmp/obelus/Cargo.toml"}]},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n'
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

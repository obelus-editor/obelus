#!/bin/sh
# An agent, for testing Obelus's side of the Agent Client Protocol.
#
# Written in `sh` on purpose: a test that needs python, or node, or a second
# Rust binary, is a test that stops running on somebody's machine. What it
# needs of a shell is `read`, `case` and `sed`, which is POSIX -- and only
# POSIX: an alternation is `sed -E` and `|`, because `\|` in a basic
# expression is GNU's, and BSD's sed on a mac matches nothing with it, so
# every answer went out with no id and not one conversation got started.
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
#   session/prompt "/broken"
#                         -> an error instead of a stop reason, the way an
#                            agent nobody has signed in to answers
#   session/prompt        -> it thinks, says something, reads a file through
#                            Obelus, tries to write one (which Obelus
#                            refuses), uses a tool, and asks permission; the
#                            turn ends once the answer to that arrives
#   session/prompt "/pair"
#                         -> asks permission twice at once, and says what
#                            each answer was
#   session/prompt "/takeback"
#                         -> asks permission and takes the question back
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
# the point of the exercise is that Obelus's numbering is its own business.

# The working directory, spelled the way the program on the other end spells
# one.
#
# `$PWD` is this shell's, and on Windows this shell is a POSIX one running
# beside a Windows Obelus: it says `/e/work/obelus` where Obelus says
# `E:/work/obelus`, and a tool call naming a file that way names no file at
# all -- the transcript showed the path and Obelus could not read it.
# `cygpath` is what those shells ship for exactly this, and where there is
# none there is nothing to translate.
here=$PWD
# And the command this is asked to have run, in the words of the shell
# that will run it -- see where it is sent, below.
ran='sleep 0.3; printf %s obelus-ran-this; exit 3'
if command -v cygpath >/dev/null 2>&1; then
    ran='ping -n 2 127.0.0.1 >nul & <nul set /p =obelus-ran-this& exit 3'
    here=$(cygpath -m "$here")
fi

# The id, verbatim: a number stays a number and a string keeps its quotes.
# Real clients number requests however they like -- the protocol's own crate
# uses uuids -- and an answer has to carry back exactly what came in.
id_of() {
    printf '%s' "$1" | sed -En 's/.*"id":("[^"]*"|[0-9]*).*/\1/p'
}

# Which conversation a request is about, read out of the request rather than
# assumed. A client may have several open on one process, and an agent that
# answered about whichever it opened last would be an agent that cannot be
# used to find out whether the client routes them.
session_of() {
    printf '%s' "$1" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p'
}

# How many conversations have been opened, so that each gets a name of its
# own. `s-1` for the first, which is what every test written before this
# expects to see.
opened=0
session='s-1'

# The turn in flight, per conversation.
#
# One variable held them all until there were two conversations to hold, and
# then a prompt in the second overwrote the first's -- so a cancellation
# meant for one was answered against the other. A real agent keeps them
# apart; an agent that did not would agree with a client that did not, and
# the two would be wrong together with nothing to catch it.
turn_of() {
    eval "printf '%s' \"\${turn_$(printf '%s' "$1" | tr -c 'A-Za-z0-9' '_')-}\""
}
set_turn() {
    eval "turn_$(printf '%s' "$1" | tr -c 'A-Za-z0-9' '_')='$2'"
}

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
# How it can take a conversation up again, which it says at the handshake.
# A client reads this and asks the one way that works, rather than asking
# the fullest and reading the error.
again='load'
# Whether it says what it can be set to again, unasked, in the breath before
# the answer that opens the session -- which agents are free to do, and
# which is how a conversation opened only to ask came to be answered as if
# it were real: before that answer arrives, nothing on the client's side
# knows the session is not one.
tells=''
# Whether what it can be set to comes after the answer that opens the
# session rather than in it: an answer with nothing but the session's name,
# and then an update with every setting, which is the other way agents do
# it.
later=''
# Where it writes down every request it is sent, one line each: the method
# and the session it names. Nothing a client asks is visible from the
# outside otherwise, and some of what Obelus owes an agent is a request --
# a session it no longer wants, let go -- or the absence of one.
log=''
for word in "$@"; do
    case "$word" in
        mode-as-option) both_ways='yes' ;;
        refuse-mode) refuses='yes' ;;
        nothing-to-change) bare='yes' ;;
        asks-at-once) at_once='yes' ;;
        only-resumes) again='resume' ;;
        forgets) again='none' ;;
        tells-settings) tells='yes' ;;
        options-later) later='yes' ;;
        log=*) log="${word#log=}" ;;
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
# what that capability is for: Obelus promises `boolean: {}` in the
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
    # Which conversation this one is about. Every request after the first
    # names it, and answering about the one opened most recently would make
    # this agent useless for finding out whether the client keeps them
    # apart -- the two would agree by accident.
    named="$(session_of "$line")"
    if [ -n "$named" ]; then
        session="$named"
    fi
    if [ -n "$log" ]; then
        method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
        if [ -n "$method" ]; then
            printf '%s %s\n' "$method" "$named" >>"$log"
        fi
    fi
    case "$line" in
        *'"method":"initialize"'*)
            # What the client promised. Obelus says it reads files and does
            # not write them, and an agent decides what to ask for from
            # exactly this -- so a client that said something else is a
            # different client, and says so through the name it is given
            # back.
            case "$line" in
                *'"readTextFile":true'*'"writeTextFile":true'*) me='Fake Agent' ;;
                *) me='Wrong Client' ;;
            esac
            case "$line" in
                *'"boolean":{}'*) switches='yes' ;;
                *) switches='' ;;
            esac
            # Whether it may ask the reader anything at all. A client that
            # does not advertise a form cannot be sent one, so Obelus's
            # promise is what decides between the question and the excuse.
            case "$line" in
                *'"form":{}'*) forms='yes' ;;
                *) forms='' ;;
            esac
            # And whether it may run anything at all. A client that does
            # not offer `terminal` cannot be sent one, so what Obelus
            # promised is what decides between running and saying it
            # cannot.
            case "$line" in
                *'"terminal":true'*) terminals='yes' ;;
                *) terminals='' ;;
            esac
            case "$again" in
                load) able='"loadSession":true,' ;;
                resume) able='"sessionCapabilities":{"resume":{}},' ;;
                *) able='' ;;
            esac
            # And that it takes tools over HTTP, which is what the real one
            # says and what decides whether Obelus offers it any: a client
            # that hands an address to an agent which cannot fetch it has
            # offered nothing, so a fixture that stayed quiet here could not
            # tell a client that offers its tools from one that does not.
            printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{%s"mcpCapabilities":{"http":true}},"agentInfo":{"name":"fake-agent-acp","title":"%s","version":"0.1"}}}\n' "$(id_of "$line")" "$able" "$me"
            ;;
        *'"method":"session/new"'*)
            opened=$((opened + 1))
            session="s-$opened"
            # What it can be set to, before the answer, where it was told
            # to.
            if [ -n "$tells" ] && [ -z "$bare" ]; then
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"config_option_update","configOptions":%s}}}\n' "$(options)"
            fi
            if [ -n "$later" ]; then
                printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"'"$session"'"}}\n' "$(id_of "$line")"
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"config_option_update","configOptions":%s}}}\n' "$(options)"
            elif [ -n "$bare" ]; then
                printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"'"$session"'"}}\n' "$(id_of "$line")"
            else
                printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"'"$session"'","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"ask first"},{"id":"code","name":"write code"}]},"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            fi
            # And, where it was asked to, a question before anything has
            # been said to it.
            if [ -n "$at_once" ]; then
                printf '{"jsonrpc":"2.0","id":906,"method":"elicitation/create","params":{"mode":"form","sessionId":"'"$session"'","message":"which workspace am I in","requestedSchema":{"type":"object","properties":{"where":{"type":"string","title":"Where"}},"required":["where"]}}}\n'
            fi
            # What it takes with a slash, which agents send once the
            # session is ready.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact","description":"Summarise the conversation"},{"name":"cost","description":"What this has cost","input":{"hint":"currency"}},{"name":"model","description":"Which model to use"},{"name":"ask","description":"Ask the reader something"},{"name":"help","description":"What it takes"},{"name":"init","description":"Start again"},{"name":"login","description":"Say who you are"},{"name":"quit","description":"Stop"},{"name":"reset","description":"Forget the session"},{"name":"share","description":"Send it somewhere"},{"name":"theme","description":"Its own colours"},{"name":"usage","description":"What it has spent"}]}}}\n'
            ;;
        *'"method":"session/set_config_option"'*)
            which=$(printf '%s' "$line" | sed -n 's/.*"configId":"\([^"]*\)".*/\1/p')
            # A value id keeps its quotes here and is stripped below; a
            # switch arrives as `true` or `false`, which is the value.
            got=$(printf '%s' "$line" | sed -En 's/.*"value":("[^"]*"|true|false).*/\1/p')
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
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":904,"method":"elicitation/create","params":{"mode":"form","sessionId":"'"$session"'","message":"what would you like to do","requestedSchema":{"type":"object","properties":{"task":{"type":"string","title":"Task","oneOf":[{"const":"report","title":"Write the weekly report","description":"Gather the git changes of the week and write them up"},{"const":"review","title":"Review the code","description":"Read the current diff for bugs and simplifications"},{"const":"build","title":"Carry on with Obelus","description":"Write code in this repository"},{"const":"survey","title":"Survey the repository","description":"Read the recent commits and describe where things stand"}]},"other":{"type":"string","title":"Other","description":"Type your own answer instead of choosing one above"}},"required":["task"]}}}\n'
            ;;
        *'"method":"session/prompt"'*'"text":"/wordy'*)
            # The same form with one answer whose line about itself is
            # longer than a card is wide. What an agent writes there is a
            # sentence about what choosing it would do, and the answers to
            # one question are told apart by exactly those sentences.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":931,"method":"elicitation/create","params":{"mode":"form","sessionId":"'"$session"'","message":"which way","requestedSchema":{"type":"object","properties":{"way":{"type":"string","title":"Way","oneOf":[{"const":"sink","title":"Sink the free functions","description":"Move the few hundred lines of pure functions that have nothing to do with the application out to the crates they belong in, which is nearly free and takes very little off the top"},{"const":"gather","title":"Gather the state","description":"Move methods onto the types that already exist"}]}},"required":["way"]}}}\n'
            ;;
        *'"id":931'*)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
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
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"id":906'*)
            case "$line" in
                *'"action":"accept"'*) said='you are somewhere' ;;
                *) said='you would not say where' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            ;;
        *'"method":"session/prompt"'*'/signin'*)
            # Somewhere to go rather than something to fill in: the other
            # kind of elicitation. Long enough that it folds across rows,
            # which is the whole point of not cutting it short.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":909,"method":"elicitation/create","params":{"mode":"url","sessionId":"%s","message":"sign in to continue","elicitationId":"e1","url":"https://console.example.com/oauth/authorize?client_id=9d1c4a&scope=user%%3Ainference&code=1&state=7f2b"}}\n' "$session"
            ;;
        *'"method":"session/prompt"'*'/nowhere'*)
            # A URL Obelus will not hand to the machine's own launcher: a
            # scheme some program on this machine has registered, which is
            # the shape that turns a link into a way to start it. It must
            # come back as an error rather than as a refusal -- this is not
            # the reader saying no.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":910,"method":"elicitation/create","params":{"mode":"url","sessionId":"%s","message":"open this","elicitationId":"e2","url":"vscode://file/etc/passwd"}}\n' "$session"
            ;;
        *'"id":909'*|*'"id":910'*)
            # What the reader said about going. Said back into the
            # transcript so a test can read it off the page.
            case "$line" in
                *'"action":"accept"'*)
                    said='you went'
                    # And later, having watched the far end, the agent says
                    # the waiting is over. A notification: nothing is owed
                    # back, because Obelus answered when it sent them.
                    printf '{"jsonrpc":"2.0","method":"elicitation/complete","params":{"elicitationId":"e1"}}\n'
                    ;;
                *'"action":"decline"'*) said='you would not go' ;;
                *) said='the question went away' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$session" "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/run'*)
            # A command, the way an agent runs one: create, wait, read the
            # output, let go. Obelus runs it and shows it -- it does not
            # ask, because asking is what this agent's own permission
            # request is for.
            set_turn "$session" "$(id_of "$line")"
            if [ -z "$terminals" ]; then
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"this client runs nothing"}}}}\n' "$session"
                printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
                continue
            fi
            # Long enough that the wait is a wait: a command that has
            # already finished when the agent asks never reaches the half
            # of the client that holds the question open.
            #
            # Written for whichever shell Obelus will reach for, which on
            # Windows is `cmd`. Two tries said why it has to be: `cmd` does
            # not know `;`, so the first ran `sleep` with the rest as its
            # arguments (`invalid time interval '0.3;'`); and handing the
            # whole thing to `sh` inside quotes does not survive `cmd /C`,
            # which took the inner pair apart and left bash saying
            # `unexpected EOF while looking for matching '"'`.
            #
            # So: no quotes and no `;` on that side. `&` is how cmd chains,
            # `ping` is how it waits without a `sleep`, and
            # `<nul set /p =` is how it prints without a line ending.
            printf '{"jsonrpc":"2.0","id":920,"method":"terminal/create","params":{"sessionId":"%s","command":"%s","args":[]}}\n' "$session" "$ran"
            ;;
        *'"id":920'*)
            term=$(printf '%s' "$line" | sed 's/.*"terminalId":"//; s/".*//')
            # Embedded in the call, which is how a client is told where to
            # show it: the client holds the process, so the row it draws is
            # filled from what the client has rather than from anything
            # said here.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"c9","title":"Run the tests","kind":"execute","status":"in_progress","content":[{"type":"terminal","terminalId":"%s"}]}}}\n' "$session" "$term"
            printf '{"jsonrpc":"2.0","id":921,"method":"terminal/wait_for_exit","params":{"sessionId":"%s","terminalId":"%s"}}\n' "$session" "$term"
            ;;
        *'"id":921'*)
            code=$(printf '%s' "$line" | sed 's/.*"exitCode"://; s/[^0-9].*//')
            printf '{"jsonrpc":"2.0","id":922,"method":"terminal/output","params":{"sessionId":"%s","terminalId":"%s"}}\n' "$session" "$term"
            ;;
        *'"id":922'*)
            out=$(printf '%s' "$line" | sed 's/.*"output":"//; s/".*//')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"it said %s and ended %s"}}}}\n' "$session" "$out" "$code"
            printf '{"jsonrpc":"2.0","id":923,"method":"terminal/release","params":{"sessionId":"%s","terminalId":"%s"}}\n' "$session" "$term"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/forever'*)
            # A command that does not end on its own, for the key that
            # stops it. The agent never releases it -- which is the case
            # the client has to survive, because the process is the
            # client's and nothing else can stop it.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":930,"method":"terminal/create","params":{"sessionId":"%s","command":"sleep 300","args":[]}}\n' "$session"
            ;;
        *'"id":930'*)
            term=$(printf '%s' "$line" | sed 's/.*"terminalId":"//; s/".*//')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"c8","title":"Wait for ever","kind":"execute","status":"in_progress","content":[{"type":"terminal","terminalId":"%s"}]}}}\n' "$session" "$term"
            ;;
        *'"method":"session/prompt"'*'/echo'*)
            # An agent that sends the prompt it was just given straight
            # back. Some do. Obelus put those words on the page when the
            # reader pressed send, and they must not land twice.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"/echo"}}}}\n' "$session"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"heard you"}}}}\n' "$session"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/relay'*)
            # Words in the reader's voice that Obelus never put there --
            # which is what a replayed conversation is made of, and what
            # another client on the same session sends.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"what did we settle on"}}}}\n' "$session"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/used'*)
            # How full it is, sent as an agent sends it: several times in a
            # turn, the numbers only going up. The last one is what the row
            # shows, and it is over the mark where that stops being dim.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"usage_update","used":12000,"size":200000}}}\n' "$session"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"usage_update","used":188000,"size":200000,"cost":{"amount":1.13,"currency":"USD"}}}}\n' "$session"
            ;;
        *'"method":"session/prompt"'*'/room'*)
            # The same, well under the mark, and with no cost: not every
            # agent counts one, and the row must not leave a gap where a
            # number would have been.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"usage_update","used":62000,"size":200000}}}\n' "$session"
            ;;
        *'"method":"session/prompt"'*'/steps'*)
            # A list of what it means to do about this turn, sent whole
            # every time the way the protocol says: three entries, and then
            # the same three with the second under way.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"plan","entries":[{"content":"read the counts tree","priority":"high","status":"completed"},{"content":"wire it to the search","priority":"medium","status":"pending"},{"content":"write the test","priority":"low","status":"pending"}]}}}\n' "$session"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"plan","entries":[{"content":"read the counts tree","priority":"high","status":"completed"},{"content":"wire it to the search","priority":"medium","status":"in_progress"},{"content":"write the test","priority":"low","status":"pending"}]}}}\n' "$session"
            ;;
        *'"method":"session/prompt"'*'/plan'*)
            # A plan put to the reader for leave to act on, which is how an
            # agent in plan mode ends its turn. The plan itself is the call's
            # own words -- `ToolCallContent::Content`, the protocol's way of
            # saying a call carries text -- and it is longer than a card is
            # tall, which is the case the transcript has to carry.
            set_turn "$session" "$(id_of "$line")"
            plan="# The plan\n\n## Context\n\nThere is no hi.py here yet.\n\n## Steps\n\n1. Write the file\n2. Run it\n3. Read the output\n4. Say what happened\n5. Stop"
            # Through `%s`, not into the format: the plan has `\n` in it and
            # printf would turn those into real newlines, which is one JSON
            # message torn into eleven lines that parse as nothing.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"p1","title":"Approve Plan","kind":"switch_mode","status":"pending","content":[{"type":"content","content":{"type":"text","text":"%s"}}]}}}\n' "$session" "$plan"
            printf '{"jsonrpc":"2.0","id":908,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"p1","title":"Approve Plan","kind":"switch_mode","content":[{"type":"content","content":{"type":"text","text":"%s"}}]},"options":[{"optionId":"go","name":"Yes, go ahead","kind":"allow_once"},{"optionId":"keep","name":"No, keep planning","kind":"reject_once"}]}}\n' "$session" "$plan"
            ;;
        *'"method":"session/prompt"'*'/markdown'*)
            # What an agent actually sends: markdown. The protocol says so
            # in as many words -- "Text content. May be plain text or
            # formatted with Markdown. Clients SHOULD render this text as
            # Markdown" -- and every agent worth talking to takes it at its
            # word. A heading, emphasis, a code span and a fenced block.
            set_turn "$session" "$(id_of "$line")"
            said='## What I would do\n\nThe **cheap** part is moving `closer_for` out to `obelus-editing`:\n\n```rust\nfn closer_for(open: char) -> char {\n```\n\n- it is a pure function\n- it has nothing to do with `App`\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$session" "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/filler'*)
            # A day's conversation rather than a line of one: enough prose,
            # in enough paragraphs, that where it wraps depends on the
            # column it is wrapped at. A transcript of one short answer
            # wraps the same at any width, and the thing this is for only
            # shows where the two disagree.
            set_turn "$session" "$(id_of "$line")"
            # Lines that are exactly 72 cells: seven words of eight and one
            # of nine, with the spaces between them. A column wider and the
            # eighth word stays on the row; a column narrower and it does
            # not -- which is the whole of what this is for, and what
            # ordinary prose only does by luck.
            group="aaaaaaaa aaaaaaaa aaaaaaaa aaaaaaaa aaaaaaaa aaaaaaaa aaaaaaaa bbbbbbbbb"
            said=""
            for round in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16; do
                said="$said$group\\n\\n"
            done
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$session" "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/pair'*)
            # Two questions at once, in one conversation: two tool calls
            # an agent runs side by side, each wanting permission before
            # it goes. Only the requests carry the calls, so a row for the
            # second is in the transcript because Obelus put it there.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":940,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"q1","title":"Read the first file","kind":"read"},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n' "$session"
            printf '{"jsonrpc":"2.0","id":941,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"q2","title":"Read the second file","kind":"read"},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n' "$session"
            ;;
        *'"id":940'*|*'"id":941'*)
            # Which answer, in brackets, so a test can tell an answer the
            # reader gave from the cancellation a question pushed off the
            # card would get.
            case "$line" in
                *'"optionId":"once"'*) answered='once' ;;
                *'"optionId":"never"'*) answered='never' ;;
                *) answered='cancelled' ;;
            esac
            case "$line" in
                *'"id":940'*) which='first' ;;
                *) which='second' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"the %s was [%s] "}}}}\n' "$which" "$answered"
            if [ "$which" = 'second' ]; then
                printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            fi
            ;;
        *'"method":"session/prompt"'*'"text":"/takeback'*)
            # A question asked and then taken back before anybody answered
            # it -- the tool call it was about overtaken, the turn moving
            # on -- which is what `$/cancel_request` is for. A moment
            # between the two, so that the card is up when it goes.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":942,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"k1","title":"Delete the build","kind":"delete"},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n' "$session"
            sleep 0.3
            printf '{"jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":942}}\n'
            ;;
        *'"id":942'*)
            case "$line" in
                *'"error"'*) said='it was taken back and I was told' ;;
                *) said='it was answered after all' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/twice'*)
            # A command put to the reader by an agent that sends the tool's
            # description as the call's title *and* as the call's content.
            # claude-agent-acp does this for every command it runs, so it is
            # what a reader of Obelus actually meets: the one line arrives
            # twice, and the thing being allowed -- the command -- arrives in
            # neither, because it is in `rawInput`, the agent's own arguments
            # in the agent's own shape.
            set_turn "$session" "$(id_of "$line")"
            echoed="List crates and app crate sources"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"e1","title":"%s","kind":"execute","status":"pending","content":[{"type":"content","content":{"type":"text","text":"%s"}}],"rawInput":{"command":"ls crates","description":"%s"}}}}\n' "$session" "$echoed" "$echoed" "$echoed"
            printf '{"jsonrpc":"2.0","id":909,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"e1","title":"%s","kind":"execute","content":[{"type":"content","content":{"type":"text","text":"%s"}}],"rawInput":{"command":"ls crates","description":"%s"}},"options":[{"optionId":"yes","name":"Yes","kind":"allow_once"},{"optionId":"no","name":"No","kind":"reject_once"}]}}\n' "$session" "$echoed" "$echoed" "$echoed"
            ;;
        *'"id":909'*)
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call_update","toolCallId":"e1","status":"completed"}}}\n'
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"id":908'*)
            # What became of the asking, on the same call: it says so in
            # words too, and both belong to that one row.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call_update","toolCallId":"p1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"the reader answered"}}]}}}\n'
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'/blocks'*)
            # Says what the prompt arrived as: how many blocks, and what
            # the client put in front of the reader's words. Obelus opens
            # with one block of its own, and what is in that block depends
            # on what it has already said -- who it is talking to, which of
            # the reader's notes this is about, or that the note has been
            # rewritten since. From here the messages look alike otherwise,
            # because what tells them apart is not in the words the reader
            # typed.
            #
            # Reported as every piece it carries rather than only the one
            # in front: the pieces are joined into one block, so "what it
            # begins with" would say nothing about the rest of it.
            #
            # Read with `case` rather than counted with `grep -o`, which is
            # not in POSIX: this script is `sh` on purpose.
            set_turn "$session" "$(id_of "$line")"
            case "$line" in
                *'"text":'*'"text":'*) blocks=2 ;;
                *) blocks=1 ;;
            esac
            first=""
            case "$line" in
                *'"text":"This is Obelus, the client you are talking through'*)
                    first="always" ;;
            esac
            case "$line" in
                *"This conversation is about one of Obelus's notes"*)
                    first="${first:+$first+}note" ;;
            esac
            case "$line" in
                *"The note this conversation is about has been rewritten"*)
                    first="${first:+$first+}rewritten" ;;
            esac
            case "$line" in
                *"This project has a workflow for changing its files"*)
                    first="${first:+$first+}workflow" ;;
            esac
            # The workflow itself, which is the tool's to hand over and
            # not the opening's: a line from the middle of it.
            case "$line" in
                *'git worktree add -b'*)
                    first="${first:+$first+}steps" ;;
            esac
            : "${first:=reader}"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"blocks='"$blocks"' first='"$first"'"}}}}\n'
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/longtitle'*)
            # A call whose title is the whole command line, which is what
            # claude-agent-acp sends for every command it runs. Longer than
            # any terminal is wide, so the row it lands on has to give
            # something up -- and what it must not give up is its own
            # account of the call.
            set_turn "$session" "$(id_of "$line")"
            # No quotes or backslashes in it: this goes into a JSON string
            # unescaped, and a title that breaks the message is a test
            # about nothing.
            long='grep -rn tick-it-off crates/obelus-app/tests/agent.rs crates/obelus-ui/src/chat.rs crates/obelus-component/src/chat.rs'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call","toolCallId":"g-1","title":"%s","kind":"execute","status":"completed","content":[{"type":"content","content":{"type":"text","text":"nothing matched"}}]}}}\n' "$long"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/many'*)
            # A turn with a run of tool calls of one kind in it, which is
            # what an agent looking around a repository actually does: a
            # client that draws thirty of these has drawn a log.
            set_turn "$session" "$(id_of "$line")"
            for name in app acp buffer ui; do
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call","toolCallId":"r-%s","title":"Read src/%s","kind":"read","status":"completed","locations":[{"path":"%s/tests/fixtures/many_lines.rs","line":4}]}}}\n' "$name" "$name" "$here"
            done
            # And one that failed, after them: the run it belongs to is not
            # the same run, because what failed is not a read.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call","toolCallId":"x-1","title":"Run the tests","kind":"execute","status":"failed"}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"that is where it is"}}}}\n'
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/edit'*)
            # An agent asking to change a file: the call carries the file as
            # it is and as it would be, which the protocol sends instead of
            # a patch, and the client is the one that works out the diff.
            set_turn "$session" "$(id_of "$line")"
            before='fn step_rows(row: usize) -> usize {\n    row\n}\n'
            after='fn step_rows(row: ScreenRow) -> usize {\n    row.get()\n}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call","toolCallId":"e-1","title":"Edit the file","kind":"edit","status":"pending","content":[{"type":"diff","path":"%s/tests/fixtures/many_lines.rs","oldText":"%s","newText":"%s"}]}}}\n' "$here" "$before" "$after"
            printf '{"jsonrpc":"2.0","id":907,"method":"session/request_permission","params":{"sessionId":"'"$session"'","toolCall":{"toolCallId":"e-1"},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n'
            ;;
        *'"id":907'*)
            case "$line" in
                *'"optionId":"once"'*) said='I changed it' ;;
                *) said='I left it alone' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call_update","toolCallId":"e-1","status":"completed"}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/nowhere'*)
            # A tool call naming a file that is not there, which is what an
            # agent that deleted one -- or made one up -- sends.
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call","toolCallId":"g-1","title":"Read the missing file","kind":"read","status":"completed","locations":[{"path":"%s/tests/fixtures/not-here.rs","line":2}]}}}\n' "$here"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
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
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","id":905,"method":"elicitation/create","params":{"mode":"form","sessionId":"'"$session"'","message":"which parts should I look at","requestedSchema":{"type":"object","properties":{"areas":{"type":"array","title":"Areas","minItems":2,"items":{"anyOf":[{"const":"app","title":"src/app","description":"the application"},{"const":"acp","title":"src/acp","description":"the agent link"},{"const":"ui","title":"src/ui","description":"the screen"}]}},"other":{"type":"string","title":"Other","description":"Anywhere else it should look"}},"required":["areas"]}}}\n'
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
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/ask'*)
            set_turn "$session" "$(id_of "$line")"
            if [ -z "$forms" ]; then
                printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"this client cannot be asked"}}}}\n'
                printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            else
                printf '{"jsonrpc":"2.0","id":903,"method":"elicitation/create","params":{"mode":"form","sessionId":"'"$session"'","message":"which way should I do it","requestedSchema":{"type":"object","properties":{"how":{"type":"string","title":"How","oneOf":[{"const":"fast","title":"Quickly"},{"const":"careful","title":"Carefully","description":"and slowly"}]},"sure":{"type":"boolean","title":"Sure"},"times":{"type":"integer","title":"Times","minimum":1,"maximum":9}},"required":["how","sure","times"]}}}\n'
            fi
            ;;
        *'"id":903'*)
            case "$line" in
                *'"action":"accept"'*)
                    how=$(printf '%s' "$line" | sed -n 's/.*"how":"\([^"]*\)".*/\1/p')
                    sure=$(printf '%s' "$line" | sed -En 's/.*"sure":(true|false).*/\1/p')
                    times=$(printf '%s' "$line" | sed -n 's/.*"times":\([0-9.]*\).*/\1/p')
                    # In brackets, so a test can say exactly what came
                    # back: a whole number sent as a float would otherwise
                    # read the same as far as the words go.
                    said="you said [$how] [$sure] [$times]"
                    ;;
                *) said='you would not say' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$said"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'"text":"/broken'*)
            printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"nobody has signed in"}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/prompt"'*'"text":"/'*)
            # A command: the text starts with a slash, and everything after
            # the name is the command's own input.
            set_turn "$session" "$(id_of "$line")"
            asked=$(printf '%s' "$line" | sed -n 's/.*"text":"\/\([^" ]*\).*/\1/p')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ran %s"}}}}\n' "$asked"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'quickly'*)
            # It changes a setting of its own accord and says so, which is
            # the other direction that path runs in: agents pick a model to
            # suit what they were asked and tell the client afterwards.
            set_turn "$session" "$(id_of "$line")"
            model='fast'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"config_option_update","configOptions":%s}}}\n' "$(options)"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/prompt"'*'slowly'*)
            # Asked to take its time: it says nothing and answers nothing,
            # so the turn stays in flight until Obelus cancels it.
            set_turn "$session" "$(id_of "$line")"
            ;;
        *'"method":"session/prompt"'*)
            set_turn "$session" "$(id_of "$line")"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"working it out"}}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"it is "}}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a rust file"}}}}\n'
            # With the kind and the file it is about, the way a real agent
            # sends them: the kind is what the client draws a glyph from,
            # and the location is what makes the row somewhere to go.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read the file","kind":"read","status":"in_progress","locations":[{"path":"%s/tests/fixtures/many_lines.rs","line":7}]}}}\n' "$here"
            printf '{"jsonrpc":"2.0","id":900,"method":"fs/read_text_file","params":{"sessionId":"'"$session"'","path":"tests/fixtures/read-me.txt"}}\n'
            ;;
        *'"id":900'*)
            # What Obelus handed back, quoted into a chunk so the test can
            # see that it was the buffer's text and not the disk's.
            # Up to the first quote or backslash: the answer ends with an
            # escaped newline, and what the test looks for is the words.
            text=$(printf '%s' "$line" | sed -n 's/.*"content":"\([^"\\]*\).*/\1/p')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" saying %s"}}}}\n' "$text"
            # And a write outside the tree, which Obelus refuses: an agent
            # inside a reader may change what the reader is looking at and
            # nothing else.
            printf '{"jsonrpc":"2.0","id":902,"method":"fs/write_text_file","params":{"sessionId":"'"$session"'","path":"/tmp/obelus-not-in-the-tree.txt","content":"no"}}\n'
            ;;
        *'"id":902'*)
            case "$line" in
                *'"error"'*) wrote='refused to write' ;;
                *) wrote='wrote the file' ;;
            esac
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" and it %s"}}}}\n' "$wrote"
            printf '{"jsonrpc":"2.0","id":901,"method":"session/request_permission","params":{"sessionId":"'"$session"'","toolCall":{"toolCallId":"t2","title":"Run the tests","kind":"execute","content":[{"type":"content","content":{"type":"text","text":"cargo test --all-features"}}],"locations":[{"path":"/tmp/obelus/Cargo.toml"}]},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Reject","kind":"reject_once"}]}}\n'
            ;;
        *'"id":901'*)
            case "$line" in
                *'"optionId":"once"'*) allowed='allowed' ;;
                *) allowed='refused' ;;
            esac
            # Both of them finish: the file it read, and the command it
            # asked about. An agent says how a call ended whether or not it
            # had to ask first.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"tool_call_update","toolCallId":"t2","status":"completed"}}}\n'
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"'"$session"'","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" and I was %s"}}}}\n' "$allowed"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(turn_of "$session")"
            ;;
        *'"method":"session/load"'*'"sessionId":"s-gone"'*)
            # A conversation the agent no longer has. Real agents sweep
            # theirs up, and one that has been swept is the case a client
            # has to survive: it asked for a name that means nothing here.
            printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32602,"message":"no such session"}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/load"'*)
            # Asked of an agent that said at the handshake it cannot. A
            # client reading what was declared never sends this, and one
            # that sends it anyway has to be told -- which is how a test
            # tells the two apart.
            if [ "$again" != load ]; then
                printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"this agent cannot replay a conversation"}}\n' "$(id_of "$line")"
                continue
            fi
            # A conversation taken up again. A real agent replays what was
            # said; what matters here is that it answers about the session
            # the client named rather than minting a new one, because that
            # is the whole of what the client has to get right.
            #
            # Which is read out of the request. It used to replay into
            # `$session`, the last one *opened* -- and a client that opens
            # one on its way up and then asks for an old one by name gets
            # the replay addressed to the wrong conversation, where it is
            # dropped as being about nothing on screen. The agent was the
            # one getting it wrong, and it was the agent every test used to
            # decide whether the client had it right.
            loaded="$(session_of "$line")"
            # And whether the client said where its own tools are. A real
            # agent connects to them at the handshake and keeps what it was
            # given; Obelus offers them on a port the machine hands out
            # afresh every run, so a conversation taken up in a later run
            # has to be told the new one or the agent goes on calling a
            # port that died with the process that named it. Said back, so
            # a test can see it.
            case "$line" in
                *'"mcpServers"'*'"url"'*) tools=given ;;
                *) tools=missing ;;
            esac
            # Both halves, in the order they were said. The reader's own
            # comes back as `user_message_chunk` -- a client that dropped
            # those would take up a conversation of answers with no
            # questions above them.
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"what did we settle on"}}}}\n' "$loaded"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"tools=%s"}}}}\n' "$loaded" "$tools"
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"where we were"}}}}\n' "$loaded"
            printf '{"jsonrpc":"2.0","id":%s,"result":{"modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"ask first"},{"id":"code","name":"write code"}]},"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            ;;
        *'"method":"session/resume"'*)
            # Taken up with its context and not a word of it sent back,
            # which is the whole difference from `session/load`. It says so
            # in the answer only; the page stays empty.
            printf '{"jsonrpc":"2.0","id":%s,"result":{"modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"ask first"},{"id":"code","name":"write code"}]},"configOptions":%s}}\n' "$(id_of "$line")" "$(options)"
            ;;
        *'"method":"session/delete"'*)
            set_turn "$session" ''
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$(id_of "$line")"
            ;;
        # Answered too, though delete always works here: a request with no
        # answer holds the client's loop shut behind it, and close is what
        # a client falls back on.
        *'"method":"session/close"'*)
            set_turn "$session" ''
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$(id_of "$line")"
            ;;
        *'"method":"session/cancel"'*)
            # Only when there is a turn to cancel. A cancellation that
            # arrives with nothing in flight is a no-op, and answering it
            # with an id nobody sent is a message no client can read.
            pending="$(turn_of "$session")"
            if [ -n "$pending" ]; then
                printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$pending"
                set_turn "$session" ''
            fi
            ;;
    esac
done

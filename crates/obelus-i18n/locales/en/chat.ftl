## A conversation with the agent.

# Beside the box while a turn is running, on the key that sends now.
sends-it-now = Sends it now
# The way back to the end of the conversation, where nothing new has come.
to-the-end = To the end
new-messages = { $count ->
    [one] { $count } new message
   *[other] { $count } new messages
}
# After a tool call's title, where the title named the file already.
line = line { $line }
# The agent's background work, on the conversation's row.
finished = { $count } finished
in-the-background = { $count } in the background
nothing-to-change = No settings
# A key on the conversation's row that goes back to the note it is about.
the-note = Note
mode = Mode
# Takes the grey suggestion into the box.
fill-it-in = Complete
no-agent = No agent
# What the transcript says before anything has been said.
no-agent-is-active = No agent is chosen. Choose one in the settings
it-stopped = The agent stopped. Ask something to start it again
starting = Starting…
ask-it-something = Ask it something

## The keys of the box, as the palette says them over a conversation.

send-now = Send now
send-now-about = Stop the running turn and send this
new-line = New line
new-line-about = Insert a line break
stop = Stop
stop-about = Stop the agent
mode-about = Switch the agent to its next mode
# Goes to the page of what the conversation is about.
read = Open
read-pull-request = Go to the pull request's page
read-issue = Go to the issue's page

## Where a tool call has got to, where there are no glyphs to say it.

waiting = Waiting
running = Running
done = Done
# Returned, with what it started still going.
background = Background
stopped = Stopped
failed = Failed

## A question the agent asks.

submit = Submit
decline = Decline

## What the transcript lays out.

# A folded plan: the step being worked on, by its place and its words.
step-of = Step { $at } of { $total } — { $step }
steps = { $count ->
    [one] { $count } step
   *[other] { $count } steps
}
# The heading over a run of tool calls.
run-of-files = { $count ->
    [one] { $count } file
   *[other] { $count } files
}
run-of-calls = { $count ->
    [one] { $count } call
   *[other] { $count } calls
}
# The heading over the agent's thinking.
thought = Thought

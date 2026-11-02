## What Obelus says that belongs to no one view.

# Two things said on the status row at once, the second after the first.
and-then = { $first }; { $then }
# A machine that would not say its name.
another-machine = Another machine
# Over the list of what went wrong while Obelus started.
problems-starting-up = Problems starting up
no-tools-for-agents = Agents cannot reach Obelus's tools
nothing-is-watched = Not watching for changes, so files changed elsewhere will not reload
could-not-open-the-link = Could not open the link

## The conversation's row.

question-waiting = A question is waiting for when you stop typing
thinking = Thinking…

## Terminals.

could-not-start-the-terminal = Could not start the terminal: { $why }
# A command line, which is a name and keeps its own spelling.
still-running = Still running: { $command }
terminals-still-running = { $count ->
    [one] A terminal is still running
   *[other] { $count } terminals are still running
}
stop-and-close = Stop it and close
stop-and-quit = { $count ->
    [one] Stop it and quit
   *[other] Stop them and quit
}
stop-and-switch = { $count ->
    [one] Stop it and switch
   *[other] Stop them and switch
}
stop-and-close-the-project = { $count ->
    [one] Stop it and close the project
   *[other] Stop them and close the project
}
stop-and-close-others = { $count ->
    [one] Stop it and close others
   *[other] Stop them and close others
}

## Signing in to an agent, said in the conversation.

sign-in-would-not-start = Could not start signing in: { $why }
signed-in = Signed in
sign-in-stopped = Signing in did not finish: it was stopped by { $signal }
sign-in-exited = Signing in did not finish: exit code { $code }

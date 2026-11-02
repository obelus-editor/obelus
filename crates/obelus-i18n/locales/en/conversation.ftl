## What Obelus says in a conversation with the agent, and about one.
## What it says *to* the agent is a prompt, and not here.

# The settings name a different agent from the one this conversation had.
another-agent = This conversation was with another agent
files-go-to-agents = A file can only be dropped into a conversation
open-link = Open link
# The way out of a card that asks the reader to sign in, or to go to a link.
not-now = Not now
not-opened = Not opened
declined = Declined
not-answered = Not answered
stopped-waiting = The agent stopped waiting for an answer
# One of the agent's settings, changed from the conversation's row: the
# agent's name for it, and the agent's name for its new value.
set-to = { $name }: { $value }
# A conversation an agent closed, by its name, where the reader was elsewhere.
closed = Closed { $name }
closed-a-conversation = Closed a conversation
# The conversation was taken up again, by an agent that keeps it but
# cannot send back what was said in it.
taken-up = Resumed. This agent cannot show the earlier messages
# The agent's reason, after the colon.
starting-again = Starting again: { $why }
refused-to-answer = The agent declined to answer
out-of-room = The agent ran out of room to answer
# What went wrong, in the agent's words.
agent-said = The agent: { $why }
agent-stopped = The agent stopped
agent-stopped-because = The agent stopped: { $why }
# The agent no longer offers what the reader chose for one of its settings.
no-longer-offered = No longer offered by { $agent }: { $value } for { $setting }, so this conversation uses { $current }

## Something Obelus asked of the agent did not work: the agent's words
## after the colon.

could-not-ask-what-it-offers = Could not ask the agent for its settings: { $why }
could-not-put-the-question = Could not show the agent's question: { $why }
could-not-change-the-setting = Could not change the setting: { $why }
could-not-stop-the-task = Could not stop the background task: { $why }
could-not-change-the-mode = Could not change the mode: { $why }

## Signing in.

no-way-to-sign-in = The agent offers no way to sign in here
no-way-to-sign-in-to = { $agent } offers no way to sign in here
sign-in = Sign in to the agent to continue
sign-in-to = Sign in to { $agent } to continue
not-signed-in = Not signed in
# The way the reader chose, in the agent's words.
signing-in = Signing in: { $way }

## The agent's questions, answered on a card. A field is the agent's word
## for what it asks.

answered = { $field }: { $answer }
left-blank = { $field }: left blank
on = On
off = Off
not-a-number = { $field } must be a number
# The question, as the card put it.
out-of-range = Out of range: { $question }
whole-number = { $field }: a whole number
number = { $field }: a number
whole-number-between = { $field }: a whole number from { $least } to { $most }
number-between = { $field }: a number from { $least } to { $most }
whole-number-at-least = { $field }: a whole number, { $least } or more
number-at-least = { $field }: a number, { $least } or more
whole-number-at-most = { $field }: a whole number, { $most } or less
number-at-most = { $field }: a number, { $most } or less

## A command the agent ran, under what it printed.

output-not-kept = … more output, not kept
command-stopped-by = … stopped by { $signal }
command-exited = … exit code { $code }

## The agent's background work.

filter-background-tasks = Filter background tasks
background-tasks = Background tasks
no-background-tasks = No background tasks
output-gone = Its output file no longer exists
no-output = This task has no output file
stopping = Stopping
paused = Paused
# The heading over the work that has ended.
finished = Finished

## The agent's own list of commands, after a /.

filter-the-agents-commands = Filter the agent's commands

## A plan the agent wrote, opened as a document.

plan = Plan
# By the plan's heading, or the conversation's name.
plan-called = Plan: { $name }

## The agents.

nothing-for-this-machine = Not available for this machine
nowhere-to-install = There is nowhere on this system to install agents
agent-not-installed = { $agent } is not installed
install-it = { $agent } is not installed. Install it in the settings
asking-what-it-offers = Asking { $agent } for its settings…
nothing-to-set = No settings for { $agent }
not-heard-what-it-offers = No answer from { $agent } about its settings

## The list of conversations.

filter-conversations = Filter conversations
# What a conversation is called that neither the agent nor a note has named.
untitled = Untitled
history-unreadable = Could not read this project's conversation history.
only-in-its-checkout = A conversation can only be resumed in the worktree it was started in.
only-by-its-agent-and-none-chosen = No agent is chosen, and a conversation can only be resumed by the agent that had it.
only-by-its-agent = The agent in use is { $agent }, and a conversation can only be resumed by the agent that had it.
# A row for starting a conversation from what the reader was on: a place in
# a file, or a note.
conversation-about = Conversation about { $title }
new-conversation-about = New conversation about { $title }
# A place in a file, as read at a commit rather than from the disk.
place-at-commit = { $place } at { $commit }

## What the transcript says Obelus told the agent with the reader's words.

told-where-it-started = Told the agent where this conversation was started from
told-the-note = Told the agent what this conversation is about
told-the-note-was-reworded = Told the agent the note has been rewritten
told-which-issue = Told the agent which issue to answer
told-what-was-said = Told the agent what has been said since
told-which-pull-request = Told the agent which pull request to review
told-what-was-pushed = Told the agent what has been pushed since

## What the box offers to say, in grey, in a conversation about something.
## The reader's own words once taken, so said as the reader would say them.

look-into-this = Look into this
review-this = Review this pull request
review-what-was-pushed = Review what has been pushed since
answer-this = Read this issue and suggest a reply
answer-what-was-said = Read what has been said since

## The workflows, on the list the `workflow` setting is chosen from: what
## each is called, and after `-about` what it does.

workflow-none = No workflow
workflow-none-about = Obelus asks nothing of the agent: it makes a change its own way, usually straight in the checkout you are reading.
workflow-in-place = In place
workflow-in-place-about = The agent changes the checkout you are reading, so each change arrives in the file in front of you and undoes like any other. It runs the tests before and after and says what it ran, waits for you to try it, offers to review it, and then asks whether to commit it here, put it on a branch for a pull request, or leave it uncommitted. Nothing is committed or pushed until you say so.
workflow-feature-branch = Feature branch
workflow-feature-branch-about = The agent works on a branch in a worktree of its own under .worktree, and tells you where to try it and what it ran, offering to start it if it can. Once you accept it, it asks whether to review it, then whether it goes back as a merge or a pull request or waits for later. A pull request is merged when its checks pass — by you, from a link it gives you, where gh is not installed and signed in — and then it asks whether to bring your checkout up to it, if that is the branch you are on. The worktree is removed only when you say so.

## What Obelus says about a language server and what it answered.

# What to call a server that has no program of its own name.
the-language-server = The language server
no-server-for-this-file = No language server for this file
no-server-to-ask = No language server to ask
not-installed = { $command } is not installed
servers-are-off = Language servers are off in the settings
no-server-running-for = No language server running for { $language }
no-server-for = No language server for { $language }
still-starting = { $server } is still starting
not-listening = The language server is not listening
did-not-answer = The language server did not answer
not-responding = { $command } is not responding
would-not-start = { $command } could not start
was-not-running = { $command } was not running
no-file-to-restart = No file to restart a language server for
no-file-to-stop = No file to stop a language server for
# Where a server answered with an error and no words of its own.
no-message = The language server reported an error with no message
still-indexing = Still indexing
asking = Asking the language server…
changed-while-asking = The file changed while asking
not-a-file-to-change = This file cannot be edited

## What a server does not offer.

no-completions = { $server } does not offer completions
no-signatures = { $server } does not offer parameter hints
no-code-actions = { $server } does not offer code actions
no-hover = { $server } does not offer hover information
no-renaming = { $server } does not support renaming
answers-none-of-these = The language server supports none of these

## Code actions.

asking-for-actions = Getting code actions…
no-actions-here = No code actions here
# The server's own reasons, after the colon.
no-actions-because = No code actions here: { $reasons }
code-actions = Code actions
filter-code-actions = Filter code actions
left-unfinished = The language server left this action unfinished
could-not-resolve = The language server could not work this action out
# The action's title, as the server wrote it.
did-nothing = { $action } did nothing
# The command's title, as the server wrote it.
asked-to-run = Asked the language server to run: { $command }
running-actions-on-save = Running code actions…

## The edits a server asks for.

nothing-to-change = Nothing to change
changed-files = { $changed ->
    [one] Changed 1 file, not saved yet
   *[other] Changed { $changed } files, not saved yet
}
# Some of the files changed were not open, and are open now.
changed-and-opened = { $changed ->
    [one] Changed 1 file and opened it, not saved yet
   *[other] Changed { $changed } files, { $opened } newly opened, not saved yet
}
not-changed = { $failed ->
    [one] 1 file could not be changed
   *[other] { $failed } files could not be changed
}
# What Obelus refuses to do, listed after the colon.
will-not = Obelus does not do this: { $what }
creating-a-file = creating a file
moving-a-file = moving a file
deleting-a-file = deleting a file
# Between the items of a list in a sentence.
list-separator = {", "}

## Renaming.

not-on-a-name = The cursor is not on a name
renaming-to = Renaming to { $name }…
changed-while-renaming = The file changed while renaming
renamed-nothing = The language server renamed nothing
no-file-to-rename = No file to rename
checking-the-rename = Checking what renaming { $path } changes…
not-ready-to-update = The language server was not ready to update anything
could-not-rename = Could not rename: { $error }
renamed-to = Renamed to { $path }

## Symbols, places and the outline.

no-symbol-here = No symbol here
# A question about the symbol, by its command's title.
nothing-found = { $action }: nothing found
filter-places = Filter places
no-file-to-outline = No file to outline
filter-symbols = Filter symbols
no-symbols-in-this-file = No symbols in this file
no-outline-for-this-language = No outline for this language
changed-while-formatting = The file changed while formatting
no-symbol-matches = No symbol matches

## Calls.

nothing-to-follow = Nothing here to follow
filter-calls = Filter calls
callers = Callers
calls = Calls
nothing-calls-that = Nothing calls that
calls-nothing = That calls nothing

## Problems.

no-problems-in-this-file = No problems in this file
no-problems-reported = No problems reported in this project
filter-problems = Filter problems
tab-file = File
tab-project = Project
errors = { $count ->
    [one] { $count } error
   *[other] { $count } errors
}
warnings = { $count ->
    [one] { $count } warning
   *[other] { $count } warnings
}
infos = { $count } info
hints = { $count ->
    [one] { $count } hint
   *[other] { $count } hints
}
# Above the problems: how many of each, then where.
problems-in-file = { $problems } in { $file }
problems-in-files = { $files ->
    [one] { $problems } in 1 file
   *[other] { $problems } in { $files } files
}

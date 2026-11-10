## Copying and cutting: what the status row says the reader got.

copied-selection = Copied selection
copied-line = Copied line
copied-code = Copied code
copied-message = Copied message
copied-note = Copied note
copied-answer = Copied answer
# Slack's app manifest, from the chat settings.
copied-manifest = Copied the manifest
copied-path = Copied path
copied-full-path = Copied full path
not-copied-selection = Could not copy selection
not-copied-line = Could not copy line
not-copied-code = Could not copy code
not-copied-message = Could not copy message
not-copied-note = Could not copy note
not-copied-answer = Could not copy answer
not-copied-manifest = Could not copy the manifest
not-copied-path = Could not copy path
not-copied-full-path = Could not copy full path
cut-selection = Cut selection
cut-line = Cut line
cut-code = Cut code
cut-message = Cut message
cut-note = Cut note
cut-answer = Cut answer
cut-manifest = Cut the manifest
cut-path = Cut path
cut-full-path = Cut full path
# The text has gone from where it was, and the clipboard would not take it.
cut-not-copied-selection = Cut selection, but could not copy it
cut-not-copied-line = Cut line, but could not copy it
cut-not-copied-code = Cut code, but could not copy it
cut-not-copied-message = Cut message, but could not copy it
cut-not-copied-note = Cut note, but could not copy it
cut-not-copied-answer = Cut answer, but could not copy it
cut-not-copied-manifest = Cut the manifest, but could not copy it
cut-not-copied-path = Cut path, but could not copy it
cut-not-copied-full-path = Cut full path, but could not copy it

## Pasting.

nothing-to-paste = Nothing to paste
# A picture pasted from a machine at the other end of a connection.
bringing-the-picture = Bringing the picture across
takes-no-pictures = This agent does not take pictures
pictures-go-to-agents = A picture goes in a message to an agent

## Going somewhere.

could-not-open = Could not open { $path }
nowhere-further-back = Nowhere further back
nowhere-further-forward = Nowhere further forward
no-file-open = No file open
unknown-language = Not a language Obelus knows
no-bracket-here = No bracket here
nothing-wider-to-select = Nothing wider to select
# What was typed for go-to-line, in quotes.
not-a-line-number = { $text } is not a line number

## Editing.

nothing-to-undo = Nothing to undo
nothing-to-redo = Nothing to redo
no-line-comment = No line comment in this language

## The lists.

no-theme-is-built-in = No theme is built in
filter-encodings = Filter encodings
# After choosing an encoding the file cannot be read again in.
written-as = Saved as { $encoding } from now on
filter-values = Filter values
filter-commands = Filter commands
# The palette's first tab: the keys of the view under it.
here = Here
# A key of a pair, by the word for what the pair does.
does-up = { $does } up
does-down = { $does } down
does-left = { $does } left
does-right = { $does } right
no-log-file = No log file
no-server-log-file = No language server log file
filter-files = Filter files
no-changes = No changes
no-files-in-this-folder = No files in this folder
# Beside a changed file that is a submodule.
submodule = Submodule
# Where a setting is held by a project's settings file.
set-by = Set by { $path }
# The name of a conversation the agent has not named yet.
a-conversation = Conversation

## Asking before something is lost.

unsaved-file = { $path } is unsaved
unsaved-files = { $files ->
    [one] 1 file is unsaved
   *[other] { $files } files are unsaved
}
unsaved-file-and-terminals = { $terminals ->
    [one] { $path } is unsaved, and a terminal is still running
   *[other] { $path } is unsaved, and { $terminals } terminals are still running
}
unsaved-files-and-terminals = { $files ->
    [one] { $terminals ->
        [one] 1 file is unsaved, and a terminal is still running
       *[other] 1 file is unsaved, and { $terminals } terminals are still running
    }
   *[other] { $terminals ->
        [one] { $files } files are unsaved, and a terminal is still running
       *[other] { $files } files are unsaved, and { $terminals } terminals are still running
    }
}
save-and-close = Save and close
close-without-saving = Close without saving
save-all-and-quit = Save all and quit
quit-without-saving = Quit without saving
save-all-and-switch = Save all and switch
switch-without-saving = Switch without saving
save-all-and-close = Save all and close
save-all-and-close-others = Save all and close others
close-others-without-saving = Close others without saving
changes-will-be-lost = Your changes will be lost
changed-on-disk = { $path } changed on disk
overwrite = Overwrite
overwrite-about = What is on disk will be lost
use-the-disk = Use the version on disk
use-the-disk-about = Undo brings yours back
deleted = { $path } was deleted
write-it-back = Write it back

## Opening and saving.

no-longer-exists = { $path } no longer exists
is-a-directory = { $path } is a directory
could-not-open-because = Could not open { $path }: { $error }
outside-the-project = { $path } is outside the project
not-a-directory = { $path } is not a directory
could-not-create = Could not create { $path }
could-not-create-because = Could not create { $path }: { $error }
already-exists = { $path } already exists
no-file-to-close = No file to close
nothing-to-preview = Nothing to preview in this file
no-file-to-preview = No file to preview
is-a-plan = This is an agent's plan, not a file
is-a-commits-version = This is a commit's version, not the file
nothing-to-save = Nothing to save
formatting = Formatting…
reverted = Reverted to the version on disk. Undo brings yours back
could-not-read-the-file = Could not read the file
# Why a file was not opened, after a colon.
not-text-in-any-encoding = not text in any encoding Obelus can write back
# The encoding the file is written in has no way to write a character in it.
not-saved-unencodable = Could not save { $name }: { $encoding } cannot encode { $character }
not-saved-named = Could not save { $name }
not-saved = Could not save

## Searching.

search-this-file = Search this file
search-the-project = Search the project
search-the-projects-symbols = Search the project's symbols
search-this-preview = Search this preview
search-this-conversation = Search this conversation
search-this-page = Search this page
search-the-notes = Search the notes
type-to-search-this-file = Type to search this file
type-to-search-this-preview = Type to search this preview
type-to-search-this-conversation = Type to search this conversation
type-to-search-this-page = Type to search this page
type-to-search-the-notes = Type to search the notes
no-match-in-this-file = No match in this file
no-match-in-this-preview = No match in this preview
no-match-in-this-conversation = No match in this conversation
no-match-on-this-page = No match on this page
no-match-in-the-notes = No match in the notes
nothing-here-to-search = Nothing here to search
nowhere-to-search = Nowhere to search
no-match-in-the-project = No match in the project
not-a-pattern = Not a valid pattern
type-to-search-every-file = Type to search every file
searching = Searching…
type-to-search-symbols = Type to search the project's symbols

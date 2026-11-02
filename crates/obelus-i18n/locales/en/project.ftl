## The project, and what Obelus keeps about it: its notes, its settings,
## its themes, the projects and the machines it has been opened on.

## The notes.

# Another window deleted a note this one was changing.
note-taken-away = That note was deleted elsewhere
notes-unreadable = Could not read the notes file
notes-unreadable-not-written = Could not read the notes file, so nothing was saved
notes-not-written = Could not save the notes

## The settings file.

not-saved-unreadable = Not saved: the settings file could not be read
# The system's own words, after the colon.
not-saved-because = Not saved: { $error }
settings-unreadable = Could not read the settings file, so nothing will be saved
nowhere-for-settings = There is nowhere on this system for a settings file
settings-file-not-written = Could not create the settings file: { $error }

## A line of a settings file that did nothing, marked on the line. A name
## is in backticks, the way a compiler writes one, and a sentence never
## starts with one.

no-such-setting = No setting is called `{ $key }`
not-for-a-project = A project may not set `{ $key }`
not-a-table = The value of `{ $key }` must be a table
no-such-choice = Not one of the choices: `{ $word }`
no-such-theme = No theme is called `{ $theme }`
no-such-command = No command is called `{ $command }`
no-such-key = No key is called `{ $key }`
# Why that key cannot be bound, after the colon.
key-not-allowed = No command can be on `{ $key }`: { $why }

## The settings' values, as the page says them.

# The value the agent's own setting is on where the reader chose none.
agents-own-value = Whatever the agent starts a conversation with
# Beside the value a list of them is on now.
current = Current
jobs-unlimited = Unlimited
jobs-all = { $cpus ->
    [one] All 1 CPU
   *[other] All { $cpus } CPUs
}
jobs-half = { $cpus ->
    [one] Half: { $jobs } of 1 CPU
   *[other] Half: { $jobs } of { $cpus } CPUs
}
# How long conversations are kept: 0 keeps them all.
days-forever = Forever
days-week = A week
days-month = A month
days-three-months = Three months
days-year = A year
days = { $days ->
    [one] 1 day
   *[other] { $days } days
}
filter-fonts = Filter fonts on this machine

## The keymaps.

choose-a-keymap = Choose a keymap
# What a keymap is, then the keys it puts two things on.
layout-gloss = { $gloss }. { $keys }
layout-keys = Open a file with { $open }, run a command with { $run }
layout-open-key = Open a file with { $open }
layout-run-key = Run a command with { $run }
# By the keymap's name.
reset-every-key-to = Reset every key to { $layout }?
reset-every-key = Reset every key to its keymap?
reset-them = Reset them
keep-them = Keep them

## Themes.

# Where a theme comes from, beside its name: the reader's own, or taken
# from the registry. Nothing is said for one built in.
theme-local = Local
theme-downloaded = Downloaded
more-themes = More themes
no-theme-by-that-name = No theme by that name. Press Enter to look in More themes
nowhere-to-keep-themes = There is nowhere on this system to keep a theme
theme-unreadable = Could not read the theme { $theme }
# Marked on the theme's file, over what the parser said.
this-theme-unreadable = Could not read this theme
# A theme the registry no longer lists goes for good, so it asks twice.
delete-again = Delete again to remove { $theme } for good

## Projects and machines.

projects-unreadable = Could not read the list of projects: { $why }
filter-projects = Filter projects
no-other-projects = No other project has been opened
filter-this-folder = Filter this folder
new-connection = New connection
filter-machines = Filter machines
# The words in quotes are what `ob --server` prints, in English.
not-a-server-key = Not a server key: it is the letters after “gets in with the key”

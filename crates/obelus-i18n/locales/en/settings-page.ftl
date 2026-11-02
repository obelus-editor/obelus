## The settings page: its tabs, its rows' words, and the keys it says.
## A setting's own name and gloss are in settings.ftl.

tab-settings = Settings
tab-keys = Keys
tab-agents = Agents
tab-chat = Chat
tab-themes = Themes

# What the filter box says before anything is typed, on each tab.
filter-keys = Filter keys
filter-agents = Filter agents
filter-chat = Filter chat settings
filter-settings = Filter settings
filter-themes = Filter themes

no-command-matches = No command matches
no-setting-matches = No setting matches
no-agent-matches = No agent matches
no-theme-matches = No theme matches

# Where a setting's value comes from, beside it.
scope-project = Project
scope-global = Global
scope-default = Default
# On the keys tab: a command on keys other than its keymap's.
scope-changed = Changed

## The keys, as said on a row and on the palette's first tab: a word, and
## after `-about` what it does.

change = Change
change-key-about = Change this key
change-setting-about = Change this setting
pair = Pair
pair-about = Make a code to pair the chat with
copy = Copy
copy-about = Copy this value
open = Open
open-steps-about = Open the setup steps
add = Add
add-variable-about = Add an environment variable for the agent
# A number being turned in its box.
turn = Adjust
turn-about = Step the number down or up
keep = Keep
keep-about = Keep the number in the box
cancel = Cancel
cancel-about = Leave the number as it was
remove = Remove
remove-key-about = Remove this key
remove-variable-about = Remove this environment variable
remove-theme-about = Uninstall this theme
add-key = Add key
add-key-about = Give this command another key
reset = Reset
reset-about = Give this command back its keymap's keys
reset-all = Reset all
reset-all-about = Give every command back its keymap's keys
forget = Clear
forget-about = Clear what is saved for this field
# An agent's setting the reader leaves to the agent.
agents-own = Agent's own
agents-own-about = Leave this to the agent
unset = Unset
unset-about = Remove this setting from the project's settings file
use-theme = Use
use-theme-about = Use this theme
update = Update
update-theme-about = Update this theme
install = Install
install-theme-about = Install this theme

## The keys tab.

key-is-taken = { $key } is bound to { $command }
press-the-new-key = Press the new key, or { $key } to remove it
press-a-key = Press a key to add it

## The agent's settings.

# Under the agent's name.
when = These apply to new conversations
# Under the heading of the agent's environment variables.
started = These apply the next time the agent starts
environment-of = { $agent } · Environment
add-a-variable = Add a variable
add-a-variable-about = A name, then its value
no-longer-offered = No longer offered by { $agent }, so a new conversation starts on its own

## The agents and the themes, as cards.

agents-failed = Could not fetch the list of agents: { $why }
fetching-agents = Fetching the list of agents…
themes-failed = Could not fetch the list of themes: { $why }
fetching-themes = Fetching the list of themes…
active = Active
in-use = In use
installed = Installed
no-longer-listed = No longer listed
installing = Installing…
updating = Updating…
installing-percent = Installing { $percent }%
updating-percent = Updating { $percent }%
installing-left = Installing { $percent }% · { $seconds }s left
updating-left = Updating { $percent }% · { $seconds }s left
wallpaper-licensed = Wallpaper { $license }
# A wallpaper nobody may hand on, so it is fetched from its author.
wallpaper-from-author = Wallpaper from its author
no-wallpaper = No wallpaper
# The reader's own theme file of the same name stands in front of it.
shadowed = Your own { $theme } is used instead

## The fonts setting, where no font is chosen.

system-font = System default

## The chat tab.

chat-heading = Chat
platform = Platform
platform-about = The chat notes can be worked on from
people = People
people-about = Who may talk to this machine through it
pair-row-about = A code to send in the group you added the bot to
off = Off
not-set = Not set
nobody = Nobody
make-a-code = Make a code
refused-by = Refused by { $platform }
refused = Refused
tokens-unreadable = The saved tokens could not be read

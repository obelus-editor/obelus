# Obelus, themed by omarchy.
#
# Rendered into ~/.local/state/omarchy/current/theme/obelus.toml every time
# a theme is set, and read from there through a link. See the README beside
# this file.
#
# What is not here is not an omission. A theme file gives what it wants and
# inherits the rest from the built-in theme it names, so the colours omarchy
# has no word for -- the wash behind a changed line, the ground behind a
# matched character -- are left to Obelus, which does have one.

base = "{{ mode }}"

background = "{{ background }}"
foreground = "{{ foreground }}"

# Line numbers: the dim ones, and the cursor's own.
gutter = "{{ dark_foreground }}"
gutter_current = "{{ light_foreground }}"

# Surfaces: a scrollbar's track, a key's cap and the card behind it, a
# chosen row.
scrollbar_track = "{{ lighter_background }}"
raised_background = "{{ lighter_background }}"
selected_row_background = "{{ selection }}"
selection_background = "{{ selection }}"

status_foreground = "{{ foreground }}"
status_stale = "{{ bright_red }}"

# What a diff has used since diffs were printed.
change_added = "{{ green }}"
change_modified = "{{ yellow }}"
change_removed = "{{ red }}"

[syntax]
keyword = "{{ magenta }}"
function = "{{ blue }}"
type_name = "{{ yellow }}"
constructor = "{{ yellow }}"
string = "{{ green }}"
escape = "{{ cyan }}"
attribute = "{{ cyan }}"
constant = "{{ orange }}"
number = "{{ orange }}"
boolean = "{{ orange }}"
label = "{{ magenta }}"
property = "{{ blue }}"
variable = "{{ foreground }}"
operator = "{{ light_foreground }}"
punctuation = "{{ dark_foreground }}"
comment = "{{ dark_foreground }}"
error = "{{ bright_red }}"
warning = "{{ bright_yellow }}"

# The sixteen a program in Obelus's own terminal names by number, as
# omarchy's own terminals have them -- so a shell inside Obelus is the
# colour it is in kitty or alacritty beside it.
[terminal]
black = "{{ background }}"
red = "{{ red }}"
green = "{{ green }}"
yellow = "{{ yellow }}"
blue = "{{ blue }}"
magenta = "{{ magenta }}"
cyan = "{{ cyan }}"
white = "{{ foreground }}"
bright_black = "{{ muted }}"
bright_red = "{{ bright_red }}"
bright_green = "{{ bright_green }}"
bright_yellow = "{{ bright_yellow }}"
bright_blue = "{{ bright_blue }}"
bright_magenta = "{{ bright_magenta }}"
bright_cyan = "{{ bright_cyan }}"
bright_white = "{{ bright_foreground }}"

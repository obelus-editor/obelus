## What Obelus cannot use in a theme file, marked on the line. Each says
## what Obelus did instead, and where a second line follows it says how
## the line should be written.

could-not-read = Could not read { $path }: { $error }
wallpaper-not-a-table = A wallpaper is a table of its own
no-such-wallpaper-key = Nothing about a wallpaper is called this
no-file = This wallpaper names no file
file-in-quotes = A file is named in quotes
no-picture-at = There is no picture at { $path }
# The opacity used instead.
opacity-range = An opacity is a whole number from 0 to 100, so this is { $opacity }
# The words a fit or an anchor may be, in a list.
no-such-fit =
    No fit is called this, so it is cover
    One of { $choices }
no-such-anchor =
    No anchor is called this, so it is center
    One of { $choices }
no-such-base = No built-in theme is called this, so it builds on dark
not-a-colour =
    Not a colour, so it is the one this builds on
    A colour is written #rrggbb or #rgb
no-such-colour-here = No colour here is called this
colours-not-a-table = These colours are a table of their own
no-such-colour = No colour in a theme is called this
italic-not-a-list =
    Not a list of kinds, so it slants what this builds on
    A list is written ["comment"]
italic-names-no-colour =
    A word here names no colour, so it slants nothing
    A kind is named the way its colour is, like comment or type_name

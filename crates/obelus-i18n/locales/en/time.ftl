## How long ago something happened: a commit, a conversation, a project
## last opened. One unit, the largest that gives at least one.

# Lowercase, because it usually follows a name: "Ann, just now".
just-now = just now
minutes-ago = { $count ->
    [one] 1 minute ago
   *[other] { $count } minutes ago
}
hours-ago = { $count ->
    [one] 1 hour ago
   *[other] { $count } hours ago
}
days-ago = { $count ->
    [one] 1 day ago
   *[other] { $count } days ago
}
weeks-ago = { $count ->
    [one] 1 week ago
   *[other] { $count } weeks ago
}
months-ago = { $count ->
    [one] 1 month ago
   *[other] { $count } months ago
}
years-ago = { $count ->
    [one] 1 year ago
   *[other] { $count } years ago
}

## The headings a list ordered by day is split under.

today = Today
yesterday = Yesterday
this-week = This week
earlier = Earlier

## How long a piece of background work ran, as short as it can be said.

seconds-short = { $n }s
minutes-short = { $n }m
hours-short = { $n }h

## The status row at the foot of the screen.

# In place of a branch's name, where no branch is checked out.
detached = Detached
# Beside the palette's key, which is where a view's own keys are said.
more = More
# The one key a list on the status row answers to.
close = Close
# In front of the box on the page that asks which project: a path is being
# typed, or the projects above are being narrowed.
open = Open
filter = Filter
# What the path box says before anything is typed into it.
a-directory-or-a-file = A directory, or a file in one
narrow-the-list = Narrow the list
# After a path that is not there.
nothing-is-here = Nothing is here

## Facts about the file, after its path.

replacing = Replacing
read-only = Read-only
unsaved = Unsaved
# The file on disk changed under the buffer.
moved = Changed on disk
deleted = Deleted
# What is on screen was read from an older version.
stale = Stale

# The label in front of the project's directory.
project = Project
pull-request = Pull request #{ $number }
issue = Issue #{ $number }
# Where another window holds the conversation about this.
talked-about-in-another-window = Talked about in another window
# The same, by the name of the worktree the other window is on.
talked-about-in = Talked about in { $name }
notes = Notes
# How many notes are not ticked off yet.
left-to-come-back-to = { $count } to come back to

## A terminal's row.

# How far back up the screen the reader has scrolled.
rows-back = { $rows ->
    [one] { $rows } row back
   *[other] { $rows } rows back
}
ended = Ended
stopped-by = Stopped by { $signal }
exited = Exit code { $code }

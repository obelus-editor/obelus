## Git's history, the worktrees, and GitHub's pull requests and issues.

nothing-changed-here = Nothing changed here

## The history.

# Its tabs: the history of the file being read, the branches and tags, and
# the whole project's.
tab-this-file = This file
tab-refs = Refs
tab-the-project = Project
no-history-here = No history here
filter-commits = Filter commits
no-refs-here = No refs here
reading-the-history = Reading the history…
no-commit-touches-this-file = No commit touches this file
no-commits-here = No commits here
# How far the walk through the history has got.
commits-read = { $count ->
    [one] 1 commit read…
   *[other] { $count } commits read…
}
not-at-that-commit = Could not read the file at that commit

## The list of open documents, and the worktrees.

tab-documents = Documents
tab-worktrees = Worktrees
nothing-open = Nothing open
filter-open-documents = Filter open documents
no-worktrees = No worktrees
filter-worktrees = Filter worktrees
# A worktree git knows of whose directory is not there.
missing = Missing
# Beside a window on a worktree that is drawn at another machine, and so
# cannot be brought forward here.
on-machine = On { $machine }
on-another-machine = On another machine

## Pull requests and issues.

tab-pull-requests = Pull requests
tab-issues = Issues
# Beside a pull request's or an issue's page in the list of open documents.
pull-request = Pull request
issue = Issue
filter-by-title-or-number = Filter by title or number
no-open-pull-requests = No open pull requests
no-open-issues = No open issues
asking-github = Asking GitHub…
still-asking-github = Still asking GitHub
# What was typed, in quotes.
searching-github = Searching GitHub for “{ $query }”
asking-for-more = Asking GitHub for { $count } more
asking-what-changed = Asking GitHub what has changed
# Why the list may not be what GitHub has now, after the dot.
may-be-out-of-date = May be out of date · { $why }
open-count = { $count } open
open-of-total = { $listed } of { $total } open
matches = { $count ->
    [one] 1 match
   *[other] { $count } matches
}
matches-so-far = { $count ->
    [one] 1 match so far
   *[other] { $count } matches so far
}
# GitHub's search stops at a thousand, however many it counts.
matches-and-no-more = { $count ->
    [one] 1 match · GitHub gives no more
   *[other] { $count } matches · GitHub gives no more
}
draft = Draft
# What a conversation is called before the agent has named it.
review = Review #{ $number }
review-named = Review #{ $number }: { $title }
issue-named = Issue #{ $number }: { $title }
no-commit-from-github = GitHub did not say which commit this is at
# A file of a pull request that is not text.
not-text = { $path } is not text

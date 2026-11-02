## A pull request or an issue, read as a page.

# The key that starts a conversation about it.
talk = Ask the agent
talk-about-pull-request = Ask the agent to review this pull request
talk-about-issue = Ask the agent to answer this issue
files = { $count ->
    [one] { $count } file
   *[other] { $count } files
}
asking-about-pull-request = Asking GitHub for its description, checks and comments
asking-about-issue = Asking GitHub for its description and comments
no-description = No description
files-heading = Files
checks-heading = Checks
# Where there are no files, or no checks, after the heading.
none = None
# Under the files, where GitHub named only some of them.
more-on-github = And { $count } more on GitHub
checks-failed = { $count } failed
checks-running = { $count } running
checks-passed = { $count } passed
checks-skipped = { $count } skipped
no-comments = No comments
# Between a comment's author and the file it is on.
on = on
replies = { $count ->
    [one] { $count } reply
   *[other] { $count } replies
}
resolved = Resolved
outdated = Outdated

## Why there is nothing from GitHub.

no-gh = Listing pull requests needs gh, which is not installed
signed-out = Not signed in to GitHub. Run gh auth login to sign in
github-failed = GitHub would not answer: { $why }
# Why GitHub would not answer, after the colon above.
answer-unreadable = its answer could not be read
answer-unreadable-because = its answer could not be read: { $error }
no-reason = it gave no reason

## What a comment's author did, after their name.

commented = commented
approved = approved
requested-changes = requested changes
dismissed = was dismissed

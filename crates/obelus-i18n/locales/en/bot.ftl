## The chat notes are worked on from: Slack or Feishu. A platform's name is
## a name, and not here. What goes to the platform itself is not here
## either: these are what Obelus says on its own screen.

not-connected = Not connected to { $platform }
# The reason, after the colon: one of the fragments below, or the
# platform's own word.
not-connected-because = Not connected to { $platform }: { $why }
token-not-kept = Could not save the token: { $why }
# A token pasted into the row for another: each starts its own way.
starts-with = { $field } must start with { $prefix }
nowhere-to-hold = Could not hold the { $platform } connection: there is nowhere to keep its lock
could-not-take-over = Could not take the { $platform } connection over from another window
not-let-go = Another window would not let go of { $platform }

## Said in a conversation whose thread on the platform missed something.

thread-not-started-in-time = Could not reach { $platform } in time to start a thread for this conversation
thread-refused = { $platform } would not start a thread for this conversation
# The platform's reason, after the colon.
message-refused = { $platform } would not take a message from this conversation: { $why }
message-not-sent-in-time = Could not reach { $platform } in time to send a message from this conversation

## Why it is not connected. Lowercase, because each follows a colon.

# The fields not filled in, in a list.
not-set = { $fields } not set
no-answer-in-time = no answer in time
# The status of the answer, as the web writes it: "401 Unauthorized".
answered = answered { $status }
no-address = no address to connect to
nowhere-to-keep-tokens = there is nowhere on this machine to keep a token
not-toml = { $path } is not valid TOML
not-toml-at = { $path } is not valid TOML at line { $line }

## What each platform has to be told, on the chat settings page: what the
## row calls it, and after `-about` where to find it. The pages named are
## the platform's, in its own words.

slack-app-token = App token
slack-app-token-about = Socket Mode's token, from Basic Information
slack-bot-token = Bot token
slack-bot-token-about = From OAuth & Permissions, once the app is installed
slack-manifest = Manifest
slack-manifest-about = One app per machine: paste it into Create New App → From a manifest
feishu-app-id = App ID
feishu-app-id-about = From Credentials & Basic Info, on the app's page
feishu-app-secret = App Secret
feishu-app-secret-about = From the same page, beside the App ID
feishu-domain = Domain
feishu-domain-about = feishu for Feishu, lark for Lark outside China
feishu-console = Developer console
feishu-console-about = One app per machine: a custom app with the bot on, long connection for events and for the card callback

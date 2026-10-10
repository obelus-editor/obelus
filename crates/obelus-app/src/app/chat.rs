//! Talking to an agent, the conversations that outlive the window, and
//! the chat they can be reached from.

pub mod agents;
pub(super) mod conversations;
pub(super) mod headless;
pub(super) mod mirroring;
pub(super) mod opening;
pub(super) mod relaying;
pub(super) mod remote;
pub mod talking;

/// The agent this window talks to: the connection, the tools Obelus offers it
/// and where, the commands it is running, and the conversations waiting on it.
#[derive(Debug, Default)]
pub(in crate::app) struct Agent {
    /// The agent Obelus is talking to, once something has needed it.
    pub(in crate::app) talker: Option<obelus_agent::acp::Talk>,
    /// Where an agent reaches what Obelus offers it, if it could listen.
    ///
    /// Taken once per project and kept: every conversation is told an
    /// address under this one, so a second agent started later reaches the
    /// same tools rather than a second server nobody asked for. Taken again
    /// only when the project is, because the tools are about one tree.
    pub(in crate::app) tools_url: Option<String>,
    /// The server at that address, which stops listening when this goes.
    pub(in crate::app) listening: Option<obelus_mcp::Listening>,
    /// The commands an agent asked to run, while they run.
    ///
    /// On the loop rather than on the connection's thread, because a
    /// command is a thing on the page: the row that says what is happening
    /// reads its output, and a key stops it.
    pub(in crate::app) runs: obelus_agent::running::Runs,
    /// Who is waiting to be told a command has ended.
    ///
    /// The agent's `terminal/wait_for_exit`, held until the command does.
    /// Answered from the frame check rather than by blocking: the loop
    /// that draws must not wait on a compile.
    pub(in crate::app) waiting_on: Vec<(
        String,
        obelus_agent::acp::Answer<Option<obelus_agent::running::Ended>>,
    )>,
}

//! What the renderer may ask the application.

use super::*;

/// What the renderer may ask the application.
///
/// Every one of these forwards to the method of the same name: the trait is
/// the list of questions, and the answers stay where they are written. The
/// two cannot drift -- a signature that stopped matching is a compile error
/// here -- and the alternative, moving forty-four methods out of the
/// application's own impl blocks, would have made every one of its several
/// hundred internal calls go through a trait that has to be in scope.
impl Screen for App {
    fn agent_name(&self) -> Option<&str> {
        App::agent_name(self)
    }
    fn agent_settings(&self) -> &[acp::Setting] {
        App::agent_settings(self)
    }
    fn agent_offering(&self) -> Option<obelus_component::settings::Offering> {
        App::agent_offering(self)
    }
    fn called(&self, key: &str, word: &str) -> Option<std::borrow::Cow<'static, str>> {
        App::called(self, key, word)
    }
    fn agent_usage(&self) -> Option<&acp::Usage> {
        App::agent_usage(self)
    }
    fn background_tasks(&self) -> Option<(usize, usize)> {
        App::background_tasks(self)
    }
    fn blame(&self) -> Option<&[Option<obelus_git::Blamed>]> {
        App::blame(self)
    }
    fn replacing(&self) -> bool {
        App::replacing(self)
    }
    fn names(&self) -> Option<&obelus_component::names::Names> {
        App::names(self)
    }
    fn card(&self) -> Option<&Card> {
        App::card(self)
    }
    fn changes(&self) -> Option<&obelus_git::Changes> {
        App::changes(self)
    }
    fn chat(&self) -> Option<&Chat> {
        App::chat(self)
    }
    fn completion(&self) -> Option<&Completion> {
        App::completion(self)
    }
    fn config(&self) -> &obelus_config::Config {
        App::config(self)
    }
    fn counts(&self) -> Option<&Counts> {
        App::counts(self)
    }
    fn current_buffer(&self) -> Option<&Buffer> {
        App::current_buffer(self)
    }
    fn drawn(&self) -> &[obelus_ui::Drawn] {
        App::drawn(self)
    }
    fn highlights(&self) -> &Highlights {
        App::highlights(self)
    }
    fn hover(&self) -> Option<&Hover> {
        App::hover(self)
    }
    fn images(&self) -> &Images {
        App::images(self)
    }
    fn keymap(&self) -> &Keymap {
        App::keymap(self)
    }
    fn layers(&self) -> layers::Layers {
        App::layers(self)
    }
    fn listed_agents(&self) -> Vec<Listed> {
        App::listed_agents(self)
    }
    fn marked_runs(&self) -> &[obelus_text::coordinates::Span] {
        App::marked_runs(self)
    }
    fn note(&self) -> Option<&str> {
        App::note(self)
    }
    fn note_is_wrong(&self) -> bool {
        App::note_is_wrong(self)
    }
    fn talked_about(&self) -> Vec<obelus_component::todo::Talked> {
        App::talked_about(self)
    }
    fn notes(&self) -> Option<&TodoView> {
        App::notes(self)
    }
    fn terminal(&self) -> Option<&obelus_terminal::Terminal> {
        App::terminal(self)
    }
    fn opened_hunks(&self) -> Vec<LineNumber> {
        App::opened_hunks(self)
    }
    fn choosing(&self) -> Option<obelus_ui::Choosing> {
        self.what_is_being_chosen()
    }
    fn naming_list(&self) -> Option<&Picker> {
        self.naming_list.as_ref()
    }

    fn version(&self) -> &str {
        App::version(self)
    }
    fn newer_release(&self) -> Option<&str> {
        App::newer_release(self)
    }

    fn phase(&self) -> u32 {
        App::phase(self)
    }
    fn pointer(&self) -> Option<(u16, u16)> {
        self.pointer
    }
    fn picker(&self) -> Option<&Picker> {
        App::picker(self)
    }
    fn pinned(&self) -> &[&'static str] {
        App::pinned(self)
    }
    fn preview(&self) -> Option<Previewed<'_>> {
        App::preview(self)
    }
    fn complaint(&self) -> Option<obelus_ui::Complained<'_>> {
        let complaint = self.complaining.as_ref()?;
        Some(obelus_ui::Complained {
            line: complaint.line,
            column: complaint.column,
            said: &complaint.said,
            severity: complaint.severity,
            others: complaint.others,
        })
    }
    fn prompt(&self) -> Option<&Prompt> {
        App::prompt(self)
    }
    fn making_in(&self) -> Option<String> {
        App::making_in(self)
    }
    fn readers_named(&self) -> &[&'static str] {
        App::readers_named(self)
    }
    fn reading_nothing(&self) -> bool {
        App::reading_nothing(self)
    }
    fn registry_failure(&self) -> Option<&str> {
        App::registry_failure(self)
    }
    fn rendered_rows(&self) -> Option<usize> {
        App::rendered_rows(self)
    }
    fn rendering(&self) -> Option<&[obelus_row::Row]> {
        App::rendering(self)
    }
    fn server_state(&self) -> Option<(&'static str, obelus_lsp::ServerState)> {
        App::server_state(self)
    }

    fn remote(&self) -> Option<(&'static str, obelus_remote::State)> {
        App::remote_badge(self)
    }

    fn server_busy(&self) -> bool {
        App::server_busy(self)
    }
    fn settings(&self) -> Option<&Settings> {
        App::settings(self)
    }
    fn signature(&self) -> Option<&obelus_component::signature::Signature> {
        App::signature(self)
    }
    fn slash(&self) -> Option<&Picker> {
        App::slash(self)
    }
    fn talking(&self) -> Talking {
        App::talking(self)
    }
    fn travelled(&self) -> i64 {
        App::travelled(self)
    }

    fn text_area(&self) -> TextArea {
        App::text_area(self)
    }
    fn theme(&self) -> &Theme {
        App::theme(self)
    }
    fn project_config(&self) -> Option<&Path> {
        App::project_config(self)
    }
    fn troubles(&self) -> &[obelus_lsp::trouble::Trouble] {
        App::troubles(self)
    }
    fn is_about_a_note(&self) -> bool {
        App::is_about_a_note(self)
    }
    fn branch_this_conversation_works_on(&self) -> Option<&obelus_git::Head> {
        App::branch_this_conversation_works_on(self)
    }
    fn what_this_conversation_is_called(&self) -> Option<String> {
        App::what_this_conversation_is_called(self)
    }
    fn head(&self) -> Option<&obelus_git::Head> {
        App::head(self)
    }
    fn tree_has_gone(&self) -> bool {
        App::tree_has_gone(self)
    }
    fn working_directory(&self) -> &Path {
        App::working_directory(self)
    }
}

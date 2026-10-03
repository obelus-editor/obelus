//! Where a background worker's events go.
//!
//! A worker used to take `Sender<Event>`, which meant every worker knew the
//! one type the whole application reacts to: the reading of a repository
//! had heard of a search hit, because both were variants of the same enum.
//! Here a worker names only what *it* produces and says "somewhere this
//! goes", and the thing on the other end is whatever happens to know how to
//! hold all of them at once.
//!
//! The application is that thing, and it is the only one that could be: it
//! is the only part of Obelus that has heard of every worker. Which is why
//! the joining is an `impl From<obelus_git::Event> for Event` written there,
//! and not a trait any of the workers implement.

/// The far end is gone.
///
/// No payload. Every send site either writes `let _ =` or asks `.is_err()`,
/// and handing the event back would not be possible anyway: by the time the
/// channel refuses it, it has already been turned into whatever that channel
/// carries -- a type this side has never heard of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gone;

impl std::fmt::Display for Gone {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("nothing is listening for events any more")
    }
}

impl std::error::Error for Gone {}

/// Somewhere events of type `E` can be put.
///
/// `Send + Sync + 'static` because that is exactly what a producer does with
/// one: clone it into a thread or a task, which wants `Send`, and hold a `&`
/// to it across an `.await` inside one, which is what makes the future
/// `Send` and wants `Sync`. Three of Obelus's producers do the second --
/// installing an agent, downloading one, opening a session -- so leaving
/// `Sync` out would surface as "future cannot be sent between threads" in
/// three files that never mention this trait.
///
/// No `Clone` supertrait, deliberately: `Clone` is not object safe, and
/// `Arc<dyn Sink<E>>` is what a producer reaches for when it has to *keep* a
/// sink in a struct somebody else's macro writes the impls for. A producer
/// that clones asks for it by name, and both a channel and that `Arc`
/// satisfy it.
///
/// Nothing here asks for `E: Clone`, and nothing may start to. An agent's
/// question carries the one channel its answer goes back through, so a sink
/// that fanned out to two places could not exist for those events at all.
/// That is not a limitation to work around; it is the invariant, stated
/// where it can be enforced.
pub trait Sink<E>: Send + Sync + 'static {
    /// Puts one in, or says there is nobody there.
    ///
    /// # Errors
    ///
    /// [`Gone`] once the far end has been dropped, which is the main loop
    /// having ended. Every producer reads that as "stop".
    fn send(&self, event: E) -> Result<(), Gone>;
}

/// Any channel carrying something this event turns into.
///
/// The whole of the arrangement. A repository sends a `obelus_git::Event`, the
/// loop's channel carries the application's `Event`, and the `From` between
/// them is written once, in the only crate that has heard of both.
///
/// Written `T: From<E>` rather than `E: Into<T>`: the same bound either way,
/// and this is the one the orphan rules let the application write.
impl<E, T> Sink<E> for std::sync::mpsc::Sender<T>
where
    E: Send + 'static,
    T: From<E> + Send + 'static,
{
    fn send(&self, event: E) -> Result<(), Gone> {
        // By path, and spelt out. `self.send(..)` would in fact reach the
        // inherent method -- an inherent impl is probed before a trait --
        // but a line that reads as unbounded recursion and is not is a line
        // somebody comes back to "fix".
        std::sync::mpsc::Sender::<T>::send(self, T::from(event)).map_err(|_| Gone)
    }
}

/// A channel with a bound, for a test that has to keep a worker in step with
/// it.
///
/// With no room at all a send waits for the receiver to take the one before,
/// so the worker cannot get further ahead of the test than the one event it
/// is holding. Asking "did it stop" of a worker on a free-running channel is
/// a race against the scheduler: a test that is paused for a few dozen
/// milliseconds after starting the worker finds it already finished.
impl<E, T> Sink<E> for std::sync::mpsc::SyncSender<T>
where
    E: Send + 'static,
    T: From<E> + Send + 'static,
{
    fn send(&self, event: E) -> Result<(), Gone> {
        std::sync::mpsc::SyncSender::<T>::send(self, T::from(event)).map_err(|_| Gone)
    }
}

/// A sink behind a pointer, which is how one is shared and cloned without
/// `Clone` being in the trait.
///
/// `?Sized`, so `Arc<dyn Sink<E>>` is itself a `Sink<E>` -- and an `Arc` is
/// `Clone` whatever it points at, which is what a producer that has to keep
/// a sink in a field wants.
impl<E, S> Sink<E> for std::sync::Arc<S>
where
    E: Send + 'static,
    S: Sink<E> + ?Sized,
{
    fn send(&self, event: E) -> Result<(), Gone> {
        (**self).send(event)
    }
}

/// Everything sent, kept, for a test that wants to look at it.
///
/// A list behind a lock rather than a channel because the question a test
/// asks is "what arrived", not "what arrives next", and a list answers that
/// without a timeout in it.
impl<E> Sink<E> for std::sync::Mutex<Vec<E>>
where
    E: Send + 'static,
{
    fn send(&self, event: E) -> Result<(), Gone> {
        // A poisoned lock is a test thread that panicked. Reporting it as
        // gone stops the producer, which is what a test that has already
        // failed wants to happen.
        self.lock().map_err(|_| Gone)?.push(event);
        Ok(())
    }
}

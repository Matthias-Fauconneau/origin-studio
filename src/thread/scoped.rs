use super::{Thread, current, spawn_unchecked, JoinHandle, park};
use core::marker::PhantomData;
use crate::sync::atomic::{AtomicUsize, Ordering};

/// A scope to spawn scoped threads in.
///
/// See [`scope`] for details.
pub struct Scope<'scope, 'env: 'scope> {
	pub(crate) main_thread: Thread,
	pub(crate) num_running_threads: AtomicUsize,
	/// Invariance over 'scope, to make sure 'scope cannot shrink, which is necessary for soundness.
    ///
    /// Without invariance, this would compile fine but be unsound:
    ///
    /// ```compile_fail,E0373
    /// std::thread::scope(|s| {
    ///     s.spawn(|| {
    ///         let a = String::from("abcd");
    ///         s.spawn(|| println!("{a:?}")); // might run after `a` is dropped
    ///     });
    /// });
    /// ```
    scope: PhantomData<&'scope mut &'scope ()>,
    env: PhantomData<&'env mut &'env ()>,
}

/// Creates a scope for spawning scoped threads.
///
/// The function passed to `scope` will be provided a [`Scope`] object,  through which scoped threads can be [spawned][`Scope::spawn`].
///
/// Unlike non-scoped threads, scoped threads can borrow non-`'static` data, as the scope guarantees all threads will be joined at the end of the scope.
///
/// All threads spawned within the scope that haven't been manually joined will be automatically joined before this function returns.
#[track_caller]
pub fn scope<'env, F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T, T>(f: F) -> T {
    let scope = Scope {
    	main_thread: current(),
        num_running_threads: AtomicUsize::new(0),
        env: PhantomData,
        scope: PhantomData,
    };

    let result = f(&scope);

    // Wait until all the threads are finished.
    while scope.num_running_threads.load(Ordering::Acquire) != 0 {
        park();
    }

    result
}

impl<'scope, 'env> Scope<'scope, 'env> {
    /// Spawns a new thread within a scope, returning a [`JoinHandle`] for it.
    ///
    /// Unlike non-scoped threads, threads spawned with this function may borrow non-`'static` data from the outside the scope. See [`scope`] for details.
    ///
    /// The join handle provides a [`join`] method that can be used to join the spawned thread. If the spawned thread panics, [`join`] will return an [`Err`] containing the panic payload.
    ///
    /// If the join handle is dropped, the spawned thread will implicitly joined at the end of the scope. In that case, if the spawned thread panics, [`scope`] will panic after all threads are joined.
    pub fn spawn<F: FnOnce() + Send + 'scope>(&'scope self, f: F) -> JoinHandle<'scope> {
    	self.num_running_threads.fetch_add(1, Ordering::Relaxed);
        spawn_unchecked(f, Some(&self))
    }
}

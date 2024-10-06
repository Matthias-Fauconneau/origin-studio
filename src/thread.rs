//! Native threads.

use crate::boxed::Box;
use crate::io;
use core::mem::forget;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::marker::PhantomData;

// Rust does't need the OS tids, it just needs unique ids, so we just use the
// raw `Thread` value casted to `usize`.
pub struct ThreadId(#[allow(dead_code)] usize);

pub struct Thread(origin::thread::Thread);

impl Thread {
    pub fn id(&self) -> ThreadId {
        ThreadId(self.0.to_raw().addr())
    }
}

pub struct JoinHandle<'scope> {
	pub thread: Thread,
	_marker: PhantomData<&'scope ()>,
}

impl JoinHandle<'_> {
    pub fn join(self) -> io::Result<()> {
        unsafe {
            origin::thread::join(self.thread .0);
        }

        // Don't call drop, which would detach the thread we just joined.
        forget(self);

        Ok(())
    }
}

impl Drop for JoinHandle<'_> {
    fn drop(&mut self) {
        unsafe {
            origin::thread::detach(self.thread .0);
        }
    }
}

pub fn spawn<F: FnOnce() + Send + 'static>(f: F) -> JoinHandle<'static> { spawn_unchecked(f, None) }
pub fn spawn_unchecked<'scope, F: FnOnce() + Send>(f: F, scope: Option<&Scope>) -> JoinHandle<'scope> {
    // Pack up the closure.
    let boxed = Box::new(move || {
        #[cfg(feature = "stack-overflow")] let _ = unsafe { crate::stack_overflow::Handler::new() };
        f()
    });
    
    let raw = Box::into_raw(boxed) as *mut (dyn FnOnce() + Send + 'static);
    let (callee, metadata) = raw.to_raw_parts();
    let args = [
        NonNull::new(callee as _),
        NonNull::new(unsafe { core::mem::transmute(metadata) }),
        NonNull::new(Box::into_raw(Box::new(scope)).cast())
    ];

    let thread = unsafe {
        let r = origin::thread::create(
            move |args| {
                // Unpack and call.

                let (callee, metadata) = (args[0], args[1]);
                let raw: *mut (dyn FnOnce() + Send + 'static) =
                    core::ptr::from_raw_parts_mut(core::mem::transmute::<_,*mut ()>(callee), core::mem::transmute(metadata));
                let boxed = Box::from_raw(raw);
                boxed();

                let scope: Box<Option<&Scope>> = Box::from_raw(args[2].unwrap().as_ptr().cast());

                if let Some(scope) = *scope {
	                if scope.num_running_threads.fetch_sub(1, core::sync::atomic::Ordering::Release) == 1 {
	                    scope.main_thread.unpark();
	                }
                }

                None
            },
            &args,
            origin::thread::default_stack_size(),
            origin::thread::default_guard_size(),
        );
        r.unwrap()
    };

    JoinHandle{thread: Thread(thread), _marker: PhantomData}
}

pub fn current() -> Thread {
    Thread(origin::thread::current())
}

pub(crate) struct GetThreadId;

unsafe impl rustix_futex_sync::lock_api::GetThreadId for GetThreadId {
    const INIT: Self = Self;

    fn nonzero_thread_id(&self) -> NonZeroUsize {
        origin::thread::current().to_raw_non_null().addr()
    }
}

pub(crate) type ReentrantMutex<T> = rustix_futex_sync::ReentrantMutex<GetThreadId, T>;
pub(crate) type ReentrantMutexGuard<'a, T> =
    rustix_futex_sync::ReentrantMutexGuard<'a, GetThreadId, T>;

pub fn park() { rustix::process::sched_yield() }
pub fn park_timeout(_time: u64) { park() }

impl Thread {
	pub fn unpark(&self) {}
}

mod scoped;

pub use scoped::{Scope, scope};

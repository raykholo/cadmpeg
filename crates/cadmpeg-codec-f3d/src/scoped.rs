//! Scoped threads that still work on `wasm32-unknown-unknown`.
//!
//! That target has no threads: `std::thread::scope`'s `spawn` panics there (a trap, in a
//! WebAssembly host), so the decoder would stop on every design with design history. Natively
//! this is `std::thread::scope`. On that target each spawned closure runs to completion when it
//! is spawned and `join` returns its result, so the same code gives the same answer on one
//! thread.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use std::thread::scope;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use sequential::scope;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod sequential {
    use std::marker::PhantomData;

    pub(crate) struct Scope<'scope, 'env: 'scope> {
        scope: PhantomData<&'scope mut &'scope ()>,
        env: PhantomData<&'env mut &'env ()>,
    }

    pub(crate) struct ScopedJoinHandle<'scope, T> {
        result: T,
        scope: PhantomData<&'scope ()>,
    }

    impl<'scope, 'env> Scope<'scope, 'env> {
        pub(crate) fn spawn<F, T>(&'scope self, work: F) -> ScopedJoinHandle<'scope, T>
        where
            F: FnOnce() -> T + Send + 'scope,
            T: Send + 'scope,
        {
            ScopedJoinHandle {
                result: work(),
                scope: PhantomData,
            }
        }
    }

    impl<T> ScopedJoinHandle<'_, T> {
        pub(crate) fn join(self) -> std::thread::Result<T> {
            Ok(self.result)
        }
    }

    pub(crate) fn scope<'env, F, T>(body: F) -> T
    where
        F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T,
    {
        body(&Scope {
            scope: PhantomData,
            env: PhantomData,
        })
    }
}

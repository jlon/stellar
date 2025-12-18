/// Defer macro for RAII-style cleanup
///
/// Similar to Go's defer, this macro ensures code is executed when the scope exits.
/// The deferred code runs even if the function returns early or panics.
///
/// # Example
///
/// ```rust
/// use stellar::defer;
///
/// fn process_file() -> Result<(), std::io::Error> {
///     let file = std::fs::File::open("test.txt")?;
///     defer! {
///         tracing::info!("File closed");
///         drop(file);
///     }
///     
///     // Do work with file...
///     // File will be closed automatically when function returns
///     Ok(())
/// }
/// ```
#[macro_export]
macro_rules! defer {
    ($($body:tt)*) => {
        let _guard = {
            struct Guard<F: FnOnce()>(Option<F>);

            impl<F: FnOnce()> Drop for Guard<F> {
                fn drop(&mut self) {
                    if let Some(f) = self.0.take() {
                        f();
                    }
                }
            }

            Guard(Some(|| {
                $($body)*
            }))
        };
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_defer_executes_on_early_return() {
        let executed = Arc::new(AtomicBool::new(false));
        let executed_clone = Arc::clone(&executed);

        fn early_return(flag: Arc<AtomicBool>) -> i32 {
            defer! {
                flag.store(true, Ordering::SeqCst);
            }
            42
        }

        let result = early_return(executed_clone);
        assert_eq!(result, 42);
        assert!(executed.load(Ordering::SeqCst));
    }
}

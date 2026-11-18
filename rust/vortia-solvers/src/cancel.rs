//! Stopping or pausing a run from outside.
//!
//! A solver checks the flags at the top of each step, so a stop or a pause takes effect between
//! steps and never leaves a step half done. A paused run waits there, holding its state in
//! memory, until the pause is lifted or a stop is asked for. The flag belongs to the thread that runs the solver, so two
//! runs on two threads stop independently and a solver needs no extra argument.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Result, SolverError};

thread_local! {
    static FLAG: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
    static PAUSE: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// How often a paused run looks again at its flags.
const PAUSED_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// Clears the flag of the thread when the run ends, even by a panic.
struct Armed;

impl Drop for Armed {
    fn drop(&mut self) {
        FLAG.with(|f| *f.borrow_mut() = None);
        PAUSE.with(|f| *f.borrow_mut() = None);
    }
}

/// Runs `run` with `flag` as this thread's stop flag: once it is set, the solver stops at the
/// start of its next step with [`SolverError::Cancelled`].
pub fn with_flag<T>(flag: Arc<AtomicBool>, run: impl FnOnce() -> Result<T>) -> Result<T> {
    FLAG.with(|f| *f.borrow_mut() = Some(flag));
    let _armed = Armed;
    run()
}

/// Like [`with_flag`], and the run also waits at the start of a step while `pause` is set.
pub fn with_control<T>(
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    run: impl FnOnce() -> Result<T>,
) -> Result<T> {
    PAUSE.with(|f| *f.borrow_mut() = Some(pause));
    with_flag(stop, run)
}

fn is_set(flag: &'static std::thread::LocalKey<RefCell<Option<Arc<AtomicBool>>>>) -> bool {
    flag.with(|f| {
        f.try_borrow()
            .is_ok_and(|f| f.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)))
    })
}

/// Called at the top of every step of a solver.
pub(crate) fn check() -> Result<()> {
    while is_set(&PAUSE) && !is_set(&FLAG) {
        std::thread::sleep(PAUSED_POLL);
    }
    if is_set(&FLAG) {
        Err(SolverError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_stops_a_thread_without_a_flag() {
        assert!(check().is_ok());
    }

    #[test]
    fn a_set_flag_stops_the_run_and_is_forgotten_after_it() {
        let flag = Arc::new(AtomicBool::new(false));
        let result = with_flag(flag.clone(), || {
            check()?;
            flag.store(true, Ordering::Relaxed);
            check()
        });
        assert!(matches!(result, Err(SolverError::Cancelled)));
        assert!(check().is_ok(), "the flag outlived its run");
    }

    #[test]
    fn a_pause_holds_the_run_until_it_is_lifted_and_a_stop_ends_it_while_paused() {
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(true));
        let (s, p) = (stop.clone(), pause.clone());
        let steps = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = steps.clone();
        let run = std::thread::spawn(move || {
            with_control(s, p, || {
                for _ in 0..3 {
                    check()?;
                    counted.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            })
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(steps.load(Ordering::Relaxed), 0, "a paused run took a step");
        pause.store(false, Ordering::Relaxed);
        assert!(run.join().unwrap().is_ok());
        assert_eq!(steps.load(Ordering::Relaxed), 3);

        let (stop, pause) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(true)),
        );
        let (s, p) = (stop.clone(), pause.clone());
        let run = std::thread::spawn(move || with_control(s, p, check));
        std::thread::sleep(std::time::Duration::from_millis(60));
        stop.store(true, Ordering::Relaxed);
        assert!(matches!(run.join().unwrap(), Err(SolverError::Cancelled)));
    }

    #[test]
    fn a_flag_stops_only_its_own_thread() {
        let flag = Arc::new(AtomicBool::new(true));
        let other = std::thread::spawn(|| check().is_ok());
        assert!(matches!(
            with_flag(flag, check),
            Err(SolverError::Cancelled)
        ));
        assert!(other.join().unwrap());
    }
}

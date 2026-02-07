use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    DEBUG.store(true, Ordering::Relaxed);
}

pub fn is_on() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

macro_rules! dbg_log {
    ($($arg:tt)*) => {
        if $crate::debug::is_on() {
            eprintln!("DEBUG {}", format!($($arg)*));
        }
    };
}
pub(crate) use dbg_log;

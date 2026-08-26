use std::sync::atomic::{AtomicU8, Ordering};

static VERBOSITY: AtomicU8 = AtomicU8::new(0);

pub fn set(level: u8) {
    VERBOSITY.store(level, Ordering::Relaxed);
}

pub fn operation(message: impl AsRef<str>) {
    if VERBOSITY.load(Ordering::Relaxed) > 0 {
        eprintln!("  -> {}", message.as_ref());
    }
}

pub fn detail(message: impl AsRef<str>) {
    if VERBOSITY.load(Ordering::Relaxed) > 1 {
        eprintln!("     {}", message.as_ref());
    }
}

pub fn section(message: impl AsRef<str>) {
    eprintln!("\n{}", message.as_ref());
}

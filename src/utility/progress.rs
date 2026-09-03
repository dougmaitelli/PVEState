use console::{style, user_attended_stderr};
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    env,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

static VERBOSITY: AtomicU8 = AtomicU8::new(0);
static ACTIVE: LazyLock<Mutex<Option<Stage>>> = LazyLock::new(|| Mutex::new(None));

struct Stage {
    bar: ProgressBar,
    message: String,
    started: Instant,
    operations: u64,
}

pub fn set(level: u8) {
    VERBOSITY.store(level, Ordering::Relaxed);
}

pub fn section(message: impl Into<String>) {
    let message = message.into();
    let mut active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
    finish_stage(active.take(), true);

    if interactive() {
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("static progress template")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_message(message.clone());
        bar.enable_steady_tick(Duration::from_millis(80));
        *active = Some(Stage {
            bar,
            message,
            started: Instant::now(),
            operations: 0,
        });
    } else {
        eprintln!("{message}");
    }
}

pub fn operation(message: impl AsRef<str>) {
    if VERBOSITY.load(Ordering::Relaxed) == 0 {
        count_operation();
        return;
    }

    let message = format!("  {} {}", style("→").cyan(), message.as_ref());
    let mut active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(stage) = active.as_mut() {
        stage.operations += 1;
        update_message(stage);
        stage.bar.println(message);
    } else {
        eprintln!("{message}");
    }
}

pub fn detail(message: impl AsRef<str>) {
    if VERBOSITY.load(Ordering::Relaxed) < 2 {
        return;
    }

    let message = format!("    {}", style(message.as_ref()).dim());
    let active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(stage) = active.as_ref() {
        stage.bar.println(message);
    } else {
        eprintln!("{message}");
    }
}

pub fn finish(success: bool) {
    let stage = ACTIVE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    finish_stage(stage, success);
}

fn count_operation() {
    let mut active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(stage) = active.as_mut() {
        stage.operations += 1;
        update_message(stage);
    }
}

fn update_message(stage: &Stage) {
    stage.bar.set_message(if stage.operations == 1 {
        format!("{} · 1 operation", stage.message)
    } else {
        format!("{} · {} operations", stage.message, stage.operations)
    });
}

fn finish_stage(stage: Option<Stage>, success: bool) {
    let Some(stage) = stage else { return };
    let marker = if success {
        style("✓").green().bold()
    } else {
        style("✗").red().bold()
    };
    let operations = match stage.operations {
        0 => String::new(),
        1 => " · 1 operation".into(),
        count => format!(" · {count} operations"),
    };
    stage.bar.finish_and_clear();
    eprintln!(
        "{marker} {}{operations} · {}",
        stage.message,
        HumanDuration(stage.started.elapsed())
    );
}

fn interactive() -> bool {
    user_attended_stderr()
        && env::var_os("CI").is_none()
        && env::var_os("TERM").is_none_or(|term| term != "dumb")
}

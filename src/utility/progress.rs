use console::{style, user_attended_stderr};
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    cell::RefCell,
    env,
    time::{Duration, Instant},
};

pub(crate) trait EventSink {
    fn section(&self, message: &str);
    fn operation(&self, message: &str);
    fn detail(&self, message: &str);
    fn finish(&self, success: bool);
}

pub(crate) struct TerminalEventSink {
    verbosity: u8,
    active: RefCell<Option<Stage>>,
}

struct Stage {
    bar: ProgressBar,
    message: String,
    started: Instant,
    operations: u64,
}

impl TerminalEventSink {
    pub(crate) fn new(verbosity: u8) -> Self {
        Self {
            verbosity,
            active: RefCell::new(None),
        }
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
}

impl EventSink for TerminalEventSink {
    fn section(&self, message: &str) {
        Self::finish_stage(self.active.borrow_mut().take(), true);
        if interactive() {
            let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
            bar.set_style(
                ProgressStyle::with_template("{spinner:.cyan} {msg}")
                    .expect("static progress template")
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );
            bar.set_message(message.to_owned());
            bar.enable_steady_tick(Duration::from_millis(80));
            *self.active.borrow_mut() = Some(Stage {
                bar,
                message: message.to_owned(),
                started: Instant::now(),
                operations: 0,
            });
        } else {
            eprintln!("{message}");
        }
    }

    fn operation(&self, message: &str) {
        let mut active = self.active.borrow_mut();
        if let Some(stage) = active.as_mut() {
            stage.operations += 1;
            let suffix = if stage.operations == 1 { "" } else { "s" };
            stage.bar.set_message(format!(
                "{} · {} operation{suffix}",
                stage.message, stage.operations
            ));
            if self.verbosity > 0 {
                stage
                    .bar
                    .println(format!("  {} {message}", style("→").cyan()));
            }
        } else if self.verbosity > 0 {
            eprintln!("  {} {message}", style("→").cyan());
        }
    }

    fn detail(&self, message: &str) {
        if self.verbosity < 2 {
            return;
        }
        let message = format!("    {}", style(message).dim());
        if let Some(stage) = self.active.borrow().as_ref() {
            stage.bar.println(message);
        } else {
            eprintln!("{message}");
        }
    }

    fn finish(&self, success: bool) {
        Self::finish_stage(self.active.borrow_mut().take(), success);
    }
}

impl Drop for TerminalEventSink {
    fn drop(&mut self) {
        Self::finish_stage(self.active.get_mut().take(), false);
    }
}

#[cfg(test)]
pub(crate) struct NullEventSink;

#[cfg(test)]
impl EventSink for NullEventSink {
    fn section(&self, _: &str) {}
    fn operation(&self, _: &str) {}
    fn detail(&self, _: &str) {}
    fn finish(&self, _: bool) {}
}

fn interactive() -> bool {
    user_attended_stderr()
        && env::var_os("CI").is_none()
        && env::var_os("TERM").is_none_or(|term| term != "dumb")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<String>>);

    impl EventSink for RecordingSink {
        fn section(&self, message: &str) {
            self.0.lock().unwrap().push(format!("section:{message}"));
        }
        fn operation(&self, message: &str) {
            self.0.lock().unwrap().push(format!("operation:{message}"));
        }
        fn detail(&self, message: &str) {
            self.0.lock().unwrap().push(format!("detail:{message}"));
        }
        fn finish(&self, success: bool) {
            self.0.lock().unwrap().push(format!("finish:{success}"));
        }
    }

    #[test]
    fn injected_sink_records_business_events_without_a_terminal() {
        let sink = RecordingSink::default();
        sink.section("capture");
        sink.operation("GET /version");
        sink.finish(true);
        assert_eq!(
            *sink.0.lock().unwrap(),
            ["section:capture", "operation:GET /version", "finish:true"]
        );
    }
}

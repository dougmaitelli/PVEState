#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeDirective {
    pub(crate) line: usize,
    pub(crate) text: String,
    pub(crate) reason: String,
}

impl NativeDirective {
    pub(crate) fn new(line: usize, text: &str, reason: impl Into<String>) -> Self {
        Self {
            line,
            text: text.to_string(),
            reason: reason.into(),
        }
    }

    pub(crate) fn diagnostic(&self) -> String {
        format!("line {} (`{}`): {}", self.line, self.text, self.reason)
    }
}

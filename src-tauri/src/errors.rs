use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AppError {
    pub error_code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn new(error_code: &'static str, message: impl Into<String>) -> Self {
        Self {
            error_code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub error_code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(error_code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_code: error_code.into(),
            message: message.into(),
        }
    }
}

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        Self::new(value.error_code, value.message)
    }
}

pub type AppResult<T> = Result<T, AppError>;

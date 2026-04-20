// Core modules
pub mod api;
pub mod clob_sdk;
pub mod config;
pub mod copy_trading;
pub mod web_state;
pub mod activity_stream;

// Trading and analysis modules
pub mod backtest;
pub mod detector;
pub mod merge;
pub mod models;
pub mod monitor;
pub mod rtds;
pub mod simulation;
pub mod trader;

// Utility modules
pub mod utils;

// Re-export commonly used types and utilities
pub use api::PolymarketApi;
pub use config::Config;
pub use models::TokenPrice;
pub use utils::{is_valid_eth_address, normalize_eth_address, format_duration, truncate_string};

// Global file writer for history.toml (initialized by main.rs)
use std::sync::{Mutex, OnceLock};
use std::fs::File;
use std::io::Write;

static HISTORY_FILE: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init_history_file(file: File) {
    HISTORY_FILE.set(Mutex::new(file)).expect("History file already initialized");
}

/// Log levels for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Debug => write!(f, "DEBUG"),
        }
    }
}

/// Log a message with timestamp and level to both stderr and history file
pub fn log_with_level(level: LogLevel, message: &str) {
    use chrono::Utc;
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let formatted = format!("[{}] [{}] {}\n", timestamp, level, message);

    // Write to stderr
    eprint!("{}", formatted);
    use std::io::Write;
    let _ = std::io::stderr().flush();

    // Write to history file if initialized
    if let Some(file_mutex) = HISTORY_FILE.get() {
        if let Ok(mut file) = file_mutex.lock() {
            let _ = write!(file, "{}", formatted);
            let _ = file.flush();
        }
    }
}

// Logging functions - modules will use these
pub fn log_to_history(message: &str) {
    log_with_level(LogLevel::Info, message);
}

pub fn log_trading_event(event: &str) {
    log_with_level(LogLevel::Info, &format!("TRADE: {}", event));
}

/// Convenience functions for different log levels
pub fn log_error(message: &str) {
    log_with_level(LogLevel::Error, message);
}

pub fn log_warn(message: &str) {
    log_with_level(LogLevel::Warn, message);
}

pub fn log_info(message: &str) {
    log_with_level(LogLevel::Info, message);
}

pub fn log_debug(message: &str) {
    log_with_level(LogLevel::Debug, message);
}

// Macro for logging - modules use crate::log_println!
#[macro_export]
macro_rules! log_println {
    ($($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            $crate::log_to_history(&format!("{}\n", message));
        }
    };
}

use std::sync::atomic::{AtomicBool, Ordering};

use flexi_logger::*;
use indicatif::MultiProgress;
use log::Log;

static EMOJI_ENABLED: AtomicBool = AtomicBool::new(true);

fn emoji_enabled() -> bool {
    EMOJI_ENABLED.load(Ordering::Relaxed)
}

fn make_prefix(logger_emoji: &str, level: log::Level) -> String {
    if emoji_enabled() {
        let level_emoji = match level {
            log::Level::Error => "❌",
            log::Level::Warn => "⚠️",
            log::Level::Info => "ℹ️",
            log::Level::Debug => "❓",
            log::Level::Trace => "🖊️",
        };
        format!("{} {} ", logger_emoji, level_emoji)
    } else {
        String::new()
    }
}

fn logger_formatter_activate(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "{}[activate] [{}] {}",
        make_prefix("⭐", level),
        style(level).paint(level.to_string()),
        record.args()
    )
}

fn logger_formatter_wait(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "{}[wait] [{}] {}",
        make_prefix("👀", level),
        style(level).paint(level.to_string()),
        record.args()
    )
}

fn logger_formatter_revoke(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "{}[revoke] [{}] {}",
        make_prefix("↩️", level),
        style(level).paint(level.to_string()),
        record.args()
    )
}

fn logger_formatter_deploy(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "{}[deploy] [{}] {}",
        make_prefix("🚀", level),
        style(level).paint(level.to_string()),
        record.args()
    )
}

pub enum LoggerType {
    Deploy,
    Activate,
    Wait,
    Revoke,
}

pub struct LogWrapper {
    bar: MultiProgress,
    log: Box<dyn Log>,
}

impl LogWrapper {
    pub fn new(bar: MultiProgress, log: Box<dyn Log>) -> Self {
        Self { bar, log }
    }

    pub fn try_init(self) -> Result<(), log::SetLoggerError> {
        use log::LevelFilter::*;
        let levels = [Off, Error, Warn, Info, Debug, Trace];

        for level_filter in levels.iter().rev() {
            let level = if let Some(level) = level_filter.to_level() {
                level
            } else {
                continue;
            };
            let meta = log::Metadata::builder().level(level).build();
            if self.enabled(&meta) {
                log::set_max_level(*level_filter);
                break;
            }
        }

        log::set_boxed_logger(Box::new(self))
    }
    pub fn multi(&self) -> MultiProgress {
        self.bar.clone()
    }
}

impl Log for LogWrapper {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.log.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.log.enabled(record.metadata()) {
            self.bar.suspend(|| self.log.log(record))
        }
    }

    fn flush(&self) {
        self.log.flush()
    }
}

pub fn init_logger(
    debug_logs: bool,
    log_dir: Option<&str>,
    logger_type: &LoggerType,
    no_emoji: bool,
) -> Result<(MultiProgress, LoggerHandle), FlexiLoggerError> {
    EMOJI_ENABLED.store(!no_emoji, Ordering::Relaxed);

    let logger_formatter = match logger_type {
        LoggerType::Deploy => logger_formatter_deploy,
        LoggerType::Activate => logger_formatter_activate,
        LoggerType::Wait => logger_formatter_wait,
        LoggerType::Revoke => logger_formatter_revoke,
    };

    let (logger, handle) = if let Some(log_dir) = log_dir {
        let mut file_spec = FileSpec::default().directory(log_dir);

        match logger_type {
            LoggerType::Activate => file_spec = file_spec.discriminant("activate"),
            LoggerType::Wait => file_spec = file_spec.discriminant("wait"),
            LoggerType::Revoke => file_spec = file_spec.discriminant("revoke"),
            LoggerType::Deploy => (),
        }

        Logger::try_with_env_or_str("debug")?
            .log_to_file(file_spec)
            .format_for_stderr(logger_formatter)
            .set_palette("196;208;51;7;8".to_string())
            .duplicate_to_stderr(match debug_logs {
                true => Duplicate::Debug,
                false => Duplicate::Info,
            })
            .print_message()
            .build()?
    } else {
        Logger::try_with_env_or_str(match debug_logs {
            true => "debug",
            false => "info",
        })?
        .log_to_stderr()
        .format(logger_formatter)
        .set_palette("196;208;51;7;8".to_string())
        .build()?
    };

    let multi = MultiProgress::new();
    LogWrapper::new(multi.clone(), logger).try_init().unwrap();

    Ok((multi, handle))
}

use anyhow::Result;
use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Logger {
    log_file: PathBuf,
    error_file: PathBuf,
    verbose: bool,
}

impl Logger {
    pub fn new(project_path: &Path, verbose: bool) -> Result<Self> {
        let logs_dir = project_path.join(".docs").join("logs");
        fs::create_dir_all(&logs_dir)?;

        let date = Local::now().format("%Y%m%d").to_string();
        let log_file = logs_dir.join(format!("{}-porpoise.log", date));
        let error_file = logs_dir.join(format!("{}-porpoise.error.log", date));

        let logger = Logger { log_file, error_file, verbose };
        logger.write_log("INFO", "porpoise", "Session started")?;
        Ok(logger)
    }

    pub fn info(&self, role: &str, msg: &str) {
        if let Err(e) = self.write_log("INFO", role, msg) {
            eprintln!("[logger error] {}", e);
        }
        if self.verbose {
            eprintln!("[{}] [{}] {}", "INFO".cyan_str(), role, msg);
        }
    }

    pub fn warn(&self, role: &str, msg: &str) {
        if let Err(e) = self.write_log("WARN", role, msg) {
            eprintln!("[logger error] {}", e);
        }
        if self.verbose {
            eprintln!("[{}] [{}] {}", "WARN".yellow_str(), role, msg);
        }
    }

    pub fn error(&self, role: &str, msg: &str) {
        let _ = self.write_log("ERROR", role, msg);
        let _ = self.write_error_log(role, msg);
        eprintln!("[{}] [{}] {}", "ERROR".red_str(), role, msg);
    }

    pub fn debug(&self, role: &str, msg: &str) {
        if !self.verbose {
            return;
        }
        let _ = self.write_log("DEBUG", role, msg);
        eprintln!("[{}] [{}] {}", "DEBUG".dimmed_str(), role, msg);
    }

    pub fn role_start(&self, role: &str, cycle: u32) {
        let msg = format!("cycle={} role={} started", cycle, role);
        self.info(role, &msg);
    }

    pub fn role_end(&self, role: &str, cycle: u32, success: bool) {
        let status = if success { "completed" } else { "failed" };
        let msg = format!("cycle={} role={} {}", cycle, role, status);
        if success {
            self.info(role, &msg);
        } else {
            self.error(role, &msg);
        }
    }

    pub fn log_path(&self) -> &Path {
        &self.log_file
    }

    fn write_log(&self, level: &str, role: &str, msg: &str) -> Result<()> {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let line = format!("[{}] [{:<5}] [{}] {}\n", ts, level, role, msg);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    fn write_error_log(&self, role: &str, msg: &str) -> Result<()> {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let line = format!("[{}] [ERROR] [{}] {}\n", ts, role, msg);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.error_file)?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }
}

// Minimal color helpers without importing colored in this module
trait ColorStr {
    fn cyan_str(&self) -> String;
    fn yellow_str(&self) -> String;
    fn red_str(&self) -> String;
    fn dimmed_str(&self) -> String;
}

impl ColorStr for str {
    fn cyan_str(&self) -> String { format!("\x1b[36m{}\x1b[0m", self) }
    fn yellow_str(&self) -> String { format!("\x1b[33m{}\x1b[0m", self) }
    fn red_str(&self) -> String { format!("\x1b[31m{}\x1b[0m", self) }
    fn dimmed_str(&self) -> String { format!("\x1b[2m{}\x1b[0m", self) }
}

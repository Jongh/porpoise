use colored::Colorize;
use std::path::{Path, PathBuf};

const MAX_CONTEXT_CHARS: u64 = 800_000; // ~200k tokens at 0.25 tokens/char

#[derive(Debug, Clone)]
pub enum TokenWarningLevel {
    Normal,
    Caution(u8),
    Warning(u8),
    Critical(u8),
}

pub struct TokenMonitor {
    /// Sorted ascending (e.g. [70, 85, 95])
    thresholds: Vec<u8>,
    project_path: PathBuf,
}

impl TokenMonitor {
    pub fn new(thresholds: Vec<u8>, path: &Path) -> Self {
        let mut t = thresholds;
        t.sort();
        t.dedup();
        TokenMonitor {
            thresholds: t,
            project_path: path.to_path_buf(),
        }
    }

    /// Estimate token usage from total size of files in .docs/reports/
    fn estimate_chars_used(&self) -> u64 {
        let reports_dir = self.project_path.join(".docs").join("reports");
        if !reports_dir.exists() {
            return 0;
        }

        let mut total_chars: u64 = 0;
        if let Ok(entries) = std::fs::read_dir(&reports_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        total_chars += metadata.len();
                    }
                }
            }
        }
        total_chars
    }

    pub fn check_usage(&self) -> TokenWarningLevel {
        let chars_used = self.estimate_chars_used();
        let percent = ((chars_used as f64 / MAX_CONTEXT_CHARS as f64) * 100.0).min(255.0) as u8;

        if self.thresholds.is_empty() {
            return TokenWarningLevel::Normal;
        }

        let t0 = self.thresholds[0];
        let t1 = self.thresholds.get(1).copied().unwrap_or(85);
        let t2 = self.thresholds.get(2).copied().unwrap_or(95);

        if percent >= t2 {
            TokenWarningLevel::Critical(percent)
        } else if percent >= t1 {
            TokenWarningLevel::Warning(percent)
        } else if percent >= t0 {
            TokenWarningLevel::Caution(percent)
        } else {
            TokenWarningLevel::Normal
        }
    }

    pub fn display_warning(&self, level: &TokenWarningLevel) {
        match level {
            TokenWarningLevel::Normal => {
                // No output for normal level
            }
            TokenWarningLevel::Caution(pct) => {
                println!(
                    "{}",
                    format!("⚠ Token usage: {}% (caution threshold reached)", pct).yellow()
                );
            }
            TokenWarningLevel::Warning(pct) => {
                println!(
                    "{}",
                    format!("⚠ Token usage: {}% (WARNING - approaching limit)", pct)
                        .yellow()
                        .bold()
                );
            }
            TokenWarningLevel::Critical(pct) => {
                println!(
                    "{}",
                    format!(
                        "✗ Token usage: {}% (CRITICAL - context window nearly full)",
                        pct
                    )
                    .red()
                    .bold()
                );
            }
        }
    }
}

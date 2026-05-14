use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use dialoguer::Confirm;

pub fn confirm_or_default(prompt: &str, default: bool, auto_approve: bool) -> Result<bool> {
    if auto_approve {
        println!("[자동 승인] {} → {}", prompt, if default { "예" } else { "아니오" });
        return Ok(default);
    }
    Ok(Confirm::new()
        .with_prompt(prompt)
        .default(default)
        .interact()?)
}

pub fn collect_multiline_input(prompt: &str) -> Result<String> {
    if std::env::var_os("EDITOR").is_some() || std::env::var_os("VISUAL").is_some() {
        let result = dialoguer::Editor::new()
            .edit("")
            .context("텍스트 에디터 실행 실패 — EDITOR/VISUAL 환경변수에 지정된 프로그램이 PATH에 없거나 실행할 수 없습니다.")?;
        return Ok(result.unwrap_or_default());
    }

    println!("{} (빈 줄 2회로 종료):", prompt);
    let _ = io::stdout().flush();

    let stdin = io::stdin();
    let mut lines: Vec<String> = Vec::new();
    let mut consecutive_empty: u32 = 0;

    for line in stdin.lock().lines() {
        let line = line?;
        if line.is_empty() {
            consecutive_empty += 1;
            if consecutive_empty >= 2 {
                break;
            }
            lines.push(line);
        } else {
            consecutive_empty = 0;
            lines.push(line);
        }
    }

    while lines.last().map(|l: &String| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_trailing_empty_trimmed() {
        let mut lines: Vec<String> = vec!["hello".into(), "world".into(), "".into()];
        while lines.last().map(|l: &String| l.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn test_empty_input_returns_empty() {
        let mut lines: Vec<String> = vec!["".into(), "".into()];
        // Simulate sentinel detection: stop at 2 consecutive empties
        let mut consecutive_empty: u32 = 0;
        let mut collected: Vec<String> = Vec::new();
        for line in lines.drain(..) {
            if line.is_empty() {
                consecutive_empty += 1;
                if consecutive_empty >= 2 {
                    break;
                }
                collected.push(line);
            } else {
                consecutive_empty = 0;
                collected.push(line);
            }
        }
        while collected.last().map(|l: &String| l.is_empty()).unwrap_or(false) {
            collected.pop();
        }
        assert_eq!(collected.join("\n"), "");
    }
}

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use crate::session::v0_7::{ExecutionResult, VerifyCommand};

const MAX_OUTPUT_BYTES: usize = 16 * 1024;

pub fn run_verify_commands(
    project_root: &Path,
    commands: &[VerifyCommand],
    allowed_prefixes: &[String],
    timeout_secs: u32,
) -> Vec<ExecutionResult> {
    commands.iter().map(|cmd| run_single(project_root, cmd, allowed_prefixes, timeout_secs)).collect()
}

fn run_single(
    project_root: &Path,
    cmd: &VerifyCommand,
    allowed_prefixes: &[String],
    timeout_secs: u32,
) -> ExecutionResult {
    // 허용 목록 검사
    if !is_allowed(&cmd.command, allowed_prefixes) {
        return ExecutionResult {
            command: cmd.command.clone(),
            args: cmd.args.clone(),
            purpose: cmd.purpose.clone(),
            exit_code: -1,
            stdout: String::new(),
            stderr: "명령이 허용 목록에 없어 실행을 건너뜀".to_string(),
            duration_ms: 0,
            truncated: false,
        };
    }

    let start = Instant::now();
    let deadline = start + Duration::from_secs(timeout_secs as u64);

    let mut child = match Command::new(&cmd.command)
        .args(&cmd.args)
        .current_dir(project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return ExecutionResult {
            command: cmd.command.clone(),
            args: cmd.args.clone(),
            purpose: cmd.purpose.clone(),
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("명령 실행 오류: {}", e),
            duration_ms: start.elapsed().as_millis() as u64,
            truncated: false,
        },
    };

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                return match child.wait_with_output() {
                    Ok(output) => {
                        let (stdout, truncated_out) = truncate_output(&output.stdout);
                        let (stderr, truncated_err) = truncate_output(&output.stderr);
                        ExecutionResult {
                            command: cmd.command.clone(),
                            args: cmd.args.clone(),
                            purpose: cmd.purpose.clone(),
                            exit_code: output.status.code().unwrap_or(-1),
                            stdout,
                            stderr,
                            duration_ms,
                            truncated: truncated_out || truncated_err,
                        }
                    }
                    Err(e) => ExecutionResult {
                        command: cmd.command.clone(),
                        args: cmd.args.clone(),
                        purpose: cmd.purpose.clone(),
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: format!("출력 수집 오류: {}", e),
                        duration_ms,
                        truncated: false,
                    },
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    child.kill().ok();
                    child.wait().ok(); // 좀비 프로세스 회수
                    return ExecutionResult {
                        command: cmd.command.clone(),
                        args: cmd.args.clone(),
                        purpose: cmd.purpose.clone(),
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: format!("타임아웃 ({}초 초과)", timeout_secs),
                        duration_ms: timeout_secs as u64 * 1000,
                        truncated: false,
                    };
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return ExecutionResult {
                command: cmd.command.clone(),
                args: cmd.args.clone(),
                purpose: cmd.purpose.clone(),
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("명령 실행 오류: {}", e),
                duration_ms: start.elapsed().as_millis() as u64,
                truncated: false,
            },
        }
    }
}

fn is_allowed(command: &str, allowed_prefixes: &[String]) -> bool {
    if allowed_prefixes.is_empty() { return false; }
    let base = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);
    allowed_prefixes.iter().any(|prefix| {
        command == prefix || command.starts_with(&format!("{}/", prefix))
            || base == prefix || base.starts_with(prefix.as_str())
    })
}

fn truncate_output(bytes: &[u8]) -> (String, bool) {
    if bytes.len() > MAX_OUTPUT_BYTES {
        let truncated = String::from_utf8_lossy(&bytes[..MAX_OUTPUT_BYTES]).to_string();
        (truncated + "\n[... output truncated ...]", true)
    } else {
        (String::from_utf8_lossy(bytes).to_string(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(command: &str, args: &[&str], purpose: &str) -> VerifyCommand {
        VerifyCommand {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            purpose: purpose.to_string(),
            expected_exit_code: 0,
        }
    }

    #[test]
    fn empty_prefix_list_blocks_all() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = vec![cmd("echo", &["hello"], "test")];
        let results = run_verify_commands(tmp.path(), &cmds, &[], 5);
        assert_eq!(results[0].exit_code, -1);
        assert!(results[0].stderr.contains("허용 목록"));
    }

    #[test]
    fn blocked_command_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = vec![cmd("rm", &["-rf", "/"], "dangerous")];
        let results = run_verify_commands(tmp.path(), &cmds, &["echo".to_string()], 5);
        assert_eq!(results[0].exit_code, -1);
        assert!(results[0].stderr.contains("허용 목록"));
    }

    #[test]
    fn allowed_command_runs() {
        let tmp = tempfile::tempdir().unwrap();
        // 'echo'는 모든 플랫폼에서 사용 가능
        let allowed = vec!["echo".to_string()];
        let cmds = vec![cmd("echo", &["hello"], "echo test")];
        let results = run_verify_commands(tmp.path(), &cmds, &allowed, 10);
        // echo가 PATH에 있으면 exit_code=0, 없으면 -1 (환경 무관 테스트)
        // 어느 쪽이든 stderr에 "허용 목록" 메시지는 없어야 함
        assert!(!results[0].stderr.contains("허용 목록"));
    }

    #[test]
    fn multiple_results_returned() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = vec![
            cmd("echo", &["a"], "first"),
            cmd("echo", &["b"], "second"),
        ];
        let results = run_verify_commands(tmp.path(), &cmds, &[], 5);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn timeout_kills_process_and_returns_timed_out() {
        let tmp = tempfile::tempdir().unwrap();
        // 플랫폼별 장시간 실행 명령
        #[cfg(unix)]
        let (command, args, allowed) = ("sleep", vec!["10"], "sleep");
        #[cfg(windows)]
        let (command, args, allowed) = ("ping", vec!["-n", "20", "127.0.0.1"], "ping");

        let cmds = vec![cmd(command, &args, "long running")];
        let start = std::time::Instant::now();
        let results = run_verify_commands(tmp.path(), &cmds, &[allowed.to_string()], 1);
        let elapsed = start.elapsed().as_secs();

        assert_eq!(results[0].exit_code, -1, "타임아웃 시 exit_code는 -1이어야 함");
        assert!(results[0].stderr.contains("타임아웃"), "타임아웃 메시지 없음: {}", results[0].stderr);
        // 타임아웃(1초) + 폴링 오버헤드(최대 0.1초) 이내에 반환되어야 함
        assert!(elapsed <= 3, "타임아웃 후 과도한 대기 시간: {}초", elapsed);
    }

    #[test]
    fn spawn_failure_returns_error_result() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = vec![cmd("__nonexistent_binary__", &[], "should fail")];
        let allowed = vec!["__nonexistent_binary__".to_string()];
        let results = run_verify_commands(tmp.path(), &cmds, &allowed, 5);
        assert_eq!(results[0].exit_code, -1);
        assert!(results[0].stderr.contains("명령 실행 오류"), "오류 메시지 없음: {}", results[0].stderr);
    }
}

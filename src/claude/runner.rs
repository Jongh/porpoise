use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TOKEN_LIMIT_MARKER: &str = "PORPOISE_TOKEN_LIMIT";
const TOKEN_LIMIT_PATTERNS: &[&str] = &[
    "You've hit your limit",
    "you've hit your limit",
    "You've hit the limit",
];

/// 계측된 에이전트 실행 결과 — 최종 출력 텍스트와 비용·토큰(가용 시).
///
/// Claude Code의 구조화 출력(`--output-format stream-json`)에서 비용을 얻는다. CLI가 비용을
/// 제공하지 않으면(구버전·미지원) 해당 필드는 `None`이며 하드 실패하지 않는다(graceful 저하).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentRun {
    pub output: String,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// stream-json 한 줄의 해석 결과.
#[derive(Debug, PartialEq)]
enum StreamEvent {
    /// assistant 메시지의 텍스트(증분 표시용).
    AssistantText(String),
    /// 최종 result 이벤트 — 최종 텍스트 + 비용·토큰.
    Result {
        text: String,
        cost_usd: Option<f64>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    /// 그 외 이벤트(system init 등) — 무시.
    Other,
}

/// stream-json 한 줄을 해석한다. JSON이 아니면 `None`(평문 폴백 신호).
fn parse_stream_event(line: &str) -> Option<StreamEvent> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => {
            let text = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| {
                            if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                b.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            Some(StreamEvent::AssistantText(text))
        }
        Some("result") => {
            let text = v
                .get("result")
                .and_then(|r| r.as_str())
                .unwrap_or_default()
                .to_string();
            let cost_usd = v.get("total_cost_usd").and_then(|c| c.as_f64());
            let usage = v.get("usage");
            let input_tokens = usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|t| t.as_u64());
            let output_tokens = usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|t| t.as_u64());
            Some(StreamEvent::Result {
                text,
                cost_usd,
                input_tokens,
                output_tokens,
            })
        }
        _ => Some(StreamEvent::Other),
    }
}

pub struct ClaudeRunner {
    binary_path: PathBuf,
}

impl ClaudeRunner {
    pub fn new() -> Result<Self> {
        let binary_path = which::which("claude").context(
            "Claude CLI binary not found in PATH. \
             Please install Claude Code: https://claude.ai/code \
             and ensure 'claude' is available in your PATH.",
        )?;
        Ok(ClaudeRunner { binary_path })
    }

    /// Run claude with an already-rendered prompt string and context files.
    /// Use this when the prompt content is generated at runtime (e.g., after template substitution).
    /// Pass `output_file: None` to skip writing to disk.
    pub fn run_with_prompt_str(
        &self,
        prompt_str: &str,
        context_files: &[PathBuf],
        output_file: Option<&Path>,
        model: Option<&str>,
    ) -> Result<String> {
        let prompt = self.build_prompt_from_content(prompt_str, context_files)?;
        self.execute_claude(&prompt, output_file, model, None, true)
    }

    /// Run claude as a full agentic session inside `working_dir`.
    ///
    /// Unlike `run_with_prompt_str`, this sets the child process's current
    /// directory so the agent reads and writes files relative to an isolated
    /// worktree. The agent is free to plan, edit, and run tools on its own —
    /// Porpoise does not constrain it to a single phase. Returns the captured
    /// stdout (the agent's final narration), not a structured report.
    /// `stream=true`면 에이전트 출력을 라인 단위로 즉시 출력(순차), `false`면 캡처만(병렬, M23).
    pub fn run_agentic(
        &self,
        prompt: &str,
        working_dir: &Path,
        model: Option<&str>,
        stream: bool,
    ) -> Result<String> {
        self.execute_claude(prompt, None, model, Some(working_dir), stream)
    }

    /// 비용 계측 에이전트 실행 — `--output-format stream-json`으로 호출해 출력과 함께
    /// 비용·토큰을 캡처한다(M28). 스트리밍 표시는 유지하며, CLI가 stream-json/비용을
    /// 지원하지 않으면 평문 폴백 + 비용 `None`으로 graceful 저하한다.
    pub fn run_agentic_metered(
        &self,
        prompt: &str,
        working_dir: &Path,
        model: Option<&str>,
        stream: bool,
    ) -> Result<AgentRun> {
        self.execute_claude_metered(prompt, model, working_dir, stream)
    }

    /// stream-json 모드로 claude를 실행하고 출력·비용을 파싱한다.
    fn execute_claude_metered(
        &self,
        prompt: &str,
        model: Option<&str>,
        working_dir: &Path,
        stream: bool,
    ) -> Result<AgentRun> {
        let mut cmd = self.make_command();
        cmd.arg("-p")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }
        cmd.current_dir(working_dir);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn claude from: {}", self.binary_path.display()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write prompt to claude stdin")?;
        }

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let reader = BufReader::new(stdout);

        let mut accumulated = String::new(); // assistant 텍스트(또는 평문 폴백) 누적
        let mut result: Option<AgentRun> = None;

        for line in reader.lines() {
            let l = match line {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Error reading claude output: {}", e);
                    break;
                }
            };
            if l.trim().is_empty() {
                continue;
            }
            match parse_stream_event(&l) {
                Some(StreamEvent::AssistantText(t)) => {
                    // tool_use만 있는 이벤트는 텍스트가 비어 있음 — 빈 줄 노이즈 방지
                    if !t.is_empty() {
                        if stream {
                            println!("{}", t);
                        }
                        accumulated.push_str(&t);
                        accumulated.push('\n');
                    }
                }
                Some(StreamEvent::Result {
                    text,
                    cost_usd,
                    input_tokens,
                    output_tokens,
                }) => {
                    result = Some(AgentRun {
                        output: text,
                        cost_usd,
                        input_tokens,
                        output_tokens,
                    });
                }
                Some(StreamEvent::Other) => {}
                None => {
                    // 비-JSON 라인 — stream-json 미지원 CLI 등. 평문으로 처리(폴백).
                    if stream {
                        println!("{}", l);
                    }
                    accumulated.push_str(&l);
                    accumulated.push('\n');
                }
            }
        }

        let status = child.wait().context("Failed to wait for claude process")?;

        let mut run = result.unwrap_or_default();
        // result 이벤트가 없거나 빈 텍스트면 누적 텍스트로 폴백.
        if run.output.trim().is_empty() {
            run.output = accumulated;
        }

        if !status.success() && run.output.is_empty() {
            anyhow::bail!(
                "claude exited with code {}. Ensure claude is properly configured.",
                status.code().unwrap_or(-1)
            );
        }

        // 토큰 한도 마커(레거시 경로와 동일 처리).
        for pat in TOKEN_LIMIT_PATTERNS {
            if run.output.contains(pat) {
                run.output.push_str(&format!("\n{}\n", TOKEN_LIMIT_MARKER));
                break;
            }
        }

        Ok(run)
    }

    /// Build a Command that correctly invokes the claude binary.
    ///
    /// On Windows, npm-installed CLIs are `.cmd` batch wrappers that cannot be
    /// spawned directly — they require `cmd.exe /C` as the launcher.
    ///
    /// `--dangerously-skip-permissions` is always passed so that claude can
    /// write/edit files without prompting for interactive TTY confirmation,
    /// which is impossible when stdin is a pipe carrying the prompt text.
    fn make_command(&self) -> Command {
        #[cfg(windows)]
        {
            let ext = self
                .binary_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if matches!(ext, "cmd" | "bat") {
                let mut cmd = Command::new("cmd");
                cmd.arg("/C").arg(&self.binary_path);
                cmd.arg("--dangerously-skip-permissions");
                return cmd;
            }
        }
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("--dangerously-skip-permissions");
        cmd
    }

    /// Build a combined prompt: context file contents prepended, then the role prompt content.
    fn build_prompt_from_content(
        &self,
        prompt_content: &str,
        context_files: &[PathBuf],
    ) -> Result<String> {
        let mut prompt = String::new();

        for ctx_file in context_files {
            if !ctx_file.exists() {
                continue;
            }
            let content = fs::read_to_string(ctx_file).with_context(|| {
                format!("Failed to read context file: {}", ctx_file.display())
            })?;
            let filename = ctx_file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| ctx_file.display().to_string());
            prompt.push_str(&format!("=== {} ===\n{}\n\n", filename, content));
        }

        prompt.push_str(prompt_content);
        Ok(prompt)
    }

    /// Spawn claude, pipe the prompt string via stdin, stream stdout, and optionally save output.
    fn execute_claude(
        &self,
        prompt: &str,
        output_file: Option<&Path>,
        model: Option<&str>,
        working_dir: Option<&Path>,
        stream: bool,
    ) -> Result<String> {
        // On Windows, .cmd/.bat files cannot be spawned directly via CreateProcess.
        // They must be invoked through `cmd.exe /C`.
        let mut cmd = self.make_command();
        cmd.arg("-p");
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn claude from: {}",
                self.binary_path.display()
            )
        })?;

        // Write prompt to stdin, then close it to signal EOF.
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write prompt to claude stdin")?;
            // stdin is dropped here — EOF is sent automatically.
        }

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let reader = BufReader::new(stdout);
        let mut full_output = String::new();

        for line in reader.lines() {
            match line {
                Ok(l) => {
                    // 병렬 실행 시 출력 인터리브 방지: stream=false면 캡처만 (M23)
                    if stream {
                        println!("{}", l);
                    }
                    full_output.push_str(&l);
                    full_output.push('\n');
                }
                Err(e) => {
                    eprintln!("Error reading claude output: {}", e);
                    break;
                }
            }
        }

        let status = child.wait().context("Failed to wait for claude process")?;

        if !status.success() && full_output.is_empty() {
            anyhow::bail!(
                "claude exited with code {}. Ensure claude is properly configured.",
                status.code().unwrap_or(-1)
            );
        }

        // Check for token limit patterns and append a marker if found
        for pat in TOKEN_LIMIT_PATTERNS {
            if full_output.contains(pat) {
                full_output.push_str(&format!("\n{}\n", TOKEN_LIMIT_MARKER));
                break;
            }
        }

        if !full_output.is_empty() {
            if let Some(output_file) = output_file {
                // output_file은 caller(roles.rs)가 project 내부 경로로 결정하므로 경계 검사 생략.
                if let Some(parent) = output_file.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("디렉토리 생성 실패: {}", parent.display()))?;
                }
                fs::write(output_file, &full_output).with_context(|| {
                    format!("Failed to write output to {}", output_file.display())
                })?;
            }
        }

        Ok(full_output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_result_event_extracts_cost_and_tokens() {
        let line = r#"{"type":"result","subtype":"success","result":"끝났습니다","total_cost_usd":0.0123,"usage":{"input_tokens":1500,"output_tokens":420},"num_turns":3}"#;
        match parse_stream_event(line).unwrap() {
            StreamEvent::Result {
                text,
                cost_usd,
                input_tokens,
                output_tokens,
            } => {
                assert_eq!(text, "끝났습니다");
                assert_eq!(cost_usd, Some(0.0123));
                assert_eq!(input_tokens, Some(1500));
                assert_eq!(output_tokens, Some(420));
            }
            other => panic!("result 이벤트여야 함: {:?}", other),
        }
    }

    #[test]
    fn parse_result_event_without_cost_is_none() {
        // 구버전 CLI: total_cost_usd/usage 없음 → None (graceful)
        let line = r#"{"type":"result","result":"done"}"#;
        match parse_stream_event(line).unwrap() {
            StreamEvent::Result {
                text,
                cost_usd,
                input_tokens,
                output_tokens,
            } => {
                assert_eq!(text, "done");
                assert_eq!(cost_usd, None);
                assert_eq!(input_tokens, None);
                assert_eq!(output_tokens, None);
            }
            other => panic!("result 이벤트여야 함: {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_event_extracts_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"작업 중"},{"type":"tool_use","name":"Edit"}]}}"#;
        assert_eq!(
            parse_stream_event(line).unwrap(),
            StreamEvent::AssistantText("작업 중".to_string())
        );
    }

    #[test]
    fn parse_system_event_is_other() {
        let line = r#"{"type":"system","subtype":"init","model":"claude-x"}"#;
        assert_eq!(parse_stream_event(line).unwrap(), StreamEvent::Other);
    }

    #[test]
    fn parse_non_json_is_none() {
        // 평문(stream-json 미지원) → None → 호출부가 평문 폴백
        assert!(parse_stream_event("그냥 평문 출력입니다").is_none());
        assert!(parse_stream_event("").is_none());
    }
}

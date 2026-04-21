use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

    /// Run claude with a prompt file and context files.
    /// Embeds context file contents directly into the prompt, then calls `claude -p`
    /// with the prompt piped via stdin (avoids Windows command-line length limits).
    /// Streams stdout to terminal and saves full output to output_file.
    pub fn run_with_prompt(
        &self,
        prompt_file: &Path,
        context_files: &[PathBuf],
        output_file: &Path,
    ) -> Result<String> {
        if let Some(parent) = output_file.parent() {
            fs::create_dir_all(parent)?;
        }

        let prompt = self.build_prompt(prompt_file, context_files)?;

        // On Windows, .cmd/.bat files cannot be spawned directly via CreateProcess.
        // They must be invoked through `cmd.exe /C`.
        let mut cmd = self.make_command();
        cmd.arg("-p");
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
                    println!("{}", l);
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

        if !full_output.is_empty() {
            fs::write(output_file, &full_output).with_context(|| {
                format!("Failed to write output to {}", output_file.display())
            })?;
        }

        Ok(full_output)
    }

    /// Build a Command that correctly invokes the claude binary.
    ///
    /// On Windows, npm-installed CLIs are `.cmd` batch wrappers that cannot be
    /// spawned directly — they require `cmd.exe /C <path>` as the launcher.
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
                return cmd;
            }
        }
        Command::new(&self.binary_path)
    }

    /// Build a combined prompt: context file contents prepended, then the role prompt.
    fn build_prompt(&self, prompt_file: &Path, context_files: &[PathBuf]) -> Result<String> {
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

        let role_prompt = fs::read_to_string(prompt_file).with_context(|| {
            format!("Failed to read prompt file: {}", prompt_file.display())
        })?;
        prompt.push_str(&role_prompt);

        Ok(prompt)
    }
}

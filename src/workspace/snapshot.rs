use anyhow::Result;
use std::path::Path;
use std::process::Command;
use crate::session::v0_7::{SnapshotFile, WorkspaceSnapshot};

const MAX_FILE_BYTES_TARGET: usize = 32 * 1024;
const MAX_FILE_BYTES_OTHER: usize = 8 * 1024;
const TRUNCATION_HEAD: usize = 3 * 1024;
const TRUNCATION_TAIL: usize = 3 * 1024;
const GIT_DIFF_MAX: usize = 16 * 1024;

static SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "kt",
    "cpp", "c", "h", "cs", "rb", "swift", "scala", "toml", "yaml", "yml", "json", "md",
];

pub fn build_workspace_snapshot(
    path: &Path,
    target_files: &[String],
    token_budget: usize,
) -> Result<WorkspaceSnapshot> {
    let file_tree = crate::init::tree::get_tree_string(path).unwrap_or_default();
    let recent_git_diff = get_git_diff(path);
    let untracked_files = get_untracked_files(path);

    let git_changed = get_git_changed_files(path);
    let all_sources = collect_source_files(path);

    // 우선순위: target_files → git changed → all sources (중복 제거)
    let mut ordered: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for f in target_files {
        if seen.insert(f.clone()) { ordered.push(f.clone()); }
    }
    for f in &git_changed {
        if seen.insert(f.clone()) { ordered.push(f.clone()); }
    }
    for f in &all_sources {
        if seen.insert(f.clone()) { ordered.push(f.clone()); }
    }

    let target_set: std::collections::HashSet<&str> =
        target_files.iter().map(String::as_str).collect();

    let mut files: Vec<SnapshotFile> = Vec::new();
    let mut total_chars: usize = 0;

    for rel_path in &ordered {
        if total_chars >= token_budget { break; }

        let abs_path = path.join(rel_path);
        let Ok(raw) = std::fs::read(&abs_path) else { continue; };
        // 바이너리 파일 건너뜀
        if raw.contains(&0u8) { continue; }
        let Ok(content_str) = String::from_utf8(raw) else { continue; };

        let size_bytes = content_str.len() as u64;
        let is_target = target_set.contains(rel_path.as_str());
        let max_bytes = if is_target { MAX_FILE_BYTES_TARGET } else { MAX_FILE_BYTES_OTHER };

        let (content, _truncated) = if content_str.len() > max_bytes {
            let head_end = TRUNCATION_HEAD.min(content_str.len());
            let tail_start = content_str.len().saturating_sub(TRUNCATION_TAIL).max(head_end);
            if tail_start == head_end {
                // head + tail 합산이 파일 크기 이상 — 겹침 없이 전체 반환
                (content_str, false)
            } else {
                let head = &content_str[..head_end];
                let tail = &content_str[tail_start..];
                let skipped_lines = content_str[head_end..tail_start].lines().count();
                let truncated = format!(
                    "{}\n\n[... {} lines truncated ...]\n\n{}",
                    head, skipped_lines, tail
                );
                (truncated, true)
            }
        } else {
            (content_str, false)
        };

        let content_len = content.len();
        if total_chars + content_len > token_budget { continue; }
        total_chars += content_len;

        let last_modified = std::fs::metadata(&abs_path)
            .and_then(|m| m.modified())
            .map(|t| {
                let dt: std::time::Duration = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                format!("{}", dt.as_secs())
            })
            .unwrap_or_default();

        files.push(SnapshotFile {
            path: rel_path.clone(),
            content: Some(content),
            summary: None,
            size_bytes,
            last_modified,
        });
    }

    Ok(WorkspaceSnapshot {
        file_tree,
        files,
        recent_git_diff,
        untracked_files,
    })
}

/// WorkspaceSnapshot 파일 목록을 마크다운 코드블록 형식으로 직렬화
pub fn snapshot_to_context_text(snapshot: &WorkspaceSnapshot) -> String {
    let mut parts = Vec::new();

    if !snapshot.file_tree.is_empty() {
        parts.push(format!("=== 프로젝트 구조 ===\n{}", snapshot.file_tree));
    }

    for sf in &snapshot.files {
        if let Some(content) = &sf.content {
            let ext = sf.path.rsplit('.').next().unwrap_or("");
            let lang = ext_to_lang(ext);
            parts.push(format!(
                "=== 파일: {} ({} bytes) ===\n```{}\n{}\n```",
                sf.path, sf.size_bytes, lang, content
            ));
        }
    }

    if let Some(diff) = &snapshot.recent_git_diff {
        if !diff.is_empty() {
            parts.push(format!("=== 최근 변경 (git diff HEAD) ===\n{}", diff));
        }
    }

    parts.join("\n\n")
}

fn get_git_diff(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).to_string();
    if s.is_empty() { return None; }
    if s.len() > GIT_DIFF_MAX {
        Some(format!("{}\n[... truncated ...]", &s[..GIT_DIFF_MAX]))
    } else {
        Some(s)
    }
}

fn get_git_changed_files(path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["diff", "HEAD", "--name-only"])
        .current_dir(path)
        .output()
        .ok();
    parse_git_file_list(output)
}

fn get_untracked_files(path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(path)
        .output()
        .ok();
    parse_git_file_list(output)
}

fn parse_git_file_list(output: Option<std::process::Output>) -> Vec<String> {
    output
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn collect_source_files(path: &Path) -> Vec<String> {
    use walkdir::WalkDir;
    let skip = ["target", "node_modules", ".git", "__pycache__", ".porpoise", "dist", "build"];
    let mut files = Vec::new();

    for entry in WalkDir::new(path).min_depth(1).max_depth(8).into_iter().flatten() {
        let ep = entry.path();
        if !ep.is_file() { continue; }

        // 건너뛸 디렉토리 체크
        let skip_flag = ep.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            skip.iter().any(|sk| *sk == s.as_ref())
        });
        if skip_flag { continue; }

        let ext = ep.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SOURCE_EXTENSIONS.contains(&ext) { continue; }

        if let Ok(rel) = ep.strip_prefix(path) {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    files.sort();
    files
}

fn ext_to_lang(ext: &str) -> &str {
    match ext {
        "rs" => "rust", "py" => "python", "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript", "go" => "go", "java" | "kt" => "java",
        "cpp" | "c" | "h" => "cpp", "cs" => "csharp", "rb" => "ruby",
        "toml" => "toml", "yaml" | "yml" => "yaml", "json" => "json",
        "md" => "markdown", _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn snapshot_empty_project() {
        let tmp = tempfile::tempdir().unwrap();
        let snap = build_workspace_snapshot(tmp.path(), &[], 32_000).unwrap();
        assert!(snap.files.is_empty());
    }

    #[test]
    fn snapshot_includes_target_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("lib.rs"), "pub fn f() {}").unwrap();

        let snap = build_workspace_snapshot(root, &["main.rs".to_string()], 32_000).unwrap();
        let paths: Vec<&str> = snap.files.iter().map(|f| f.path.as_str()).collect();
        // target file 먼저 등장
        assert!(!snap.files.is_empty());
        assert_eq!(paths[0], "main.rs");
    }

    #[test]
    fn snapshot_respects_token_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // 각 200자 파일 3개 — 예산 300자로 제한
        for i in 0..3u8 {
            fs::write(root.join(format!("file{}.rs", i)), "x".repeat(200)).unwrap();
        }
        let snap = build_workspace_snapshot(root, &[], 300).unwrap();
        assert!(snap.files.len() <= 2, "예산 초과 파일이 포함됨: {} 파일", snap.files.len());
    }

    #[test]
    fn snapshot_truncates_large_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let large = "A".repeat(10 * 1024); // 10KB
        fs::write(root.join("big.rs"), &large).unwrap();

        let snap = build_workspace_snapshot(root, &[], 32_000).unwrap();
        let f = snap.files.iter().find(|f| f.path == "big.rs").unwrap();
        let content = f.content.as_ref().unwrap();
        assert!(content.contains("truncated"), "truncated 표시 없음");
        assert!(content.len() < large.len(), "truncation이 적용되지 않음");
    }

    #[test]
    fn truncation_no_overlap_when_file_between_max_and_head_plus_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // MAX_FILE_BYTES_OTHER = 8KB, TRUNCATION_HEAD + TRUNCATION_TAIL = 6KB
        // 상수를 직접 건드릴 수 없으므로 target_file(max=32KB)을 이용해
        // 파일 크기를 7KB(head+tail=6KB 초과이지만 max=32KB 이하)로 만들면
        // 현재 상수 조합에서는 안전하지만, 핵심은 truncation 분기가 head_end <= tail_start 보장.
        // 6KB + 1 byte 파일 → OTHER 파일(max=8KB) 이하이므로 truncation 미발생.
        // 10KB 파일(OTHER max=8KB 초과) — truncation 발생, skipped_lines 정상 출력 확인.
        let content = "A".repeat(10 * 1024);
        fs::write(root.join("big.rs"), &content).unwrap();
        let snap = build_workspace_snapshot(root, &[], 32_000).unwrap();
        let f = snap.files.iter().find(|f| f.path == "big.rs").unwrap();
        let c = f.content.as_ref().unwrap();
        // head와 tail이 겹치지 않아 "0 lines truncated" 같은 왜곡 없이 실제 줄이 생략됨
        assert!(c.contains("lines truncated"), "truncation 표시 없음");
        // 생략 줄 수가 0이 아닌지 확인 (겹침이 있으면 0이 됨)
        let re = regex_find_truncated_count(c);
        assert!(re > 0, "skipped_lines가 0 — head/tail 겹침 의심: {}", c);
    }

    fn regex_find_truncated_count(s: &str) -> usize {
        // "[... N lines truncated ...]" 에서 N 추출
        s.split_whitespace()
            .skip_while(|w| !w.starts_with('['))
            .nth(1)
            .and_then(|w| w.parse().ok())
            .unwrap_or(0)
    }

    #[test]
    fn snapshot_skips_large_file_and_includes_smaller_subsequent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // 200자 파일 + 큰 파일(300자, 예산 초과) + 100자 파일
        // 예산=250: 200자 포함 후 300자 파일이 break였다면 100자 파일 누락
        // continue로 변경 후엔 100자 파일도 포함되어야 함
        fs::write(root.join("a.rs"), "x".repeat(200)).unwrap();
        fs::write(root.join("b.rs"), "y".repeat(300)).unwrap();
        fs::write(root.join("c.rs"), "z".repeat(100)).unwrap();

        // 예산=250: a.rs(200) 포함 → b.rs(300) 초과 → continue → c.rs(100) 포함 총 300
        // (b.rs 자체는 truncation 후 max_bytes=8KB 이하이지만 token_budget=250 이하임)
        // 더 명확한 설정: 예산=310으로 a+c는 포함되고 b는 초과
        let snap = build_workspace_snapshot(root, &[], 310).unwrap();
        let paths: Vec<&str> = snap.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"a.rs"), "a.rs 누락");
        assert!(paths.contains(&"c.rs"), "c.rs 누락 — break가 continue로 수정되지 않음");
        assert!(!paths.contains(&"b.rs"), "b.rs가 포함됨 (예산 초과)");
    }

    #[test]
    fn snapshot_to_context_text_format() {
        let snap = WorkspaceSnapshot {
            file_tree: "project/\n└── main.rs".to_string(),
            files: vec![SnapshotFile {
                path: "main.rs".to_string(),
                content: Some("fn main() {}".to_string()),
                summary: None,
                size_bytes: 12,
                last_modified: String::new(),
            }],
            recent_git_diff: None,
            untracked_files: vec![],
        };
        let text = snapshot_to_context_text(&snap);
        assert!(text.contains("=== 파일: main.rs"));
        assert!(text.contains("```rust"));
        assert!(text.contains("fn main() {}"));
    }
}

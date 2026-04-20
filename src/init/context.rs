use anyhow::Result;
use dialoguer::Input;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_name: String,
    pub description: String,
    pub tree_output: String,
    pub detected_files: Vec<String>,
}

pub fn collect_user_description(tree: &str) -> Result<ProjectContext> {
    let current_dir = std::env::current_dir()?;

    let project_name = current_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-project".to_string());

    let detected_files = detect_relevant_files(&current_dir);

    if !detected_files.is_empty() {
        println!("Detected relevant files:");
        for f in &detected_files {
            println!("  - {}", f);
        }
        println!();
    }

    let description: String = Input::new()
        .with_prompt("Please describe your project")
        .interact_text()?;

    Ok(ProjectContext {
        project_name,
        description,
        tree_output: tree.to_string(),
        detected_files,
    })
}

fn detect_relevant_files(dir: &Path) -> Vec<String> {
    let doc_extensions = ["md", "txt", "rst"];
    let src_extensions = ["rs", "py", "ts", "js", "go", "java", "c", "cpp", "h", "hpp"];

    let mut files = Vec::new();

    let walker = WalkBuilder::new(dir)
        .max_depth(Some(3))
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path().to_path_buf();
        if path.is_dir() {
            continue;
        }

        // Skip .git and other ignored dirs
        let path_str = path.to_string_lossy().to_string();
        if path_str.contains("/.git/")
            || path_str.contains("\\.git\\")
            || path_str.contains("/target/")
            || path_str.contains("\\target\\")
        {
            continue;
        }

        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            if doc_extensions.contains(&ext_str.as_str())
                || src_extensions.contains(&ext_str.as_str())
            {
                let rel = path
                    .strip_prefix(dir)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path_str.clone());
                files.push(rel);
            }
        }
    }

    files.sort();
    files
}

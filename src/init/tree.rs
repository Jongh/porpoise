use anyhow::Result;
use ignore::WalkBuilder;
use std::io::Write;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "__pycache__", "target", ".docs"];
const MAX_DEPTH: usize = 3;

pub fn print_tree(path: &Path) -> Result<()> {
    let output = get_tree_string(path)?;
    print!("{}", output);
    Ok(())
}

pub fn get_tree_string(path: &Path) -> Result<String> {
    let mut buf = Vec::new();
    write_tree(&mut buf, path)?;
    Ok(String::from_utf8(buf)?)
}

fn write_tree(writer: &mut dyn Write, root: &Path) -> Result<()> {
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    writeln!(writer, "{}/", root_name)?;

    let entries = collect_entries(root, 0)?;
    let mut file_count = 0;
    let mut dir_count = 0;

    write_entries(writer, &entries, &[], &mut file_count, &mut dir_count)?;

    writeln!(writer)?;
    writeln!(
        writer,
        "{} directories, {} files",
        dir_count, file_count
    )?;

    Ok(())
}

#[derive(Debug)]
struct Entry {
    name: String,
    is_dir: bool,
    children: Vec<Entry>,
}

fn collect_entries(dir: &Path, depth: usize) -> Result<Vec<Entry>> {
    if depth >= MAX_DEPTH {
        return Ok(vec![]);
    }

    let mut entries: Vec<Entry> = Vec::new();

    // Use WalkBuilder to respect .gitignore at depth 1
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .build();

    let mut paths: Vec<PathBuf> = Vec::new();
    for result in walker {
        match result {
            Ok(entry) => {
                let ep = entry.path().to_path_buf();
                // Skip the root itself
                if ep == dir {
                    continue;
                }
                let name = ep
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                // Skip hidden and skipped dirs
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                paths.push(ep);
            }
            Err(_) => continue,
        }
    }

    paths.sort();

    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let is_dir = path.is_dir();
        let children = if is_dir {
            collect_entries(&path, depth + 1)?
        } else {
            vec![]
        };

        entries.push(Entry {
            name,
            is_dir,
            children,
        });
    }

    Ok(entries)
}

fn write_entries(
    writer: &mut dyn Write,
    entries: &[Entry],
    prefix_stack: &[bool], // true = last at this level
    file_count: &mut usize,
    dir_count: &mut usize,
) -> Result<()> {
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == entries.len() - 1;

        // Build prefix string
        let mut prefix = String::new();
        for &was_last in prefix_stack {
            if was_last {
                prefix.push_str("    ");
            } else {
                prefix.push_str("│   ");
            }
        }

        let connector = if is_last { "└── " } else { "├── " };

        if entry.is_dir {
            *dir_count += 1;
            writeln!(writer, "{}{}{}/", prefix, connector, entry.name)?;
            let mut new_stack = prefix_stack.to_vec();
            new_stack.push(is_last);
            write_entries(writer, &entry.children, &new_stack, file_count, dir_count)?;
        } else {
            *file_count += 1;
            writeln!(writer, "{}{}{}", prefix, connector, entry.name)?;
        }
    }

    Ok(())
}

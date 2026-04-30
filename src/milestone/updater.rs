use anyhow::Result;
use std::path::Path;

use crate::logger::Logger;
use crate::utils::fs::write_file;

pub fn update_task_status(path: &Path, task_id: &str, completed: bool, logger: &Logger) {
    if let Err(e) = try_update_task_status(path, task_id, completed) {
        logger.warn(
            "milestone",
            &format!("milestone 파일 업데이트 실패 (task={}): {}", task_id, e),
        );
    }
}

fn try_update_task_status(path: &Path, task_id: &str, completed: bool) -> Result<()> {
    let milestone_id = extract_milestone_id(task_id)?;
    let milestone_path = path
        .join(".porpoise")
        .join("milestones")
        .join(format!("M{}.md", milestone_id));

    if !milestone_path.exists() {
        anyhow::bail!("milestone 파일 없음: {}", milestone_path.display());
    }

    let content = std::fs::read_to_string(&milestone_path)
        .map_err(|e| anyhow::anyhow!("읽기 실패: {}", e))?;

    let (from, to) = if completed {
        (format!("- [ ] {}:", task_id), format!("- [x] {}:", task_id))
    } else {
        (format!("- [x] {}:", task_id), format!("- [ ] {}:", task_id))
    };

    let new_content = content.replace(&from, &to);
    write_file(&milestone_path, &new_content, path)
        .map_err(|e| anyhow::anyhow!("쓰기 실패: {}", e))?;

    Ok(())
}

fn extract_milestone_id(task_id: &str) -> Result<u32> {
    let prefix = task_id
        .split('-')
        .next()
        .ok_or_else(|| anyhow::anyhow!("task_id 형식 오류: {}", task_id))?;
    let id_str = prefix
        .strip_prefix('M')
        .ok_or_else(|| anyhow::anyhow!("task_id가 M으로 시작하지 않음: {}", task_id))?;
    id_str
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("milestone ID 파싱 실패: {}", id_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logger::Logger;

    fn make_logger(dir: &std::path::Path) -> Logger {
        Logger::new(dir, false).unwrap()
    }

    fn setup_milestone(dir: &std::path::Path, id: u32, content: &str) {
        let milestones_dir = dir.join(".porpoise").join("milestones");
        std::fs::create_dir_all(&milestones_dir).unwrap();
        std::fs::write(milestones_dir.join(format!("M{}.md", id)), content).unwrap();
    }

    fn read_milestone(dir: &std::path::Path, id: u32) -> String {
        std::fs::read_to_string(dir.join(".porpoise").join("milestones").join(format!("M{}.md", id)))
            .unwrap()
    }

    #[test]
    fn marks_task_complete_in_milestone_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = "# M1: 테스트\n\n- [ ] M1-T01: 작업1\n- [ ] M1-T02: 작업2\n";
        setup_milestone(dir.path(), 1, content);

        let logger = make_logger(dir.path());
        update_task_status(dir.path(), "M1-T01", true, &logger);

        let result = read_milestone(dir.path(), 1);
        assert!(result.contains("- [x] M1-T01:"));
        assert!(result.contains("- [ ] M1-T02:"));
    }

    #[test]
    fn missing_milestone_file_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".porpoise").join("milestones")).unwrap();
        let logger = make_logger(dir.path());
        // M1.md 없음 — warn만 출력되고 패닉 없어야 함
        update_task_status(dir.path(), "M1-T01", true, &logger);
    }

    #[test]
    fn extract_milestone_id_parses_correctly() {
        assert_eq!(extract_milestone_id("M1-T05").unwrap(), 1);
        assert_eq!(extract_milestone_id("M12-T03").unwrap(), 12);
        assert!(extract_milestone_id("bad").is_err());
        assert!(extract_milestone_id("X1-T01").is_err());
    }
}

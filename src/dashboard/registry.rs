//! 프로젝트 레지스트리 (M32) — `~/.porpoise/registry.json`.
//!
//! 대시보드가 접근할 수 있는 프로젝트의 **허용 목록**이다. 클라이언트는 경로가 아니라
//! 불투명 id로만 프로젝트를 참조하고, 서버는 등록된 경로로만 해석한다(임의 파일시스템
//! 열람 차단). 손상된 레지스트리는 빈 목록으로 우아하게 처리한다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectEntry {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

/// 사용자 홈 디렉터리 (`USERPROFILE` → `HOME` 폴백).
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 레지스트리 파일 경로. 테스트를 위해 베이스 디렉터리를 주입받는 변형을 핵심으로 둔다.
fn registry_path_in(base: &Path) -> PathBuf {
    base.join(".porpoise").join("registry.json")
}

pub fn registry_path() -> Option<PathBuf> {
    home_dir().map(|h| registry_path_in(&h))
}

/// 경로를 정규화한다 (절대화 + 구분자 통일·소문자화(Windows 대소문자 무시 대응)).
pub(crate) fn normalize(path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('/', "\\");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// 정규화된 경로의 안정 id — FNV-1a 64bit (릴리즈 간 안정, 외부 의존 없음).
pub fn project_id(normalized_path: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in normalized_path.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn load_from(file: &Path) -> Registry {
    let Ok(content) = std::fs::read_to_string(file) else {
        return Registry::default();
    };
    serde_json::from_str(content.trim_start_matches('\u{feff}')).unwrap_or_default()
}

fn save_to(file: &Path, reg: &Registry) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(reg).unwrap_or_else(|_| "{}".to_string());
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    if std::fs::rename(&tmp, file).is_err() {
        std::fs::remove_file(file).ok();
        std::fs::rename(&tmp, file)?;
    }
    Ok(())
}

/// 레지스트리에 프로젝트를 등록한다(upsert, 순수 로직). 반환: 변경 여부.
fn upsert(reg: &mut Registry, path: &Path) -> bool {
    let norm = normalize(path);
    let id = project_id(&norm);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| norm.clone());
    let entry = ProjectEntry { id: id.clone(), name, path: norm };
    match reg.projects.iter_mut().find(|p| p.id == id) {
        Some(existing) => {
            if *existing != entry {
                *existing = entry;
                true
            } else {
                false
            }
        }
        None => {
            reg.projects.push(entry);
            true
        }
    }
}

/// id로 등록 경로를 해석한다 (등록되어 있고 `.porpoise`가 실존할 때만 Some).
fn resolve_in(reg: &Registry, id: &str) -> Option<PathBuf> {
    let entry = reg.projects.iter().find(|p| p.id == id)?;
    let path = PathBuf::from(&entry.path);
    if path.join(".porpoise").exists() {
        Some(path)
    } else {
        None
    }
}

// ── 파일 기반 공개 API (홈 레지스트리) ───────────────────────────────────

pub fn load() -> Registry {
    registry_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// porpoise 프로젝트(`.porpoise` 존재)면 등록한다. 실패는 경고용 Err.
pub fn register(path: &Path) -> Result<(), String> {
    if !path.join(".porpoise").exists() {
        return Err(format!("porpoise 프로젝트가 아닙니다 (.porpoise 없음): {}", path.display()));
    }
    let Some(file) = registry_path() else {
        return Err("홈 디렉터리를 찾을 수 없습니다".to_string());
    };
    let mut reg = load_from(&file);
    if upsert(&mut reg, path) {
        save_to(&file, &reg).map_err(|e| format!("레지스트리 저장 실패: {}", e))?;
    }
    Ok(())
}

pub fn unregister(path: &Path) -> Result<(), String> {
    let Some(file) = registry_path() else {
        return Err("홈 디렉터리를 찾을 수 없습니다".to_string());
    };
    let mut reg = load_from(&file);
    let norm = normalize(path);
    let id = project_id(&norm);
    let before = reg.projects.len();
    reg.projects.retain(|p| p.id != id);
    if reg.projects.len() != before {
        save_to(&file, &reg).map_err(|e| format!("레지스트리 저장 실패: {}", e))?;
        Ok(())
    } else {
        Err(format!("등록되어 있지 않습니다: {}", path.display()))
    }
}

pub fn resolve(id: &str) -> Option<PathBuf> {
    resolve_in(&load(), id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_dir(base: &Path, name: &str) -> PathBuf {
        let p = base.join(name);
        std::fs::create_dir_all(p.join(".porpoise")).unwrap();
        p
    }

    #[test]
    fn project_id_is_stable_and_distinct() {
        let a = project_id("c:\\code\\a");
        assert_eq!(a, project_id("c:\\code\\a"), "동일 입력 → 동일 id");
        assert_ne!(a, project_id("c:\\code\\b"));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn upsert_adds_then_dedupes() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = project_dir(tmp.path(), "alpha");
        let mut reg = Registry::default();
        assert!(upsert(&mut reg, &proj), "신규 등록");
        assert!(!upsert(&mut reg, &proj), "동일 경로 재등록은 무변경");
        assert_eq!(reg.projects.len(), 1);
        assert_eq!(reg.projects[0].name, "alpha");
    }

    #[test]
    fn resolve_only_registered_and_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = project_dir(tmp.path(), "beta");
        let mut reg = Registry::default();
        upsert(&mut reg, &proj);
        let id = reg.projects[0].id.clone();

        assert!(resolve_in(&reg, &id).is_some(), "등록 + 실존 → Some");
        assert!(resolve_in(&reg, "deadbeefdeadbeef").is_none(), "미등록 id → None");

        // 경로가 사라지면 resolve 거부
        std::fs::remove_dir_all(proj.join(".porpoise")).unwrap();
        assert!(resolve_in(&reg, &id).is_none(), ".porpoise 소멸 → None");
    }

    #[test]
    fn load_save_roundtrip_and_corrupt() {
        let tmp = tempfile::tempdir().unwrap();
        let file = registry_path_in(tmp.path());

        let mut reg = Registry::default();
        let proj = project_dir(tmp.path(), "gamma");
        upsert(&mut reg, &proj);
        save_to(&file, &reg).unwrap();

        let loaded = load_from(&file);
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "gamma");

        // 손상 파일 → 빈 목록 (패닉 없음)
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(load_from(&file).projects.len(), 0);
        // 부재 파일 → 빈 목록
        assert_eq!(load_from(&tmp.path().join("nope.json")).projects.len(), 0);
    }
}

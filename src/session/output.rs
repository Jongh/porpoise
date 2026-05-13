use serde::{Deserialize, Serialize, Deserializer, Serializer};
use crate::session::planning::PlanningOutput;
use crate::session::development::DevelopmentOutput;
use crate::session::testing::TestingOutput;
use crate::session::review::ReviewOutput;
use crate::session::milestone::MilestoneOutput;
use crate::session::v0_7::{FileOperation, VerifyCommand};

// ExitCode: 기존 orchestrator/report.rs에서 이동 (re-export 유지)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExitCode {
    Next,
    Prev,
    Resp,
    Limit,
}

impl Default for ExitCode {
    fn default() -> Self {
        ExitCode::Resp
    }
}

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitCode::Next => write!(f, "NEXT"),
            ExitCode::Prev => write!(f, "PREV"),
            ExitCode::Resp => write!(f, "RESP"),
            ExitCode::Limit => write!(f, "LIMIT"),
        }
    }
}

/// role별로 직렬화/역직렬화를 수동 구현한다.
/// 각 Output 구조체의 `role` 필드 값으로 variant를 판별한다.
#[derive(Debug, Clone)]
pub enum RoleOutputData {
    Planning(PlanningOutput),
    Development(DevelopmentOutput),
    Testing(TestingOutput),
    Review(ReviewOutput),
    Milestone(MilestoneOutput),
}

impl Serialize for RoleOutputData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            RoleOutputData::Planning(o) => o.serialize(s),
            RoleOutputData::Development(o) => o.serialize(s),
            RoleOutputData::Testing(o) => o.serialize(s),
            RoleOutputData::Review(o) => o.serialize(s),
            RoleOutputData::Milestone(o) => o.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for RoleOutputData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // 먼저 raw Value로 파싱한 뒤 role 필드를 보고 variant를 선택
        let v = serde_json::Value::deserialize(d)?;
        let role = v.get("role").and_then(|r| r.as_str()).unwrap_or("");
        match role {
            "planning" => {
                serde_json::from_value::<PlanningOutput>(v)
                    .map(RoleOutputData::Planning)
                    .map_err(serde::de::Error::custom)
            }
            "development" => {
                serde_json::from_value::<DevelopmentOutput>(v)
                    .map(RoleOutputData::Development)
                    .map_err(serde::de::Error::custom)
            }
            "testing" => {
                serde_json::from_value::<TestingOutput>(v)
                    .map(RoleOutputData::Testing)
                    .map_err(serde::de::Error::custom)
            }
            "review" => {
                serde_json::from_value::<ReviewOutput>(v)
                    .map(RoleOutputData::Review)
                    .map_err(serde::de::Error::custom)
            }
            "milestone" => {
                serde_json::from_value::<MilestoneOutput>(v)
                    .map(RoleOutputData::Milestone)
                    .map_err(serde::de::Error::custom)
            }
            other => Err(serde::de::Error::custom(format!(
                "unknown role: '{}'", other
            ))),
        }
    }
}

#[allow(dead_code)]
impl RoleOutputData {
    pub fn status(&self) -> &ExitCode {
        match self {
            RoleOutputData::Planning(o) => &o.status,
            RoleOutputData::Development(o) => &o.status,
            RoleOutputData::Testing(o) => &o.status,
            RoleOutputData::Review(o) => &o.status,
            RoleOutputData::Milestone(o) => &o.status,
        }
    }
    pub fn summary(&self) -> &str {
        match self {
            RoleOutputData::Planning(o) => &o.summary,
            RoleOutputData::Development(o) => &o.summary,
            RoleOutputData::Testing(o) => &o.summary,
            RoleOutputData::Review(o) => &o.summary,
            RoleOutputData::Milestone(o) => &o.summary,
        }
    }
    pub fn questions(&self) -> &[String] {
        match self {
            RoleOutputData::Planning(o) => &o.questions,
            RoleOutputData::Development(o) => &o.questions,
            RoleOutputData::Testing(o) => &o.questions,
            RoleOutputData::Review(o) => &o.questions,
            RoleOutputData::Milestone(o) => &o.questions,
        }
    }
    pub fn prev_reason(&self) -> Option<&str> {
        match self {
            RoleOutputData::Planning(o) => o.prev_reason.as_deref(),
            RoleOutputData::Development(o) => o.prev_reason.as_deref(),
            RoleOutputData::Testing(o) => o.prev_reason.as_deref(),
            RoleOutputData::Review(o) => o.prev_reason.as_deref(),
            RoleOutputData::Milestone(_) => None,
        }
    }
    pub fn completed_tasks(&self) -> &[String] {
        match self {
            RoleOutputData::Review(o) => &o.completed_tasks,
            _ => &[],
        }
    }
    pub fn prev_target(&self) -> Option<&str> {
        match self {
            RoleOutputData::Review(o) => o.prev_target.as_deref(),
            _ => None,
        }
    }
    pub fn milestone_complete(&self) -> bool {
        match self {
            RoleOutputData::Review(o) => o.milestone_complete,
            _ => false,
        }
    }
    pub fn review_status(&self) -> Option<&str> {
        match self {
            RoleOutputData::Review(o) => Some(&o.review_status),
            _ => None,
        }
    }

    pub fn file_operations(&self) -> Option<&Vec<FileOperation>> {
        match self {
            RoleOutputData::Development(o) => o.file_operations.as_ref(),
            _ => None,
        }
    }

    pub fn verify_commands(&self) -> Option<&Vec<VerifyCommand>> {
        match self {
            RoleOutputData::Development(o) => o.verify_commands.as_ref(),
            _ => None,
        }
    }
}

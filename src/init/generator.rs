use anyhow::{Context, Result};
use chrono::Local;
use colored::Colorize;
use std::path::Path;

use super::context::ProjectContext;
use crate::utils::fs::write_file;

pub fn generate_docs(ctx: &ProjectContext, path: &Path) -> Result<()> {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Generate CLAUDE.md — minimal reference pointer to .porpoise/project.md
    let claude_md_path = path.join("CLAUDE.md");
    let claude_md_content = format!(
        r#"# {project_name}

프로젝트의 개발 루틴, 파일 구조, 오케스트레이션 규칙은 `.porpoise/project.md`를 참조하세요.
"#,
        project_name = ctx.project_name,
    );
    write_file(&claude_md_path, &claude_md_content, path)?;
    println!("  {} {}", "Created:".green(), claude_md_path.display());

    // Generate .porpoise/project.md — single source of truth for all project context
    let docs_dir = path.join(".porpoise");
    let project_md_path = docs_dir.join("project.md");
    let project_md_content = format!(
        r#"# 개발 루틴 문서

## 프로젝트: {project_name}
## 초기화: {timestamp}

## 파일 구조
{tree}

## Porpoise 오케스트레이션
이 프로젝트는 Porpoise 오케스트레이션 도구로 관리됩니다.

### 폴더 역할
| 폴더 | 쓰기 주체 | 내용 |
|------|----------|------|
| `.porpoise/reports/` | Claude (단독) | 포맷된 역할 보고서 |
| `.porpoise/messages/` | Porpoise (단독) | Claude 실행 전체 출력 |
| `.porpoise/hints/` | Porpoise (RESP 흐름) | 사용자 추가 지시사항 |
| `.porpoise/prompts/` | 초기화 시 생성 | 역할별 프롬프트 파일 |

## 역할별 책임
- Planning: 작업 범위 정의, 기술 명세 작성
- Development: 코드 구현
- Testing: 테스트 실행 및 버그 리포트
- Review: 코드 리뷰 및 품질 평가

## 완료 기준 (DoD)
- 코드 리뷰 통과
- 테스트 통과
- 문서화 완료

## 컨벤션
- 커밋 메시지: 한국어 허용
- 브랜치 전략: main 브랜치 직접 커밋 (소규모 프로젝트)
- 보고서 파일명: {{task-id}}-{{role}}-C{{cycle}}-R{{retry}}.md
"#,
        project_name = ctx.project_name,
        timestamp = timestamp,
        tree = ctx.tree_output,
    );
    write_file(&project_md_path, &project_md_content, path)?;
    println!("  {} {}", "Created:".green(), project_md_path.display());

    // Create hints directory (populated at runtime by RESP answers)
    let hints_dir = docs_dir.join("hints");
    std::fs::create_dir_all(&hints_dir)
        .with_context(|| format!("hints 디렉토리 생성 실패: {}", hints_dir.display()))?;

    // Create reports directory (Claude saves formatted role reports here)
    let reports_dir = docs_dir.join("reports");
    std::fs::create_dir_all(&reports_dir)
        .with_context(|| format!("reports 디렉토리 생성 실패: {}", reports_dir.display()))?;

    // Generate prompt files
    let prompts_dir = docs_dir.join("prompts");
    let prompts = [
        ("00-orche.md", generate_orche_prompt(ctx)),
        ("01-planning.md", generate_pm_prompt()),
        ("02-development.md", generate_developer_prompt()),
        ("03-testing.md", generate_tester_prompt()),
        ("04-review.md", generate_reviewer_prompt()),
    ];

    for (filename, content) in &prompts {
        let prompt_path = prompts_dir.join(filename);
        write_file(&prompt_path, content, path)?;
        println!("  {} {}", "Created:".green(), prompt_path.display());
    }

    Ok(())
}

fn generate_orche_prompt(ctx: &ProjectContext) -> String {
    format!(
        r#"# Porpoise 오케스트레이션 시스템 프롬프트

## 역할
당신은 Porpoise 오케스트레이션 시스템의 일부입니다. 소프트웨어 개발 사이클을 Planning → Development → Testing → Review 순서로 진행합니다.

## 프로젝트
프로젝트 상세 정보는 `.porpoise/project.md`를 참조하세요.
- 이름: {project_name}

## 폴더 구조 및 쓰기 권한

| 폴더 | 쓰기 주체 | 내용 |
|------|----------|------|
| `.porpoise/reports/` | **Claude (당신)** | 포맷된 역할 보고서 |
| `.porpoise/messages/` | Porpoise (시스템) | Claude 실행 전체 출력 |
| `.porpoise/hints/` | Porpoise (시스템) | 사용자 추가 지시사항 |
| `.porpoise/prompts/` | 초기화 시 생성 | 역할별 프롬프트 파일 |

**⚠ 중요**: `.porpoise/reports/`에만 보고서를 저장하세요. `.porpoise/messages/`에는 절대 직접 쓰지 않습니다.

## 보고서 저장 규칙
역할 수행 완료 후 보고서를 아래 경로에 저장합니다:
`.porpoise/reports/{{task-id}}-{{role}}-C{{cycle}}-R{{retry}}.md`
예: `.porpoise/reports/M1-T01-planning-C1-R0.md`

**기존 파일이 존재하면 절대 덮어쓰지 않습니다.** 파일이 이미 있으면 retry 번호를 증가시켜 새 파일로 저장합니다.

## 오케스트레이션 규칙
1. 각 역할은 독립적으로 실행됩니다.
2. 이전 역할의 보고서(`.porpoise/reports/`)를 컨텍스트로 참조합니다.
3. 사이클은 Review NEXT 코드 출력 후 완료됩니다.

## 종료 코드 규칙
보고서의 **마지막 줄**에 아래 코드 중 하나를 단독으로 출력합니다:
- `NEXT`: 현재 역할 완료, 다음 단계 진행
- `PREV`: 이전 역할 재작업 필요

## Hint 파일
`.porpoise/hints/` 폴더에는 이전 RESP 라운드에서 사용자가 제공한 답변이 저장됩니다.
파일명 패턴: `{{task-id}}-{{role}}-C{{cycle}}-R{{retry}}-hints.md`
각 역할은 실행 시 해당 hint 파일이 컨텍스트에 포함되면 그 내용을 최우선으로 반영합니다.
"#,
        project_name = ctx.project_name,
    )
}

fn hint_section() -> &'static str {
    r#"
---

## Hint 파일 참조

`.porpoise/hints/` 폴더의 hint 파일이 컨텍스트에 포함된 경우, 그 내용은 **사용자가 직접 제공한 추가 지시사항**입니다.

- hint 파일의 지시사항은 다른 어떤 컨텍스트보다 **우선순위가 높습니다**.
- hint 내용과 이전 명세가 충돌할 경우 hint를 따르세요.
- hint 내용을 그대로 반영했음을 리포트의 서두에 명시하세요.
"#
}

fn exit_code_section() -> &'static str {
    r#"
---

## 응답 종료 코드

응답의 **마지막 줄**에 아래 코드 중 하나를 **단독으로** 출력한다. 다른 텍스트가 뒤따르면 안 된다.

| 코드 | 조건 |
|------|------|
| `NEXT` | 현재 역할 완료, 다음 단계 진행 가능 |
| `PREV` | 이전 역할 재작업 필요 (Critical 버그, 명세 오류 등) |
"#
}

fn generate_pm_prompt() -> String {
    format!(
        r#"# PM (Product Manager) 역할 프롬프트

## 역할 정의
당신은 소프트웨어 프로젝트의 PM(Product Manager)입니다. 작업 범위를 정의하고, 기술 명세를 작성하며, 개발자가 구현할 수 있도록 상세한 요구사항을 제공합니다.

## 책임
1. **작업 범위 정의**: 이번 사이클에서 구현할 기능을 명확히 정의합니다.
2. **기술 명세 작성**: 개발자가 참고할 수 있는 상세한 기술 명세를 작성합니다.
3. **우선순위 설정**: 기능의 우선순위를 명확히 합니다.
4. **완료 기준 정의**: 각 기능의 완료 기준(Definition of Done)을 설정합니다.

## 출력 형식
리포트에 다음 섹션을 포함하세요:

```markdown
# PM 리포트 - 사이클 {{cycle}}

## 이번 사이클 작업 범위
...

## 기술 명세
...

## 우선순위 목록
1. ...
2. ...

## 완료 기준
- [ ] ...

## 개발자에게 전달 사항
...
```

## 중요 지침
- 마일스톤의 작업 항목은 **위에서부터 순서대로 하나씩** 처리하세요. 아직 완료되지 않은 첫 번째 항목만 이번 사이클에서 다룹니다.
- 명세는 구체적이고 측정 가능해야 합니다.
- 모호한 요구사항은 명확히 해야 합니다.
- 기술적 부채를 최소화하는 방향으로 설계하세요.
- 구현 불가능한 치명적 문제 발견 시 PREV를 사용하세요.
{hint}{exit_code}"#,
        hint = hint_section(),
        exit_code = exit_code_section()
    )
}

fn generate_developer_prompt() -> String {
    format!(
        r#"# Developer 역할 프롬프트

## 역할 정의
당신은 소프트웨어 프로젝트의 Developer입니다. PM의 명세를 바탕으로 코드를 구현하고, 단위 테스트를 작성하며, 코드 품질을 유지합니다.

## 책임
1. **코드 구현**: PM 명세에 따라 기능을 구현합니다.
2. **단위 테스트 작성**: 구현한 코드에 대한 단위 테스트를 작성합니다.
3. **코드 품질 유지**: 코딩 컨벤션을 준수하고 가독성 높은 코드를 작성합니다.
4. **문서화**: 코드에 적절한 주석과 문서를 추가합니다.

## 출력 형식
리포트에 다음 섹션을 포함하세요:

```markdown
# Developer 리포트 - 사이클 {{cycle}}

## 구현 완료 항목
- [x] ...

## 구현 미완료 항목
- [ ] ...

## 주요 변경사항
...

## 테스트 결과
...

## 알려진 이슈
...

## 테스터에게 전달 사항
...
```

## 중요 지침
- PM 명세를 충실히 따르세요.
- 테스트 가능한 코드를 작성하세요.
- PM 명세에 구현 불가능한 오류가 있으면 PREV를 사용하세요.
- unwrap() 신규 추가 금지.
{hint}{exit_code}"#,
        hint = hint_section(),
        exit_code = exit_code_section()
    )
}

fn generate_tester_prompt() -> String {
    format!(
        r#"# Tester 역할 프롬프트

## 역할 정의
당신은 소프트웨어 프로젝트의 Tester입니다. Developer가 구현한 코드를 테스트하고, 버그를 발견하며, 품질을 검증합니다.

## 책임
1. **기능 테스트**: 구현된 기능이 명세에 맞게 동작하는지 확인합니다.
2. **버그 리포트**: 발견된 버그를 상세히 문서화합니다.
3. **회귀 테스트**: 기존 기능이 새 변경으로 인해 망가지지 않았는지 확인합니다.
4. **성능 테스트**: 필요한 경우 성능 측정을 수행합니다.

## 출력 형식
리포트에 다음 섹션을 포함하세요:

```markdown
# Tester 리포트 - 사이클 {{cycle}}

## 테스트 수행 항목
- [x] ...

## 발견된 버그
### Critical 버그 (PREV 필요)
...

### Minor 버그
...

## 테스트 통과 항목
...

## 테스트 실패 항목
...

## 리뷰어에게 전달 사항
...
```

## 중요 지침
- 모든 PM 요구사항을 커버하는 테스트를 수행하세요.
- **Developer 리포트에서 '완료'로 표시된 항목을 그대로 신뢰하지 말 것.** PM 명세를 기준으로 독립적으로 재검증하세요.
- Developer 리포트의 판단과 별개로, 각 기능이 실제로 명세를 충족하는지 직접 확인하세요.
- Critical 버그(수정 없이 릴리즈 불가) 발견 시 반드시 PREV를 사용하세요.
- Minor 버그만 있으면 NEXT를 사용하세요.
- 엣지 케이스를 반드시 테스트하세요.
{hint}{exit_code}"#,
        hint = hint_section(),
        exit_code = exit_code_section()
    )
}

fn generate_reviewer_prompt() -> String {
    format!(
        r#"# Reviewer 역할 프롬프트

## 역할 정의
당신은 소프트웨어 프로젝트의 Reviewer입니다. 코드 품질, 아키텍처, 보안, 성능을 종합적으로 평가하고 최종 승인 여부를 결정합니다.

## 책임
1. **코드 리뷰**: 코드 품질, 가독성, 유지보수성을 평가합니다.
2. **아키텍처 검토**: 설계 결정의 적절성을 평가합니다.
3. **보안 검토**: 보안 취약점을 식별합니다.
4. **최종 승인**: 전체 사이클의 완료 여부를 결정합니다.

## 출력 형식
리포트에 다음 섹션을 포함하세요:

```markdown
# Reviewer 리포트 - 사이클 {{cycle}}

## 리뷰 결과
**상태**: APPROVED / CHANGES_REQUESTED / REJECTED

## 코드 품질 평가
...

## 아키텍처 평가
...

## 보안 평가
...

## 개선 필요 항목
- [ ] ...

## 승인 조건 (CHANGES_REQUESTED인 경우)
...

## 다음 사이클 권고사항
...
```

## 중요 지침
- 객관적이고 건설적인 피드백을 제공하세요.
- APPROVED → NEXT 출력: 자동 커밋 및 다음 작업으로 진행됩니다.
- CHANGES_REQUESTED → PREV 출력: Developer 또는 Tester로 재작업 라우팅됩니다.
- REJECTED (근본적 재설계 필요) → PREV 출력: PM으로 재라우팅됩니다.
- 머지 블로커가 있으면 반드시 PREV를 사용하세요.
- hint 파일이 있으면 해당 내용을 리뷰 기준에 반영하세요.
- 릴리즈 태스크(빌드 버전 업데이트 등 마일스톤 최종 태스크)를 리뷰하는 경우, `README.md`의
  `## CHANGELOG` 섹션에 새 버전 항목을 추가하세요.
  형식: `### [vX.Y.Z]\n- 변경사항` (기존 항목 형식 유지).
  완료된 마일스톤 태스크 목록을 기반으로 항목을 서술합니다.

## 메타데이터 블록 (선택)
추가 메타데이터가 필요한 경우:

```
<!-- PORPOISE_META
status: APPROVED
milestone_complete: false
prev_target: development
-->
```

- `prev_target`: PREV 출력 시 복귀할 역할. 생략하면 Planning부터 재시작.
  - 허용값: `development`, `testing` (PM은 기본값이므로 생략)
  - 예: Reviewer가 코드 품질 문제만 발견한 경우 `prev_target: development`
  - 예: Reviewer가 테스트 커버리지 문제만 발견한 경우 `prev_target: testing`
{hint}{exit_code}"#,
        hint = hint_section(),
        exit_code = exit_code_section()
    )
}

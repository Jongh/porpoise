use anyhow::Result;
use chrono::Local;
use colored::Colorize;
use std::path::Path;

use super::context::ProjectContext;
use crate::utils::fs::write_file;

pub fn generate_docs(ctx: &ProjectContext, path: &Path) -> Result<()> {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Generate claude.md
    let claude_md_path = path.join("claude.md");
    let claude_md_content = format!(
        r#"# {project_name}

## 프로젝트 개요
{description}

## 현재 상태
- 초기화됨: {timestamp}
- 현재 사이클: 0
- 다음 단계: PM 역할 실행 대기

## 파일 구조
{tree}

## Porpoise 오케스트레이션
이 프로젝트는 Porpoise 오케스트레이션 도구로 관리됩니다.
- 리포트 위치: .docs/reports/
- 프롬프트 위치: .docs/prompts/
"#,
        project_name = ctx.project_name,
        description = ctx.description,
        timestamp = timestamp,
        tree = ctx.tree_output,
    );
    write_file(&claude_md_path, &claude_md_content, path)?;
    println!("  {} {}", "Created:".green(), claude_md_path.display());

    // Generate .docs/project.md
    let docs_dir = path.join(".docs");
    let project_md_path = docs_dir.join("project.md");
    let project_md_content = format!(
        r#"# 개발 루틴 문서

## 프로젝트: {project_name}
## 초기화: {timestamp}

## 마일스톤
- [ ] 마일스톤 1: 초기 구현

## 역할별 책임
- PM: 작업 범위 정의, 기술 명세 작성
- Developer: 코드 구현
- Tester: 테스트 실행 및 버그 리포트
- Reviewer: 코드 리뷰 및 품질 평가

## 완료 기준 (DoD)
- 코드 리뷰 통과
- 테스트 통과
- 문서화 완료

## 컨벤션
- 커밋 메시지: 한국어 허용
- 브랜치 전략: main 브랜치 직접 커밋 (소규모 프로젝트)
"#,
        project_name = ctx.project_name,
        timestamp = timestamp,
    );
    write_file(&project_md_path, &project_md_content, path)?;
    println!("  {} {}", "Created:".green(), project_md_path.display());

    // Generate prompt files
    let prompts_dir = docs_dir.join("prompts");
    let prompts = [
        ("00-orche.md", generate_orche_prompt(ctx)),
        ("01-pm.md", generate_pm_prompt()),
        ("02-developer.md", generate_developer_prompt()),
        ("03-tester.md", generate_tester_prompt()),
        ("04-reviewer.md", generate_reviewer_prompt()),
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
당신은 Porpoise 오케스트레이션 시스템의 일부입니다. 소프트웨어 개발 사이클을 PM → Developer → Tester → Reviewer 순서로 진행합니다.

## 프로젝트
- 이름: {project_name}
- 설명: {description}

## 오케스트레이션 규칙
1. 각 역할은 독립적으로 실행됩니다.
2. 각 역할의 결과는 `.docs/reports/` 에 저장됩니다.
3. 다음 역할은 이전 역할의 리포트를 참고합니다.
4. 사이클은 Reviewer 승인 후 완료됩니다.

## 리포트 형식
각 리포트는 마크다운 형식으로 작성되며, 다음 섹션을 포함해야 합니다:
- 역할 및 사이클 번호
- 수행한 작업
- 결과 및 산출물
- 다음 단계 권고사항
- 특이사항 (버그, 이슈 등)

## 사용자 개입 조건
다음 상황에서는 `USER_INPUT_REQUIRED` 마커를 포함하세요:
- 중요한 설계 결정이 필요한 경우
- 명세가 불명확한 경우
- 외부 리소스가 필요한 경우
"#,
        project_name = ctx.project_name,
        description = ctx.description,
    )
}

fn generate_pm_prompt() -> String {
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
# PM 리포트 - 사이클 {cycle}

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
- 명세는 구체적이고 측정 가능해야 합니다.
- 모호한 요구사항은 명확히 해야 합니다.
- 기술적 부채를 최소화하는 방향으로 설계하세요.
- 사용자 개입이 필요한 경우 `USER_INPUT_REQUIRED` 마커를 사용하세요.
"#.to_string()
}

fn generate_developer_prompt() -> String {
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
# Developer 리포트 - 사이클 {cycle}

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
- 중요한 버그 발견 시 `Critical` 마커를 사용하세요.
- 사용자 개입이 필요한 경우 `USER_INPUT_REQUIRED` 마커를 사용하세요.
"#.to_string()
}

fn generate_tester_prompt() -> String {
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
# Tester 리포트 - 사이클 {cycle}

## 테스트 수행 항목
- [x] ...

## 발견된 버그
### Critical 버그
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
- 치명적인 버그는 `Critical` 마커를 사용하세요.
- 엣지 케이스를 반드시 테스트하세요.
- 사용자 개입이 필요한 경우 `USER_INPUT_REQUIRED` 마커를 사용하세요.
"#.to_string()
}

fn generate_reviewer_prompt() -> String {
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
# Reviewer 리포트 - 사이클 {cycle}

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

## 마일스톤 완료 여부
(마일스톤이 완료된 경우 "마일스톤 완료" 라고 명시)

## 다음 사이클 권고사항
...
```

## 중요 지침
- 객관적이고 건설적인 피드백을 제공하세요.
- APPROVED: 모든 기준을 충족한 경우
- CHANGES_REQUESTED: 수정 후 재검토가 필요한 경우
- REJECTED: 근본적인 재설계가 필요한 경우
- 마일스톤 완료 시 "마일스톤 완료"를 명시하세요.
"#.to_string()
}

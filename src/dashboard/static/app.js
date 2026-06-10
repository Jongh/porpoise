// Porpoise 대시보드 앱 (M30) — read-only. API 폴링 + 렌더링.
(function () {
  const $ = (s) => document.querySelector(s);
  const sel = $("#milestone");
  const projSel = $("#project");

  // M32: 선택된 프로젝트 id ("" = 서버 기동 디렉터리, 하위호환)
  let currentProject = "";

  // URL에 프로젝트 스코프를 부여한다.
  function withProject(url) {
    if (!currentProject) return url;
    return url + (url.includes("?") ? "&" : "?") + "project=" + encodeURIComponent(currentProject);
  }

  async function getJSON(url) {
    const r = await fetch(withProject(url));
    if (!r.ok) throw new Error(url + " → " + r.status);
    return r.json();
  }

  function card(label, value) {
    return `<div class="card"><div class="label">${label}</div><div class="value">${value}</div></div>`;
  }

  function money(v) {
    return v == null ? "-" : "$" + Number(v).toFixed(4);
  }

  // 파일 유래 문자열을 innerHTML에 넣기 전 이스케이프 (방어적 — self-XSS 방지)
  function esc(s) {
    return String(s == null ? "" : s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
  }

  // M32: 프로젝트 목록 — 2개 이상일 때만 셀렉터 노출 (1개면 기존 화면과 동일)
  async function loadProjects() {
    const r = await fetch("/api/projects"); // 스코프 무관 엔드포인트
    if (!r.ok) return;
    const data = await r.json();
    const projects = data.projects || [];
    if (projects.length < 2) return;
    projSel.innerHTML = "";
    projects.forEach((p) => {
      const o = document.createElement("option");
      o.value = p.id;
      o.textContent = p.name;
      o.title = p.path;
      if (p.current) o.selected = true;
      projSel.appendChild(o);
    });
    $("#project-wrap").classList.remove("hidden");
  }

  async function loadMilestones() {
    const data = await getJSON("/api/milestones");
    sel.innerHTML = "";
    (data.milestones || []).forEach((m) => {
      const o = document.createElement("option");
      o.value = m.number;
      o.textContent = "M" + m.number + (m.title ? " — " + m.title : "");
      sel.appendChild(o);
    });
  }

  async function loadReport(milestone) {
    const q = milestone ? "?milestone=" + milestone : "";
    const rep = await getJSON("/api/report" + q);

    // 롤업 카드
    $("#rollup").innerHTML = [
      card("태스크", rep.total),
      card("성공률", (rep.success_rate != null ? rep.success_rate.toFixed(1) : "0") + "%"),
      card("PASS / FAIL", rep.passed + " / " + rep.failed),
      card("재투입", rep.total_redispatches),
      card("폴백", rep.fallback_count),
      card("총비용", money(rep.total_cost)),
    ].join("");

    // 표
    const tbody = $("#report-table tbody");
    tbody.innerHTML = "";
    const tasks = rep.tasks || [];
    if (tasks.length === 0) {
      $("#report-empty").classList.remove("hidden");
      $("#report-table").classList.add("hidden");
    } else {
      $("#report-empty").classList.add("hidden");
      $("#report-table").classList.remove("hidden");
      tasks.forEach((t) => {
        const verdict = t.final_verdict ? "PASS" : "FAIL";
        const tr = document.createElement("tr");
        tr.className = "report-row";
        tr.title = "클릭하면 작업 내용 상세를 펼칩니다";
        tr.innerHTML =
          `<td class="mono">${esc(t.task_id)}</td>` +
          `<td><span class="tag ${verdict}">${verdict}</span></td>` +
          `<td>${t.attempts}</td><td>${t.max_redispatch}</td>` +
          `<td>${t.fallback_used ? "예" : ""}</td>` +
          `<td>${money(t.cost_usd)}</td>`;
        // M36: 행 클릭 → 작업 내용 상세 펼침/접기
        tr.addEventListener("click", () => toggleDetail(tr, t.task_id));
        tbody.appendChild(tr);
      });
    }

    // 비용 차트
    const costItems = tasks
      .filter((t) => t.cost_usd != null)
      .map((t) => ({ label: t.task_id, value: t.cost_usd, color: t.final_verdict ? "#3fb950" : "#f85149" }));
    Chart.bars($("#cost-chart"), costItems, { format: (v) => "$" + v.toFixed(4) });
  }

  // ── M36: 리포트 행 펼침 — 작업 내용 상세 ─────────────────────────────
  async function toggleDetail(row, taskId) {
    // 이미 펼쳐져 있으면 접기
    const next = row.nextElementSibling;
    if (next && next.classList.contains("detail-row")) {
      next.remove();
      return;
    }
    // 다른 펼침은 닫기 (한 번에 하나)
    document.querySelectorAll(".detail-row").forEach((el) => el.remove());

    const tr = document.createElement("tr");
    tr.className = "detail-row";
    const td = document.createElement("td");
    td.colSpan = 6;
    td.innerHTML = '<div class="muted">불러오는 중...</div>';
    tr.appendChild(td);
    row.after(tr);

    try {
      const data = await getJSON("/api/task?id=" + encodeURIComponent(taskId));
      const rounds = data.rounds || [];
      if (rounds.length === 0) {
        td.innerHTML = '<div class="muted">상세 기록 없음</div>';
        return;
      }
      td.innerHTML = rounds
        .map((r) => {
          const v = r.verdict === "PASS" ? "PASS" : "FAIL";
          const head =
            `<div class="round-head"><span class="tag ${v}">${v}</span>` +
            `<span class="muted"> R${r.redispatch} · diff ${r.diff_lines}줄` +
            (r.cost_usd != null ? " · " + money(r.cost_usd) : "") +
            (r.fallback_used ? " · 폴백" : "") +
            `</span></div>`;
          const feedback = r.feedback
            ? `<div class="detail-block fail-block"><b>검증 피드백</b><pre>${esc(r.feedback)}</pre></div>`
            : "";
          const output = r.dispatch_output
            ? `<div class="detail-block"><b>에이전트 작업 보고</b><pre>${esc(r.dispatch_output)}</pre></div>`
            : '<div class="muted">에이전트 보고 없음</div>';
          return `<div class="round">${head}${feedback}${output}</div>`;
        })
        .join("");
    } catch (e) {
      td.innerHTML = '<div class="gate-error">상세 조회 실패: ' + esc(String(e)) + "</div>";
    }
  }

  // 의존성 그래프 — 의존 깊이로 열 배치, SVG 노드+엣지
  function renderDepGraph(tasks) {
    const box = $("#dep-graph");
    box.innerHTML = "";
    if (!tasks || tasks.length === 0) {
      box.innerHTML = '<p class="muted">태스크 없음</p>';
      return;
    }
    const ids = new Set(tasks.map((t) => t.id));
    const byId = {};
    tasks.forEach((t) => (byId[t.id] = t));

    // 깊이 계산(dangling 의존은 무시)
    const depth = {};
    function d(id, seen) {
      if (depth[id] != null) return depth[id];
      if (seen.has(id)) return 0;
      seen.add(id);
      const deps = (byId[id].dependencies || []).filter((x) => ids.has(x));
      const v = deps.length === 0 ? 0 : 1 + Math.max(...deps.map((x) => d(x, seen)));
      depth[id] = v;
      return v;
    }
    tasks.forEach((t) => d(t.id, new Set()));

    const cols = {};
    tasks.forEach((t) => {
      const c = depth[t.id];
      (cols[c] = cols[c] || []).push(t.id);
    });
    const colKeys = Object.keys(cols).map(Number).sort((a, b) => a - b);

    const colW = 200, rowH = 56, nodeW = 150, nodeH = 38, padX = 20, padY = 16;
    const maxRows = Math.max(...colKeys.map((c) => cols[c].length));
    const W = padX * 2 + colKeys.length * colW;
    const H = padY * 2 + maxRows * rowH;
    const pos = {};
    colKeys.forEach((c, ci) => {
      cols[c].forEach((id, ri) => {
        pos[id] = { x: padX + ci * colW, y: padY + ri * rowH };
      });
    });

    const svg = Chart.el("svg", { width: W, height: H, viewBox: `0 0 ${W} ${H}` });
    // 엣지
    tasks.forEach((t) => {
      (t.dependencies || []).filter((x) => ids.has(x)).forEach((dep) => {
        const a = pos[dep], b = pos[t.id];
        if (!a || !b) return;
        svg.appendChild(
          Chart.el("line", {
            x1: a.x + nodeW, y1: a.y + nodeH / 2, x2: b.x, y2: b.y + nodeH / 2,
            stroke: "#3a4350", "stroke-width": 1.5,
          })
        );
      });
    });
    // 노드
    const color = { done: "#3fb950", ready: "#4aa8ff", waiting: "#8a93a3" };
    tasks.forEach((t) => {
      const p = pos[t.id];
      svg.appendChild(
        Chart.el("rect", {
          x: p.x, y: p.y, width: nodeW, height: nodeH, rx: 8,
          fill: "#1f2630", stroke: color[t.status] || "#8a93a3", "stroke-width": 2,
        })
      );
      const dot = Chart.el("circle", { cx: p.x + 14, cy: p.y + nodeH / 2, r: 5, fill: color[t.status] || "#8a93a3" });
      svg.appendChild(dot);
      const tx = Chart.el("text", { x: p.x + 26, y: p.y + nodeH / 2 + 4, fill: "#e6e9ef", "font-size": "12" });
      tx.textContent = t.id;
      svg.appendChild(tx);
    });
    box.appendChild(svg);
  }

  async function loadTasks() {
    const data = await getJSON("/api/tasks");
    renderDepGraph(data.tasks || []);
  }

  async function refresh() {
    try {
      const milestone = sel.value;
      await Promise.all([loadReport(milestone), loadTasks()]);
    } catch (e) {
      console.error(e);
    }
  }

  // ── M31: 라이브 패널 ──────────────────────────────────────────────
  const PHASES = ["brief", "dispatch", "verify", "integrate"];
  let wasActive = false;

  function phaseSteps(task) {
    if (task.phase === "merged")
      return '<span class="phase-step final-merged">MERGED</span>';
    if (task.phase === "halted")
      return '<span class="phase-step final-halted">HALTED</span>';
    return PHASES.map(
      (p) => `<span class="phase-step${p === task.phase ? " active" : ""}">${p}</span>`
    ).join("");
  }

  function renderLive(payload) {
    const live = (payload && payload.live) || { run_active: false };
    const badge = $("#live-badge");
    const body = $("#live-body");
    const budgetBox = $("#live-budget");

    if (live.run_active) {
      badge.textContent = "RUNNING · " + esc(live.mode || "");
      badge.className = "live-badge running";
    } else if (live.pending_gate) {
      // M36: 재실행 게이트 등 — 실행 전 승인 대기 상태
      badge.textContent = "WAITING";
      badge.className = "live-badge running";
    } else {
      badge.textContent = "IDLE";
      badge.className = "live-badge idle";
    }

    const tasks = live.tasks || [];
    if (tasks.length === 0) {
      body.className = "muted";
      body.textContent = live.run_active ? "함대 준비 중..." : "실행 중인 함대 없음";
    } else {
      body.className = "";
      const head = live.run_active ? "" : '<div class="muted" style="margin-bottom:6px">마지막 실행 요약</div>';
      body.innerHTML =
        head +
        tasks
          .map(
            (t) =>
              `<div class="live-task"><span class="tid">${esc(t.task_id)}</span>` +
              `<span class="phase-steps">${phaseSteps(t)}</span>` +
              (t.redispatch > 0 ? `<span class="muted">재투입 ${t.redispatch}</span>` : "") +
              // M36: 작업 제목 — 무슨 작업인지 표시
              (t.title ? `<span class="task-title" title="${esc(t.title)}">${esc(t.title)}</span>` : "") +
              `</div>`
          )
          .join("");
    }

    // 비용/예산
    const cost = live.total_cost_usd || 0;
    if (live.budget_usd) {
      budgetBox.classList.remove("hidden");
      const pct = Math.min(100, (cost / live.budget_usd) * 100);
      const fill = $("#budget-fill");
      fill.style.width = pct + "%";
      fill.className = cost >= live.budget_usd ? "over" : "";
      $("#budget-label").textContent =
        "비용 " + money(cost) + " / 예산 " + money(live.budget_usd) + " (" + pct.toFixed(0) + "%)";
    } else if (cost > 0) {
      budgetBox.classList.remove("hidden");
      $("#budget-fill").style.width = "0%";
      $("#budget-label").textContent = "누적 비용 " + money(cost);
    } else {
      budgetBox.classList.add("hidden");
    }

    // M33: 승인 대기 게이트 카드 (M34: kind별 폼)
    // M36: run_active와 무관하게 표시 — 재실행 게이트(orchestrator 초입)는 conductor 루프
    // 밖에서 발동해 live가 idle 상태일 수 있다. pending_gate는 게이트 종료 시 항상 해제됨.
    const gate = live.pending_gate;
    const card = $("#gate-card");
    const textInput = $("#gate-text");
    if (gate) {
      const isNewGate = currentGateId !== gate.id;
      currentGateId = gate.id;
      currentGateKind = gate.kind || "confirm";
      $("#gate-prompt").textContent = gate.prompt;
      // text 계열 게이트는 입력 폼 표시, 승인 버튼 라벨을 "전송"으로
      const wantsText = currentGateKind === "text" || currentGateKind === "confirm_text";
      textInput.classList.toggle("hidden", !wantsText);
      $("#gate-approve").textContent = currentGateKind === "text" ? "전송" : "승인";
      if (isNewGate) textInput.value = "";
      card.classList.remove("hidden");
    } else {
      currentGateId = null;
      currentGateKind = "confirm";
      card.classList.add("hidden");
      $("#gate-error").classList.add("hidden");
    }
    // 실행 중 상시 사전 정지 버튼 (M34: 정지 예약 가시화 — 서버 진실 stop_pending)
    const stopBtn = $("#stop-next");
    stopBtn.classList.toggle("hidden", !live.run_active);
    const armed = !!(payload && payload.stop_pending);
    stopBtn.classList.toggle("armed", armed);
    stopBtn.disabled = armed;
    stopBtn.textContent = armed ? "⏹ 정지 예약됨" : "다음 게이트에서 정지";

    // 실행 종료 전환(RUNNING→IDLE) 시 리포트·DAG 자동 새로고침
    if (wasActive && !live.run_active) refresh();
    wasActive = !!live.run_active;
  }

  // ── M33: 제어 전송 ─────────────────────────────────────────────────
  let currentGateId = null;
  let currentGateKind = "confirm"; // M34: text 게이트면 입력값 동봉

  async function sendControl(payload) {
    const err = $("#gate-error");
    err.classList.add("hidden");
    try {
      const r = await fetch(withProject("/api/control"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      if (!r.ok) {
        const data = await r.json().catch(() => ({}));
        err.textContent = "전송 실패 (" + r.status + ") " + (data.error || "");
        err.classList.remove("hidden");
      }
      // 성공 시 별도 처리 없음 — conductor가 소비하면 live 이벤트로 카드가 자연 소멸
    } catch (e) {
      err.textContent = "전송 실패: " + e;
      err.classList.remove("hidden");
    }
  }

  let pollTimer = null;
  function startLivePolling() {
    if (pollTimer) return;
    const poll = async () => {
      try { renderLive(await getJSON("/api/live")); } catch (e) { /* 서버 종료 등 — 무시 */ }
    };
    poll();
    pollTimer = setInterval(poll, 2000);
  }

  let es = null;
  function startLive() {
    // M32: 프로젝트 전환 시 기존 스트림을 닫고 새 스코프로 재구독
    if (es) {
      es.close();
      es = null;
    }
    try {
      es = new EventSource(withProject("/api/events"));
      es.addEventListener("live", (ev) => {
        try { renderLive(JSON.parse(ev.data)); } catch (e) { console.error(e); }
      });
      es.onerror = () => {
        // SSE 실패 → 폴링 폴백 (연결은 EventSource가 자동 재시도하므로 병행 무해)
        startLivePolling();
      };
    } catch (e) {
      startLivePolling(); // EventSource 미지원 환경
    }
  }

  // M32: 프로젝트 전환 — 마일스톤 목록부터 라이브 스트림까지 전부 새 스코프로
  async function switchProject() {
    currentProject = projSel.value;
    wasActive = false;
    try {
      await loadMilestones();
    } catch (e) {
      console.error(e);
    }
    await refresh();
    startLive();
  }

  async function init() {
    try {
      await loadProjects();
      await loadMilestones();
    } catch (e) {
      console.error(e);
    }
    await refresh();
    startLive();
    sel.addEventListener("change", refresh);
    projSel.addEventListener("change", switchProject);
    $("#refresh").addEventListener("click", refresh);
    // M33: 게이트 제어 버튼 (M34: text 게이트면 입력값 동봉)
    $("#gate-approve").addEventListener("click", () => {
      if (!currentGateId) return;
      const payload = { gate_id: currentGateId, decision: "approve" };
      if (currentGateKind === "text" || currentGateKind === "confirm_text") {
        payload.text = $("#gate-text").value;
      }
      sendControl(payload);
    });
    $("#gate-stop").addEventListener("click", () => {
      if (currentGateId) sendControl({ gate_id: currentGateId, decision: "stop" });
    });
    $("#stop-next").addEventListener("click", () => sendControl({ decision: "stop" }));
    // M34: 텍스트 게이트에서 Enter = 전송 (콘솔 Input의 자연스러운 대체)
    $("#gate-text").addEventListener("keydown", (e) => {
      if (e.key === "Enter") $("#gate-approve").click();
    });
  }

  init();
})();

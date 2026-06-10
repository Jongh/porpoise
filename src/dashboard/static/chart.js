// Porpoise 대시보드 경량 차트 lib (M30) — 의존성·CDN 0, SVG 막대 차트.
(function (global) {
  const NS = "http://www.w3.org/2000/svg";
  function el(name, attrs) {
    const e = document.createElementNS(NS, name);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    return e;
  }

  // bars(container, items): items = [{label, value, color?}]
  function bars(container, items, opts) {
    opts = opts || {};
    const fmt = opts.format || ((v) => v);
    container.innerHTML = "";
    if (!items || items.length === 0) {
      container.innerHTML = '<p class="muted">데이터 없음</p>';
      return;
    }
    const W = Math.max(320, container.clientWidth || 600);
    const rowH = 30, padL = 110, padR = 70, top = 8;
    const H = top * 2 + items.length * rowH;
    const max = Math.max(...items.map((d) => d.value), 0.0000001);
    const barW = W - padL - padR;
    const svg = el("svg", { width: W, height: H, viewBox: `0 0 ${W} ${H}` });

    items.forEach((d, i) => {
      const y = top + i * rowH;
      const w = Math.max(2, (d.value / max) * barW);
      // label
      const lab = el("text", { x: padL - 8, y: y + 19, "text-anchor": "end", class: "c-label" });
      lab.textContent = d.label;
      lab.setAttribute("fill", "#8a93a3");
      lab.setAttribute("font-size", "12");
      svg.appendChild(lab);
      // bar
      svg.appendChild(
        el("rect", { x: padL, y: y + 6, width: w, height: rowH - 14, rx: 4, fill: d.color || "#4aa8ff" })
      );
      // value
      const val = el("text", { x: padL + w + 6, y: y + 19, class: "c-value" });
      val.textContent = fmt(d.value);
      val.setAttribute("fill", "#e6e9ef");
      val.setAttribute("font-size", "12");
      svg.appendChild(val);
    });
    container.appendChild(svg);
  }

  global.Chart = { bars: bars, el: el, NS: NS };
})(window);

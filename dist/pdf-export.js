/* Searchable, vector print/PDF export. No PDF runtime, upload or screenshot. */
(() => {
  "use strict";
  const COLORS = { physics: "#20749f", society: "#287e57", engineering: "#a45d14" };
  const SCALE_X = 1.18, SCALE_Y = 1.42, W = 320, H = 112, HEADER = 116, PAD = 28;
  const SVG = "http://www.w3.org/2000/svg";
  const plain = value => String(value ?? "").replace(/§./gu, "").replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/gu, "");

  function plan(items, bands = []) {
    if (!items.length) throw new Error("No technologies to export");
    if (items.length > 10000) throw new Error("Export supports at most 10,000 technologies");
    const nodes = items.map(({ tech, x, y }) => {
      if (!tech?.id || !Number.isFinite(x) || !Number.isFinite(y) || Math.abs(x) > 1e7 || Math.abs(y) > 1e7) {
        throw new Error("Invalid export coordinates");
      }
      return { tech, x: x * SCALE_X + PAD, y: y * SCALE_Y + HEADER };
    });
    const width = Math.max(1100, ...nodes.map(n => n.x + W + PAD));
    const height = Math.max(300, ...nodes.map(n => n.y + H + PAD));
    // Chrome's maximum PDF page dimension is bounded. Preserve aspect ratio;
    // vector text remains zoomable even for unusually large source catalogs.
    const mmPerUnit = Math.min(420 / width, 4700 / height);
    return { nodes, bands, width, height, pageWidth: width * mmPerUnit, pageHeight: height * mmPerUnit };
  }

  function element(doc, name, attrs = {}, text) {
    const node = doc.createElementNS(SVG, name);
    for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
    if (text !== undefined) node.textContent = plain(text);
    return node;
  }

  function makeSvg(doc, graph, options = {}) {
    const layout = plan(graph.items, graph.bands);
    const svg = element(doc, "svg", { xmlns: SVG, viewBox: `0 0 ${layout.width} ${layout.height}`,
      width: layout.width, height: layout.height, role: "img", "aria-label": "Searchable bilingual technology tree" });
    svg.style.fontFamily = '"Segoe UI", "Microsoft YaHei UI", "PingFang SC", "Noto Sans CJK SC", sans-serif';
    svg.append(element(doc, "rect", { width: layout.width, height: layout.height, fill: "#fff" }));
    const text = (parent, value, x, y, size = 13, fill = "#182d36", extra = {}) => {
      const node = element(doc, "text", { x, y, "font-size": size, fill, ...extra }, value);
      parent.append(node); return node;
    };
    text(svg, "Stellaris Tech Tree / 群星科技树", PAD, 30, 24, "#143c4f", { "font-weight": 700 });
    text(svg, options.subtitle || "", PAD, 53, 12);
    text(svg, `${layout.nodes.length} technologies · 科技名称 / English names / IDs · Ctrl+F`, PAD, 75, 12);
    text(svg, "Filtered source reference, not save progress. OR prerequisites are alternatives. / 按当前筛选导出；可选前置不需全部研究。", PAD, 96, 11, "#50656f");
    const defs = element(doc, "defs"); svg.append(defs);
    const arrow = element(doc, "marker", { id: "tte-pdf-arrow", viewBox: "0 0 8 8", refX: 7, refY: 4, markerWidth: 5, markerHeight: 5, orient: "auto" });
    arrow.append(element(doc, "path", { d: "M0,0 L8,4 L0,8 Z", fill: "#78909b" })); defs.append(arrow);
    const atlas = options.atlas;
    if (atlas?.data && Number.isInteger(atlas.columns) && atlas.columns > 0 && atlas.tileSize > 0) {
      defs.append(element(doc, "image", { id: "tte-pdf-atlas", href: atlas.data, width: atlas.width, height: atlas.height }));
    }
    for (const band of graph.bands || []) {
      if (!Number.isFinite(band.y)) continue;
      const y = band.y * SCALE_Y + HEADER;
      svg.append(element(doc, "rect", { x: PAD, y, width: layout.width - 2 * PAD, height: band.heading ? 35 : 28,
        fill: band.heading ? "#e3edf2" : "#f1f5f7" }));
      text(svg, band.label, PAD + 8, y + 20, band.heading ? 17 : 13, COLORS[band.area], { "font-weight": 700 });
    }
    const positions = new Map(layout.nodes.map(n => [n.tech.id, n]));
    for (const edge of graph.edges || []) {
      const a = positions.get(edge.from), b = positions.get(edge.to);
      if (!a || !b) continue;
      const sx = a.x + W, sy = a.y + H / 2, tx = b.x, ty = b.y + H / 2;
      const bend = Math.max(46, Math.abs(tx - sx) * .45);
      svg.append(element(doc, "path", { d: `M${sx},${sy} C${sx+bend},${sy} ${tx-bend},${ty} ${tx},${ty}`,
        fill: "none", stroke: "#78909b", "stroke-width": 1.4, "marker-end": "url(#tte-pdf-arrow)" }));
    }
    // Measure with the same system font. SVG text stays text in Chrome's PDF;
    // no canvas is used for captions or connections.
    const measure = doc.createElement("canvas").getContext("2d");
    function caption(parent, value, x, y, size, maxWidth, maxLines) {
      value = plain(value); if (!value) return;
      measure.font = `${size}px "Segoe UI", "Microsoft YaHei UI", "PingFang SC", sans-serif`;
      const lines = []; let line = "";
      for (const char of Array.from(value)) {
        if (line && measure.measureText(line + char).width > maxWidth && lines.length < maxLines - 1) {
          lines.push(line); line = "";
        }
        line += char;
      }
      lines.push(line);
      const node = element(doc, "text", { x, y, "font-size": size, fill: "#152f3d" });
      lines.forEach((row, i) => {
        const attrs = { x, dy: i ? size + 2 : 0 };
        if (measure.measureText(row).width > maxWidth) Object.assign(attrs, { textLength: maxWidth, lengthAdjust: "spacingAndGlyphs" });
        node.append(element(doc, "tspan", attrs, row));
      });
      parent.append(node);
    }
    for (const { tech, x, y } of layout.nodes) {
      const group = element(doc, "g", { "data-technology": tech.id, transform: `translate(${x} ${y})` });
      const color = COLORS[tech.area] || "#4d6670";
      group.append(element(doc, "rect", { width: W, height: H, rx: 4, fill: "#f6f9fb", stroke: color, "stroke-width": 1.3 }));
      group.append(element(doc, "rect", { width: W, height: 3, fill: color }));
      if (atlas?.data && !tech.iconFallback && Number.isInteger(tech.iconIndex)) {
        const col = tech.iconIndex % atlas.columns, row = Math.floor(tech.iconIndex / atlas.columns);
        const icon = element(doc, "svg", { x: 7, y: 12, width: 44, height: 44,
          viewBox: `${col*atlas.tileSize} ${row*atlas.tileSize} ${atlas.tileSize} ${atlas.tileSize}` });
        icon.append(element(doc, "use", { href: "#tte-pdf-atlas" })); group.append(icon);
      } else {
        text(group, String(tech.area || "?").slice(0, 1).toUpperCase(), 15, 42, 24, color, { "font-weight": 700 });
      }
      text(group, `T${tech.tier ?? 0}`, 13, 79, 12, color, { "font-weight": 700 });
      const en = plain(tech.name?.en || tech.id), zh = plain(tech.name?.zh || "");
      // One text run per full name is intentional: PDF find can match the
      // complete name without a layout-induced line break inside the query.
      caption(group, en, 59, 25, 13, 252, 1);
      if (zh && zh !== en) caption(group, zh, 59, 55, 12, 252, 1);
      // Never ellipsize IDs or names: full values remain findable offline.
      caption(group, tech.id, 59, 87, 9, 252, 1);
      caption(group, tech.categoryName?.en || tech.category || "", 59, 103, 9, 252, 1);
      svg.append(group);
    }
    return { svg, ...layout };
  }

  let active = null;
  function open(graph, options = {}) {
    if (active) active.close();
    const doc = document, en = options.language === "en";
    const built = makeSvg(doc, graph, options);
    const root = doc.createElement("section"); root.id = "tte-pdf-export";
    root.setAttribute("role", "dialog"); root.setAttribute("aria-modal", "true");
    root.setAttribute("aria-label", en ? "Searchable PDF preview" : "可搜索 PDF 预览");
    const toolbar = doc.createElement("div"); toolbar.className = "tte-pdf-toolbar";
    const note = doc.createElement("span"); note.textContent = en
      ? "Searchable bilingual PDF · Choose Save as PDF in the print dialog. Vector text and lines remain zoomable."
      : "可搜索双语 PDF · 在打印窗口选择「另存为 PDF」。文字和连线可离线放大，Ctrl+F 搜索名称与 ID。";
    const save = doc.createElement("button"); save.type = "button";
    save.textContent = en ? "Save as PDF" : "保存为 PDF";
    const back = doc.createElement("button"); back.type = "button";
    back.textContent = en ? "Back to tree" : "返回科技树";
    toolbar.append(note, save, back);
    const scroll = doc.createElement("div"); scroll.className = "tte-pdf-scroll";scroll.append(built.svg);
    const style = doc.createElement("style");
    style.textContent = `
      #tte-pdf-export{position:fixed;inset:0;z-index:100000;background:#dce5e9;color:#173c4f;display:flex;flex-direction:column;font:14px "Segoe UI","Microsoft YaHei UI",sans-serif}
      #tte-pdf-export .tte-pdf-toolbar{display:flex;align-items:center;gap:12px;padding:12px 20px;background:#fff;border-bottom:1px solid #a8bbc5}
      #tte-pdf-export .tte-pdf-toolbar span{flex:1}
      #tte-pdf-export button{border:1px solid #7897a7;border-radius:5px;background:#f2f8fa;color:#143c4f;padding:8px 15px;cursor:pointer;white-space:nowrap}
      #tte-pdf-export button:focus-visible{outline:3px solid #268eba;outline-offset:2px}
      #tte-pdf-export .tte-pdf-scroll{flex:1;min-height:0;overflow:auto;padding:20px}
      #tte-pdf-export .tte-pdf-scroll>svg{display:block;width:1200px;height:auto;max-width:none;background:white;box-shadow:0 1px 9px #607581}
      @media print{
        @page{size:${built.pageWidth.toFixed(3)}mm ${built.pageHeight.toFixed(3)}mm;margin:0}
        html,body{margin:0!important;padding:0!important;height:auto!important;overflow:visible!important;background:#fff!important}
        body>*:not(#tte-pdf-export){display:none!important}
        #tte-pdf-export{position:static!important;display:block!important;background:#fff!important;width:${built.pageWidth.toFixed(3)}mm!important;height:auto!important}
        #tte-pdf-export .tte-pdf-toolbar{display:none!important}
        #tte-pdf-export .tte-pdf-scroll{display:block!important;overflow:visible!important;padding:0!important}
        #tte-pdf-export .tte-pdf-scroll>svg{width:100%!important;height:auto!important;max-width:none!important;box-shadow:none!important;break-inside:avoid}
      }`;
    root.append(style, toolbar, scroll);
    const app = doc.getElementById("app"), inert = app?.inert, title = doc.title;
    if (app) app.inert = true;
    const close = () => {
      root.remove(); if (app) app.inert = inert;
      doc.title = title; active = null; options.onClose?.();
    };
    root.addEventListener("keydown", event => {
      if (event.key === "Escape") { event.preventDefault(); close(); }
      if (event.key === "Tab") { event.preventDefault(); (doc.activeElement === save ? back : save).focus(); }
      // Ctrl+F is intentionally left to Chrome; prevent panning the hidden tree.
      event.stopPropagation();
    });
    save.addEventListener("click", () => window.print()); back.addEventListener("click", close);
    doc.title = "Stellaris Tech Tree - bilingual searchable";
    doc.body.append(root); active = { close }; save.focus();
    return { count: built.nodes.length, width: built.width, height: built.height, close };
  }
  const api = { plan, makeSvg, open, plain };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else window.TechTreePdf = api;
})();

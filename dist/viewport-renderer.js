/* A viewport-sized renderer: world coordinates never create a giant GPU layer. */
(() => {
  "use strict";
  const COLORS = { physics: "#5bbcff", society: "#67d5a4", engineering: "#f0a65e" };
  const CARD_W = 280, CARD_H = 84;
  const DOM_THRESHOLD = .48;

  class TechViewport {
    constructor({ viewport, canvas, nodes, makeNode, label }) {
      Object.assign(this, { viewport, canvas, nodes, makeNode, label });
      this.context = canvas.getContext("2d", { alpha: true });
      this.items = [];
      this.edges = [];
      this.bands = [];
      this.mounted = new Map();
      this.transform = { x: 0, y: 0, scale: 1 };
      this.highlights = { selectedId: null, related: new Set(), upstream: new Set(), downstream: new Set(), matches: new Set(), query: false };
      this.frame = 0;
      this.size = { width: 0, height: 0, dpr: 1 };
      this.cardCache = new Map();
      this.maxCachedCards = Infinity;
      this.cardSheets = [];
      this.iconAtlas = null;
      this.resizeObserver = new ResizeObserver(() => this.resize());
      this.resizeObserver.observe(viewport);
    }

    resize() {
      const { width, height } = this.viewport.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      if (width !== this.size.width || height !== this.size.height || dpr !== this.size.dpr) {
        this.size = { width, height, dpr };
        this.canvas.width = Math.round(width * dpr);
        this.canvas.height = Math.round(height * dpr);
      }
      this.requestDraw();
    }

    setGraph(items, edges, bands) {
      this.items = items;
      this.edges = edges;
      this.bands = bands;
      this.mounted.clear();
      this.nodes.replaceChildren();
      this.requestDraw();
    }

    setHighlights(highlights) {
      this.highlights = highlights;
      this.requestDraw();
    }

    setTransform(transform) {
      this.transform = { ...transform };
      this.requestDraw();
    }

    setIconAtlas(image, metadata) {
      this.iconAtlas = { image, ...metadata };
      this.invalidateCards();
    }

    invalidateCards() {
      this.cardCache.clear();
      // Release backing stores promptly on language / atlas changes.
      for (const sheet of this.cardSheets) { sheet.canvas.width = 0; sheet.canvas.height = 0; }
      this.cardSheets = [];
      this.requestDraw();
    }

    paintCard(ctx, tech) {
      const color = COLORS[tech.area];
      ctx.fillStyle = "#0b1c27"; ctx.fillRect(0, 0, CARD_W, CARD_H);
      ctx.fillStyle = "#102734"; ctx.fillRect(0, 0, 64, CARD_H);
      const atlas = this.iconAtlas;
      if (tech.iconFallback) {
        ctx.strokeStyle = color; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(32, 42, 22, 0, Math.PI * 2); ctx.stroke();
        ctx.fillStyle = color; ctx.font = '700 24px "Segoe UI", sans-serif';
        ctx.fillText(tech.area.slice(0, 1).toUpperCase(), 24, 50);
      } else if (atlas && Number.isInteger(tech.iconIndex)) {
        const col = tech.iconIndex % atlas.columns, row = Math.floor(tech.iconIndex / atlas.columns);
        ctx.drawImage(atlas.image, col * atlas.tileSize, row * atlas.tileSize, atlas.tileSize, atlas.tileSize, 0, 10, 64, 64);
      }
      ctx.fillStyle = color; ctx.fillRect(0, 0, CARD_W, 2);
      ctx.strokeStyle = "rgba(110,169,190,.28)"; ctx.lineWidth = 1;
      ctx.strokeRect(.5, .5, CARD_W - 1, CARD_H - 1);
      ctx.fillStyle = "rgba(0,0,0,.12)"; ctx.fillRect(242, 2, 38, CARD_H - 2);
      ctx.font = '700 12px "Segoe UI", sans-serif'; ctx.fillStyle = color;
      ctx.fillText(`T${tech.tier ?? 0}`, 253, 46);
      const label = this.label(tech);
      const text = (value, y, font, fill) => {
        ctx.font = font; ctx.fillStyle = fill;
        let visible = String(value || "");
        if (ctx.measureText(visible).width > 160) {
          while (visible && ctx.measureText(`${visible}…`).width > 160) visible = visible.slice(0, -1);
          visible += "…";
        }
        ctx.fillText(visible, 73, y);
      };
      text(label.primary, 31, '730 13.3px "Segoe UI", "Microsoft YaHei UI", sans-serif', "#d5e9f0");
      text(label.secondary, 47, '11px "Segoe UI", "Microsoft YaHei UI", sans-serif', "#8ea7b1");
      text(label.category, 62, '10.2px "Segoe UI", "Microsoft YaHei UI", sans-serif', "#78929c");
      let flagX = 273;
      for (const [flag, fill] of [["rare", "#b379f0"], ["dangerous", "#ff6872"], ["repeatable", "#ecd36c"]]) {
        if (!tech.flags?.[flag]) continue;
        ctx.fillStyle = fill; ctx.beginPath(); ctx.arc(flagX, 77, 2.5, 0, Math.PI * 2); ctx.fill(); flagX -= 8;
      }
    }

    cacheCard(tech) {
      // Packed raster cards: at most ~69 MiB for all 678, never a world-size layer.
      const index = this.cardCache.size, page = Math.floor(index / 128), slot = index % 128;
      if (!this.cardSheets[page]) {
        const canvas = document.createElement("canvas"); canvas.width = CARD_W * 8; canvas.height = CARD_H * 16;
        this.cardSheets[page] = { canvas, context: canvas.getContext("2d", { alpha: false }) };
      }
      const sheet = this.cardSheets[page], x = slot % 8 * CARD_W, y = Math.floor(slot / 8) * CARD_H;
      sheet.context.save(); sheet.context.translate(x, y); this.paintCard(sheet.context, tech); sheet.context.restore();
      const entry = { canvas: sheet.canvas, x, y };
      this.cardCache.set(tech.id, entry);
      return entry;
    }

    requestDraw() {
      if (this.frame) return;
      this.frame = requestAnimationFrame(() => {
        this.frame = 0;
        this.draw();
      });
    }

    isDimmed(id) {
      const h = this.highlights;
      return h.selectedId ? !h.related.has(id) : h.query && !h.matches.has(id);
    }

    draw() {
      const start = performance.now();
      const ctx = this.context;
      const { width, height, dpr } = this.size;
      const { x: panX, y: panY, scale } = this.transform;
      if (!ctx || !width || !height) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);
      const sx = x => panX + x * scale;
      const sy = y => panY + y * scale;
      const bounds = { left: (-panX - 40) / scale, right: (width - panX + 40) / scale,
        top: (-panY - 40) / scale, bottom: (height - panY + 40) / scale };

      for (const band of this.bands) {
        const x = sx(band.x), y = sy(band.y), w = band.width * scale, h = band.height * scale;
        if (x + w < 0 || y + h < 0 || x > width || y > height) continue;
        ctx.globalAlpha = 1;
        ctx.fillStyle = "rgba(127,184,206,.035)";
        ctx.fillRect(x, y, w, h);
        ctx.strokeStyle = "rgba(138,190,210,.13)";
        ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(x + w, y); ctx.stroke();
        if (band.heading || scale >= DOM_THRESHOLD) {
          ctx.font = `${band.heading ? 700 : 600} ${Math.max(band.heading ? 12 : 10, 14 * Math.min(1, scale))}px "Segoe UI", "Microsoft YaHei UI", sans-serif`;
          ctx.fillStyle = band.heading ? COLORS[band.area] : "#94b2bf";
          const overviewHeading = band.heading && scale < .18;
          const parts = band.label.split(" · ");
          ctx.fillText(band.heading && scale < .3 ? parts[0] : band.label,
            Math.max(overviewHeading ? x - 58 : x + 8, 8), overviewHeading ? y + 12 : y + (band.heading ? 15 : 18));
          if (band.heading && scale >= .65) {
            ctx.font = '600 11px "Segoe UI", sans-serif';
            ctx.fillStyle = "#7896a5";
            for (const tier of band.tiers || []) {
              ctx.fillText(`TIER ${tier}`, sx(150 + tier * 390), y + 34);
            }
          }
        }
      }

      let drawnEdges = 0;
      const h = this.highlights;
      for (const edge of this.edges) {
        if (edge.maxX < bounds.left || edge.minX > bounds.right || edge.maxY < bounds.top || edge.minY > bounds.bottom) continue;
        const up = h.selectedId && (h.upstream.has(edge.from) || edge.from === h.selectedId) && (h.upstream.has(edge.to) || edge.to === h.selectedId);
        const down = h.selectedId && (h.downstream.has(edge.from) || edge.from === h.selectedId) && (h.downstream.has(edge.to) || edge.to === h.selectedId);
        if (scale < .2 && !up && !down) continue;
        ctx.globalAlpha = up || down ? .95 : h.selectedId || h.query ? .12 : .5;
        ctx.strokeStyle = up ? "#8ad9ff" : down ? "#74e8b5" : "#385360";
        ctx.lineWidth = up || down ? 1.8 : 1;
        ctx.beginPath();
        ctx.moveTo(sx(edge.sx), sy(edge.sy));
        ctx.bezierCurveTo(sx(edge.c1x), sy(edge.sy), sx(edge.c2x), sy(edge.ty), sx(edge.tx), sy(edge.ty));
        ctx.stroke();
        if (scale >= .3 || up || down) {
          const tx = sx(edge.tx), ty = sy(edge.ty);
          ctx.fillStyle = ctx.strokeStyle;
          ctx.beginPath(); ctx.moveTo(tx, ty); ctx.lineTo(tx - 5, ty - 3); ctx.lineTo(tx - 5, ty + 3); ctx.closePath(); ctx.fill();
        }
        drawnEdges += 1;
      }

      const mountedIds = new Set();
      let drawnNodes = 0;
      let drawnIcons = 0, pendingCards = false, baked = 0;
      const bakeDeadline = performance.now() + 4;
      for (const item of this.items) {
        const { tech, x, y } = item;
        if (x + CARD_W < bounds.left || x > bounds.right || y + CARD_H < bounds.top || y > bounds.bottom) continue;
        drawnNodes += 1;
        const selected = h.selectedId === tech.id;
        const related = h.related.has(tech.id);
        const match = h.matches.has(tech.id);
        const dimmed = this.isDimmed(tech.id);

        if (scale >= DOM_THRESHOLD) {
          mountedIds.add(tech.id);
          let node = this.mounted.get(tech.id);
          if (!node) {
            node = this.makeNode(tech, 0, 0);
            this.nodes.append(node);
            this.mounted.set(tech.id, node);
          }
          node.style.left = `${sx(x)}px`;
          node.style.top = `${sy(y)}px`;
          node.style.transform = `scale(${scale})`;
          node.style.setProperty("--render-scale", String(scale));
          node.classList.toggle("selected", selected);
          node.classList.toggle("related", related && !selected);
          node.classList.toggle("dimmed", dimmed);
          node.classList.toggle("search-match", match);
          continue;
        }

        const px = sx(x), py = sy(y), w = CARD_W * scale, nodeH = CARD_H * scale;
        ctx.globalAlpha = dimmed ? .3 : 1;
        let cached = this.cardCache.get(tech.id);
        if (!cached && this.cardCache.size < this.maxCachedCards && (baked === 0 || (baked < 24 && performance.now() < bakeDeadline))) {
          cached = this.cacheCard(tech); baked += 1;
        }
        if (cached) ctx.drawImage(cached.canvas, cached.x, cached.y, CARD_W, CARD_H, px, py, w, nodeH);
        else {
          // During a bounded cache warm-up, render the same full card directly.
          // No placeholder rectangles or missing icons at any zoom level.
          ctx.save();
          ctx.translate(px, py); ctx.scale(scale, scale); this.paintCard(ctx, tech); ctx.restore();
          if (this.cardCache.size < this.maxCachedCards) pendingCards = true;
        }
        if (this.iconAtlas && Number.isInteger(tech.iconIndex)) drawnIcons += 1;
        if (selected || related || match) {
          ctx.strokeStyle = selected ? "#b8f3ff" : match ? "#fff3a7" : COLORS[tech.area];
          ctx.lineWidth = selected || related ? 1.5 : .7; ctx.strokeRect(px, py, w, nodeH);
        }
      }

      for (const [id, node] of this.mounted) {
        if (!mountedIds.has(id)) {
          node.remove();
          this.mounted.delete(id);
        }
      }
      ctx.globalAlpha = 1;
      // Observable diagnostics support regression checks without exposing app state.
      this.viewport.dataset.renderedNodes = String(drawnNodes);
      this.viewport.dataset.domNodes = String(this.mounted.size);
      this.viewport.dataset.renderedEdges = String(drawnEdges);
      this.viewport.dataset.renderedIcons = String(drawnIcons);
      this.viewport.dataset.cachedCards = String(this.cardCache.size);
      this.viewport.dataset.cardCacheBytes = String(this.cardSheets.reduce((sum, sheet) => sum + sheet.canvas.width * sheet.canvas.height * 4, 0));
      this.viewport.dataset.panX = panX.toFixed(2);
      this.viewport.dataset.panY = panY.toFixed(2);
      this.viewport.dataset.scale = scale.toFixed(4);
      this.viewport.dataset.renderMs = (performance.now() - start).toFixed(2);
      if (pendingCards) this.requestDraw();
    }

    hitTest(clientX, clientY) {
      const rect = this.viewport.getBoundingClientRect();
      const wx = (clientX - rect.left - this.transform.x) / this.transform.scale;
      const wy = (clientY - rect.top - this.transform.y) / this.transform.scale;
      return this.items.find(item => wx >= item.x && wx <= item.x + CARD_W && wy >= item.y && wy <= item.y + CARD_H)?.tech.id || null;
    }
  }

  window.TechViewport = TechViewport;
})();

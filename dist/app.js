(() => {
  "use strict";

  const CARD_W = 280;
  const CARD_H = 84;
  const COL_PITCH = 390;
  const ROW_PITCH = 104;
  const WORLD_LEFT = 150;
  const MIN_SCALE = 0.025;
  const MAX_SCALE = 1.9;
  const AREA_ORDER = ["physics", "society", "engineering"];
  const AREA_NAMES = {
    physics: ["物理学", "Physics"],
    society: ["社会学", "Society"],
    engineering: ["工程学", "Engineering"],
  };
  const CATEGORY_ORDER = {
    physics: ["computing", "field_manipulation", "particles"],
    society: ["biology", "military_theory", "new_worlds", "psionics", "statecraft", "metaphysics"],
    engineering: ["industry", "materials", "propulsion", "voidcraft", "robotics", "archaeostudies"],
  };
  const AREA_COLORS = { physics: "#5bbcff", society: "#67d5a4", engineering: "#f0a65e" };

  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
  const el = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const text = key => window.TechI18n.text(key, state.language);
  const pair = (zh, en) => window.TechI18n.pair(zh, en, state.language);
  const areaName = area => pair(...AREA_NAMES[area]);

  const dom = {
    app: $("#app"), versionLabel: $("#versionLabel"), searchShell: $("#searchShell"),
    searchInput: $("#searchInput"), searchResults: $("#searchResults"), areaTabs: $("#areaTabs"),
    tierFilters: $("#tierFilters"), categoryFilter: $("#categoryFilter"), resultCount: $("#resultCount"),
    resetFilters: $("#resetFilters"), filterPanel: $("#filterPanel"), mobileFilter: $("#mobileFilter"),
    dlcMode: $("#dlcMode"), dlcChoices: $("#dlcChoices"), dlcSummary: $("#dlcSummary"),
    excludeSeason10: $("#excludeSeason10"), dlcSelectAll: $("#dlcSelectAll"), dlcSelectNone: $("#dlcSelectNone"),
    dlcSection: $("#dlcSection"), detailDlc: $("#detailDlc"), detailDlcRule: $("#detailDlcRule"),
    canvasShell: $("#canvasShell"), viewport: $("#viewport"), world: $("#world"), graphCanvas: $("#graphCanvas"),
    fieldJumps: $("#fieldJumps"), nodes: $("#nodes"), canvasStatus: $("#canvasStatus"), nodeTemplate: $("#techNodeTemplate"),
    zoomIn: $("#zoomIn"), zoomOut: $("#zoomOut"), fitView: $("#fitView"), zoomLabel: $("#zoomLabel"),
    minimap: $("#minimap"), minimapWrap: $("#minimapWrap"), gestureHint: $("#gestureHint"),
    detailPanel: $("#detailPanel"), detailClose: $("#detailClose"), detailIcon: $("#detailIcon"),
    detailEyebrow: $("#detailEyebrow"), detailTitle: $("#detailTitle"), detailSubtitle: $("#detailSubtitle"),
    detailBadges: $("#detailBadges"), detailMetrics: $("#detailMetrics"),
    detailDescriptionZh: $("#detailDescriptionZh"), detailDescriptionEn: $("#detailDescriptionEn"),
    prereqSection: $("#prereqSection"), prereqList: $("#prereqList"), unlockSection: $("#unlockSection"),
    unlockList: $("#unlockList"), effectSection: $("#effectSection"), effectList: $("#effectList"),
    detailId: $("#detailId"), detailSource: $("#detailSource"), helpButton: $("#helpButton"),
    helpPopover: $("#helpPopover"), scrim: $("#scrim"), fatalError: $("#fatalError"),
  };

  const savedLanguage = (() => {
    try { return localStorage.getItem("stellaris-tech-language"); } catch { return null; }
  })();

  const state = {
    data: null,
    techById: new Map(),
    dependents: new Map(),
    renderer: null,
    searchScores: new Map(),
    areaBounds: new Map(),
    layout: new Map(),
    visible: [],
    area: "physics",
    language: ["zh", "en", "bilingual"].includes(savedLanguage) ? savedLanguage : "bilingual",
    tiers: new Set(),
    category: "all",
    sourceLayer: "all",
    dlcMode: "all",
    ownedDlcs: new Set(),
    flags: new Set(),
    selectedId: null,
    transform: { x: 0, y: 0, scale: 1 },
    worldWidth: 2500,
    worldHeight: 1600,
    searchResults: [],
    searchIndex: -1,
    searchQuery: "",
    pointers: new Map(),
    panStart: null,
    pinchStart: null,
    interactionCount: 0,
    minimapFrame: 0,
  };

  function buildDataIndexes() {
    window.TechSearch.configure(state.data.searchPinyin);
    state.techById = new Map(state.data.technologies.map(tech => [tech.id, tech]));
    state.dependents = new Map(state.data.technologies.map(tech => [tech.id, []]));
    for (const tech of state.data.technologies) {
      tech.search = window.TechSearch.build(tech);
      for (const prerequisite of tech.prerequisites) {
        if (state.dependents.has(prerequisite)) state.dependents.get(prerequisite).push(tech.id);
      }
      state.tiers.add(tech.tier);
    }
  }

  function setupControls() {
    if (state.data.meta.localViewer) setupLocalViewer();
    for (const area of [...AREA_ORDER, "all"]) {
      const count = area === "all" ? state.data.technologies.length : state.data.technologies.filter(tech => tech.area === area).length;
      const target = $(`[data-count="${area}"]`);
      if (target) target.textContent = count;
    }

    dom.tierFilters.replaceChildren();
    for (const tier of [...state.tiers].sort((a, b) => a - b)) {
      const button = el("button", "active", `T${tier}`);
      button.type = "button";
      button.dataset.tier = String(tier);
      button.setAttribute("aria-pressed", "true");
      button.addEventListener("click", () => {
        if (state.tiers.has(tier)) state.tiers.delete(tier); else state.tiers.add(tier);
        button.classList.toggle("active", state.tiers.has(tier));
        button.setAttribute("aria-pressed", String(state.tiers.has(tier)));
        renderGraph(true);
      });
      dom.tierFilters.append(button);
    }
    syncAreaTabs();
    syncLanguageButtons();
    updateCategoryOptions();
    setupDlcControls();
    syncLanguageButtons();
  }

  function setupLocalViewer() {
    const group = el("fieldset", "filter-group local-source-group");
    const legend = el("legend"); legend.id = "sourceLegend";
    const select = el("select"); select.id = "sourceFilter";
    select.setAttribute("aria-label", "Technology source / 科技来源");
    select.append(new Option("All sources / 全部来源", "all"));
    for (const source of state.data.sources) select.append(new Option(`${source.name} (${source.count})`, String(source.id)));
    select.addEventListener("change", () => {
      state.sourceLayer = select.value; renderGraph(true);
      if (state.searchQuery) showSearchResults();
    });
    const wrap = el("div", "select-wrap"); wrap.append(select);
    group.append(legend, wrap);
    const note = el("p", "filter-note"); note.id = "localSnapshotNote";
    group.append(note);
    dom.filterPanel.querySelector(".filter-group").before(group);
    dom.dlcMode.closest("fieldset").hidden = true;
    state.renderer.maxCachedCards = 512;
  }

  function setupDlcControls() {
    const catalog = state.data.dlcs || [];
    state.ownedDlcs = new Set(catalog.map(dlc => dlc.id));
    try {
      const saved = JSON.parse(localStorage.getItem("stellaris-tech-dlcs") || "null");
      if (saved && Array.isArray(saved.owned)) {
        state.ownedDlcs = new Set(saved.owned.filter(id => catalog.some(dlc => dlc.id === id)));
        if (["all", "owned", "base"].includes(saved.mode)) state.dlcMode = saved.mode;
      }
    } catch { /* session-only preferences */ }
    for (const dlc of catalog) {
      const label = el("label", "check-row");
      const input = el("input"); input.type = "checkbox"; input.dataset.dlc = dlc.id;
      const name = pair(dlc.name.zh, dlc.name.en);
      label.append(input, el("span", "check-ui"), el("span", "", `${name}${dlc.season === 10 ? " · S10" : ""}`));
      input.addEventListener("change", () => {
        if (input.checked) state.ownedDlcs.add(dlc.id); else state.ownedDlcs.delete(dlc.id);
        state.dlcMode = "owned";
        updateDlcFilter();
      });
      dom.dlcChoices.append(label);
    }
    dom.dlcMode.addEventListener("change", () => { state.dlcMode = dom.dlcMode.value; updateDlcFilter(); });
    dom.excludeSeason10.addEventListener("click", () => {
      if (state.dlcMode === "all") state.ownedDlcs = new Set(catalog.map(dlc => dlc.id));
      if (state.dlcMode === "base") state.ownedDlcs.clear();
      catalog.filter(dlc => dlc.season === 10).forEach(dlc => state.ownedDlcs.delete(dlc.id));
      state.dlcMode = "owned"; updateDlcFilter();
    });
    dom.dlcSelectAll.addEventListener("click", () => { state.ownedDlcs = new Set(catalog.map(dlc => dlc.id)); state.dlcMode = "owned"; updateDlcFilter(); });
    dom.dlcSelectNone.addEventListener("click", () => { state.ownedDlcs.clear(); state.dlcMode = "owned"; updateDlcFilter(); });
    syncDlcControls();
  }

  function syncDlcControls() {
    dom.dlcMode.value = state.dlcMode;
    dom.dlcSummary.textContent = `${state.ownedDlcs.size}/${(state.data.dlcs || []).length}`;
    $$('input[data-dlc]', dom.dlcChoices).forEach(input => { input.checked = state.ownedDlcs.has(input.dataset.dlc); });
  }

  function updateDlcFilter() {
    syncDlcControls();
    try { localStorage.setItem("stellaris-tech-dlcs", JSON.stringify({ mode: state.dlcMode, owned: [...state.ownedDlcs] })); } catch { /* session-only */ }
    renderGraph(true);
    if (state.selectedId) renderDetails(state.techById.get(state.selectedId));
    if (state.searchQuery) showSearchResults();
  }

  function dlcAllows(tech) {
    return state.dlcMode === "all" || window.TechDlc.allows(tech.dlcCondition, state.dlcMode === "base" ? new Set() : state.ownedDlcs);
  }

  function syncAreaTabs() {
    $$('button[data-area]', dom.areaTabs).forEach(button => button.classList.toggle("active", button.dataset.area === state.area));
  }

  function syncLanguageButtons() {
    $$('button[data-language]').forEach(button => button.classList.toggle("active", button.dataset.language === state.language));
    document.documentElement.lang = state.language === "en" ? "en" : "zh-CN";
    window.TechI18n.apply(state.language);
    if (state.data) {
      dom.versionLabel.textContent = `${state.data.meta.game.name} · ${text("localData")}`;
      document.title = `Stellaris ${text("brand")} · ${state.data.meta.game.raw}`;
      if (state.data.meta.localViewer && $("#sourceLegend")) {
        $("#sourceLegend").textContent = pair("来源", "Source");
        $("#sourceFilter").options[0].textContent = pair("全部来源", "All sources");
        $("#localSnapshotNote").textContent = pair("本地科技资料，尚未连接存档。", "Source snapshot; save not connected.");
      }
      $$('input[data-dlc]', dom.dlcChoices).forEach(input => {
        const dlc = state.data.dlcs.find(item => item.id === input.dataset.dlc);
        input.parentElement.lastElementChild.textContent = `${pair(dlc.name.zh, dlc.name.en)}${dlc.season === 10 ? " · S10" : ""}`;
      });
    } else {
      dom.versionLabel.textContent = text("loadingGame");
      document.title = `Stellaris ${text("brand")}`;
    }
    for (const area of [...AREA_ORDER, "all"]) {
      const names = area === "all" ? ["全部", "All fields"] : AREA_NAMES[area];
      const tab = $(`button[data-area="${area}"]`, dom.areaTabs);
      $("span:nth-child(2)", tab).textContent = state.language === "en" ? names[1] : names[0];
      $("small", tab).hidden = state.language !== "bilingual";
      const jump = $(`button[data-jump="${area}"]`, dom.fieldJumps);
      if (jump) jump.textContent = state.language === "en" ? names[1] : names[0];
    }
  }

  function updateCategoryOptions() {
    const current = state.category;
    const categories = new Map();
    for (const tech of state.data.technologies) {
      if (state.area !== "all" && tech.area !== state.area) continue;
      categories.set(tech.category, tech.categoryName);
    }
    dom.categoryFilter.replaceChildren(new Option(state.language === "en" ? "All categories" : "全部类别", "all"));
    [...categories.entries()].sort((a, b) => a[1].zh.localeCompare(b[1].zh, "zh-CN")).forEach(([id, name]) => {
      const label = state.language === "en" ? name.en : state.language === "zh" ? name.zh : `${name.zh} · ${name.en}`;
      dom.categoryFilter.add(new Option(label, id));
    });
    state.category = categories.has(current) ? current : "all";
    dom.categoryFilter.value = state.category;
  }

  function filterTechnologies() {
    return state.data.technologies.filter(tech => {
      if (state.sourceLayer !== "all" && String(tech.sourceLayer) !== state.sourceLayer) return false;
      if (state.area !== "all" && tech.area !== state.area) return false;
      if (!state.tiers.has(tech.tier)) return false;
      if (state.category !== "all" && tech.category !== state.category) return false;
      for (const flag of state.flags) if (!tech.flags[flag]) return false;
      return dlcAllows(tech);
    });
  }

  function orderedCategories(area, techs) {
    const present = new Set(techs.map(tech => tech.category));
    const preferred = (CATEGORY_ORDER[area] || []).filter(category => present.has(category));
    const rest = [...present].filter(category => !preferred.includes(category)).sort();
    return [...preferred, ...rest];
  }

  function languageText(tech) {
    const category = tech.categoryName[state.language === "en" ? "en" : "zh"] || tech.category;
    if (state.language === "en") return { primary: tech.name.en, secondary: "", category };
    if (state.language === "zh") return { primary: tech.name.zh, secondary: "", category };
    return { primary: tech.name.zh, secondary: tech.name.en, category };
  }

  function makeNode(tech, x, y) {
    const node = dom.nodeTemplate.content.firstElementChild.cloneNode(true);
    node.classList.add(tech.area);
    node.dataset.id = tech.id;
    node.style.left = `${x}px`;
    node.style.top = `${y}px`;
    applyIcon($(".node-icon", node), tech);
    const label = languageText(tech);
    $(".node-copy strong", node).textContent = label.primary;
    $(".node-copy em", node).textContent = label.secondary;
    $(".node-copy em", node).hidden = !label.secondary;
    $(".node-copy small", node).textContent = tech.categoryName[state.language === "en" ? "en" : "zh"] || tech.category;
    $(".node-tier", node).textContent = `T${tech.tier}`;
    const flags = $(".node-flags", node);
    ["rare", "dangerous", "repeatable"].forEach(flag => {
      if (tech.flags[flag]) flags.append(el("i", flag));
    });
    node.setAttribute("aria-label", `${pair(tech.name.zh, tech.name.en)}, ${text("tier")} ${tech.tier}`);
    node.addEventListener("click", event => {
      event.stopPropagation();
      selectTech(tech.id);
    });
    node.addEventListener("dblclick", event => {
      event.stopPropagation();
      focusTech(tech.id, true);
    });
    return node;
  }

  function renderGraph(shouldFit = false) {
    state.visible = filterTechnologies();
    const visibleIds = new Set(state.visible.map(tech => tech.id));
    state.layout.clear();
    state.areaBounds.clear();
    dom.resultCount.textContent = state.language === "en" ? `${state.visible.length} technologies` : `${state.visible.length} 项科技`;
    dom.fieldJumps.hidden = state.area !== "all";

    if (!state.visible.length) {
      dom.canvasStatus.hidden = false;
      dom.canvasStatus.replaceChildren(el("span", "", text("noTechs")));
      state.worldWidth = 1200;
      state.worldHeight = 700;
      state.renderer.setGraph([], [], []);
      scheduleMinimap();
      return;
    }
    dom.canvasStatus.hidden = true;

    const maxTier = Math.max(...state.visible.map(tech => tech.tier), 0);
    const areas = state.area === "all" ? AREA_ORDER : [state.area];
    const panelWidth = WORLD_LEFT * 2 + (maxTier + 1) * COL_PITCH;
    const items = [], bands = [], edges = [];
    let cursorY = 18;

    for (const area of areas) {
      const areaTechs = state.visible.filter(tech => tech.area === area);
      if (!areaTechs.length) continue;
      const areaY = cursorY;
      bands.push({ x: 0, y: cursorY, width: panelWidth, height: 40, heading: true, area,
        tiers: [...state.tiers].sort((a, b) => a - b),
        label: areaName(area) });
      cursorY += 40;

      for (const category of orderedCategories(area, areaTechs)) {
        const categoryTechs = areaTechs.filter(tech => tech.category === category);
        const byTier = new Map();
        for (const tech of categoryTechs) {
          if (!byTier.has(tech.tier)) byTier.set(tech.tier, []);
          byTier.get(tech.tier).push(tech);
        }
        for (const group of byTier.values()) group.sort((a, b) => a.name.en.localeCompare(b.name.en));
        const maxRows = Math.max(...[...byTier.values()].map(group => group.length), 1);
        const bandHeight = Math.max(132, 52 + maxRows * ROW_PITCH);
        const sample = categoryTechs[0];
        bands.push({ x: 0, y: cursorY, width: panelWidth, height: bandHeight, area,
          label: pair(sample.categoryName.zh, sample.categoryName.en) });

        for (const [tier, group] of byTier.entries()) {
          group.forEach((tech, index) => {
            const x = WORLD_LEFT + tier * COL_PITCH;
            const y = cursorY + 42 + index * ROW_PITCH;
            state.layout.set(tech.id, { x, y });
            items.push({ tech, x, y });
          });
        }
        cursorY += bandHeight + 14;
      }
      state.areaBounds.set(area, { x: 0, y: areaY, width: panelWidth, height: cursorY - areaY });
      cursorY += state.area === "all" ? 56 : 18;
    }

    state.worldWidth = panelWidth;
    state.worldHeight = cursorY + 42;
    for (const tech of state.visible) {
      const target = state.layout.get(tech.id);
      for (const prerequisite of tech.prerequisites) {
        if (!visibleIds.has(prerequisite)) continue;
        const source = state.layout.get(prerequisite);
        if (!source || !target) continue;
        const sx = source.x + CARD_W;
        const sy = source.y + CARD_H / 2;
        const tx = target.x;
        const ty = target.y + CARD_H / 2;
        const bend = Math.max(52, Math.abs(tx - sx) * 0.45);
        const c1x = sx + bend, c2x = tx - bend;
        edges.push({ from: prerequisite, to: tech.id, sx, sy, tx, ty, c1x, c2x,
          minX: Math.min(sx, tx, c1x, c2x), maxX: Math.max(sx, tx, c1x, c2x),
          minY: Math.min(sy, ty), maxY: Math.max(sy, ty) });
      }
    }
    state.renderer.setGraph(items, edges, bands);
    applyHighlights();
    if (state.selectedId && !visibleIds.has(state.selectedId)) closeDetails();
    if (shouldFit) {
      if (state.selectedId && visibleIds.has(state.selectedId)) focusTech(state.selectedId);
      else readableView(areas[0]);
    } else applyTransform();
    scheduleMinimap();
  }

  function collectRelations(id, direction) {
    const visited = new Set();
    const stack = [id];
    while (stack.length) {
      const current = stack.pop();
      const next = direction === "up"
        ? (state.techById.get(current)?.prerequisites || [])
        : (state.dependents.get(current) || []);
      for (const related of next) {
        if (visited.has(related)) continue;
        visited.add(related);
        stack.push(related);
      }
    }
    return visited;
  }

  function applyHighlights() {
    const query = state.searchQuery.trim();
    const searchMatches = new Set();
    if (query) {
      for (const tech of state.visible) if ((state.searchScores.get(tech.id) ?? Infinity) < 80) searchMatches.add(tech.id);
    }
    const upstream = state.selectedId ? collectRelations(state.selectedId, "up") : new Set();
    const downstream = state.selectedId ? collectRelations(state.selectedId, "down") : new Set();
    const related = new Set([...upstream, ...downstream, state.selectedId].filter(Boolean));

    state.renderer.setHighlights({ selectedId: state.selectedId, related, upstream, downstream, matches: searchMatches, query: Boolean(query) });
    scheduleMinimap();
  }

  function selectTech(id) {
    const tech = state.techById.get(id);
    if (!tech) return;
    state.selectedId = id;
    applyHighlights();
    renderDetails(tech);
  }

  function closeDetails() {
    state.selectedId = null;
    dom.detailPanel.classList.remove("open");
    dom.detailPanel.setAttribute("aria-hidden", "true");
    dom.scrim.classList.remove("show");
    applyHighlights();
  }

  function formatNumber(value) {
    if (value === null || value === undefined || value === "") return "—";
    return typeof value === "number" ? new Intl.NumberFormat().format(value) : String(value);
  }

  function addRelationButton(container, relatedId) {
    const related = state.techById.get(relatedId);
    if (!related) return;
    const button = el("button");
    button.type = "button";
    button.disabled = !dlcAllows(related);
    if (button.disabled) button.title = text("excluded");
    const image = el("span", "tech-icon"); image.setAttribute("aria-hidden", "true"); applyIcon(image, related);
    const copy = el("span");
    copy.append(el("strong", "", languageText(related).primary));
    if (state.language === "bilingual") copy.append(el("small", "", related.name.en));
    button.append(image, copy);
    button.addEventListener("click", () => navigateToTech(relatedId));
    container.append(button);
  }

  function renderDetails(tech) {
    applyIcon(dom.detailIcon, tech);
    dom.detailEyebrow.textContent = `${areaName(tech.area)} · ${pair(tech.categoryName.zh, tech.categoryName.en)}`;
    dom.detailTitle.textContent = languageText(tech).primary;
    dom.detailSubtitle.textContent = state.language === "bilingual" ? tech.name.en : "";
    dom.detailSubtitle.hidden = state.language !== "bilingual";
    dom.detailDescriptionZh.textContent = tech.description.zh || "暂无中文说明。";
    dom.detailDescriptionEn.textContent = tech.description.en || "No English description available.";
    dom.detailDescriptionZh.hidden = state.language === "en";
    dom.detailDescriptionEn.hidden = state.language === "zh";
    dom.detailId.textContent = tech.id;
    dom.detailSource.textContent = tech.source;
    if (state.data.meta.localViewer && tech.iconFallback) dom.detailSource.textContent += ` · ${pair("占位图标", "Placeholder icon")}`;
    if (state.data.meta.localViewer && tech.unknownPrerequisites?.length) {
      dom.detailSource.textContent += ` · ${pair("未解析的前置", "Unresolved prerequisites")}: ${tech.unknownPrerequisites.join(", ")}`;
    }
    renderDlcConditions(tech);
    dom.detailBadges.replaceChildren();
    const badges = [
      ...["rare", "dangerous", "repeatable", "start", "insight", "conditional"].map(flag => [tech.flags[flag], flag, pair(window.TechI18n.text(flag, "zh"), window.TechI18n.text(flag, "en"))]),
    ];
    badges.forEach(([enabled, className, label]) => { if (enabled) dom.detailBadges.append(el("span", className, label)); });

    dom.detailMetrics.replaceChildren();
    [[text("tier"), `T${tech.tier}`], [text("researchCost"), formatNumber(tech.cost)], [text("weight"), formatNumber(tech.weight)]].forEach(([label, value]) => {
      const item = el("div"); item.append(el("small", "", label), el("strong", "", value)); dom.detailMetrics.append(item);
    });

    dom.prereqList.replaceChildren();
    const hasAlternative = condition => Array.isArray(condition) && (condition[0] === "any" || condition.slice(1).some(hasAlternative));
    if (hasAlternative(tech.prerequisiteCondition)) dom.prereqList.append(el("span", "relation-empty", text("alternative")));
    if (tech.prerequisites.length) tech.prerequisites.forEach(id => addRelationButton(dom.prereqList, id));
    else dom.prereqList.append(el("span", "relation-empty", text("noPrerequisite")));

    const leadsTo = state.dependents.get(tech.id) || [];
    dom.unlockList.replaceChildren();
    if (leadsTo.length) leadsTo.slice().sort((a, b) => state.techById.get(a).tier - state.techById.get(b).tier).forEach(id => addRelationButton(dom.unlockList, id));
    else dom.unlockList.append(el("span", "relation-empty", text("noSuccessor")));

    dom.effectList.replaceChildren();
    const directEffects = [...(tech.effects || []), ...(tech.unlocks || []).map(item => ({ ...item, value: state.language === "en" ? "Unlock" : "解锁" }))];
    dom.effectSection.hidden = !directEffects.length;
    directEffects.forEach(effect => {
      const copy = el("span", "effect-copy", state.language === "en" ? effect.en : effect.zh || effect.en);
      if (state.language === "bilingual" && effect.en && effect.en !== effect.zh) copy.append(el("small", "", effect.en));
      if (state.language !== "en" && effect.localized === false) copy.append(el("small", "", text("noChinese")));
      const row = el("div"); row.append(copy, el("strong", "", effect.value)); dom.effectList.append(row);
    });
    dom.detailPanel.classList.add("open");
    dom.detailPanel.setAttribute("aria-hidden", "false");
    if (matchMedia("(max-width: 820px)").matches) dom.scrim.classList.add("show");
  }

  function renderDlcConditions(tech) {
    dom.dlcSection.hidden = !tech.dlcs?.length;
    dom.detailDlc.replaceChildren();
    if (dom.dlcSection.hidden) return;
    const owned = state.dlcMode === "all" ? new Set(state.data.dlcs.map(dlc => dlc.id)) : state.dlcMode === "base" ? new Set() : state.ownedDlcs;
    const mark = condition => {
      const result = window.TechDlc.evaluate(condition, owned);
      const icon = el("span", `condition-mark ${result === null ? "unknown" : result ? "met" : "unmet"}`, result === null ? "—" : result ? "✓" : "✕");
      icon.setAttribute("aria-hidden", "true"); return icon;
    };
    const name = id => {
      const dlc = state.data.dlcs.find(item => item.id === id);
      return dlc ? pair(dlc.name.zh, dlc.name.en) : id;
    };
    function draw(condition) {
      if (condition == null || typeof condition === "boolean") {
        const row = el("div", "dlc-condition-row");
        row.append(mark(condition), el("span", "", text(condition == null ? "gameplay" : condition ? "unrestricted" : "unavailable"))); return row;
      }
      const [op, ...children] = condition;
      if (op === "dlc" || op === "not" && Array.isArray(children[0]) && children[0][0] === "dlc") {
        const id = op === "dlc" ? children[0] : children[0][1];
        const row = el("div", "dlc-condition-row"); row.dataset.dlc = id;
        row.append(mark(condition), el("span", "", `${op === "not" ? `${text("not")} ` : ""}${name(id)}`));
        row.title = state.dlcMode === "all" ? text("assumed") : text(owned.has(id) ? "enabled" : "disabled"); return row;
      }
      const group = el("div", "dlc-condition-group");
      const heading = el("div", "dlc-condition-label");
      heading.append(mark(condition), el("span", "", text(op === "all" ? "allRules" : op === "any" ? "anyRules" : "notRule")));
      group.append(heading); children.forEach(child => group.append(draw(child))); return group;
    }
    dom.detailDlc.append(draw(tech.dlcCondition));
    dom.detailDlcRule.textContent = `${state.dlcMode === "all" ? `${text("assumed")} · ` : ""}${text(window.TechDlc.evaluate(tech.dlcCondition, owned) === false ? "blocked" : "eligible")}`;
  }

  function navigateToTech(id) {
    const tech = state.techById.get(id);
    if (!tech) return;
    if (!state.layout.has(id)) {
      state.area = tech.area;
      state.category = "all";
      state.sourceLayer = "all";
      if ($("#sourceFilter")) $("#sourceFilter").value = "all";
      state.flags.clear();
      state.tiers = new Set(state.data.technologies.map(item => item.tier));
      syncAreaTabs(); updateCategoryOptions(); syncFilterControls(); renderGraph(false);
    }
    requestAnimationFrame(() => focusTech(id, true));
  }

  function syncFilterControls() {
    $$('[data-tier]', dom.tierFilters).forEach(button => {
      const active = state.tiers.has(Number(button.dataset.tier));
      button.classList.toggle("active", active); button.setAttribute("aria-pressed", String(active));
    });
    $$('input[data-flag]', dom.filterPanel).forEach(input => { input.checked = state.flags.has(input.dataset.flag); });
    dom.categoryFilter.value = state.category;
  }

  function applyTransform() {
    const { scale } = state.transform;
    state.renderer.setTransform(state.transform);
    dom.zoomLabel.textContent = `${Math.round(scale * 100)}%`;
    scheduleMinimap();
  }

  function setScaleAt(nextScale, screenX, screenY) {
    const old = state.transform.scale;
    const scale = clamp(nextScale, state.data.meta.localViewer ? .001 : MIN_SCALE, MAX_SCALE);
    const worldX = (screenX - state.transform.x) / old;
    const worldY = (screenY - state.transform.y) / old;
    state.transform.scale = scale;
    state.transform.x = screenX - worldX * scale;
    state.transform.y = screenY - worldY * scale;
    applyTransform();
  }

  function fitView() {
    const rect = dom.viewport.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const padding = 72;
    const scale = clamp(Math.min((rect.width - padding * 2) / state.worldWidth, (rect.height - padding * 2) / state.worldHeight, .92), state.data.meta.localViewer ? .001 : MIN_SCALE, .92);
    state.transform = {
      scale,
      x: (rect.width - state.worldWidth * scale) / 2,
      y: (rect.height - state.worldHeight * scale) / 2,
    };
    applyTransform();
  }

  function readableView(area) {
    const bounds = state.areaBounds.get(area) || [...state.areaBounds.values()][0];
    if (!bounds) return;
    const rect = dom.viewport.getBoundingClientRect();
    const scale = clamp(rect.width / (COL_PITCH * 3.2), .5, .85);
    const firstY = Math.min(...state.visible.filter(tech => tech.area === area).map(tech => state.layout.get(tech.id).y));
    state.transform = { scale, x: 22 - (bounds.x + WORLD_LEFT) * scale,
      y: 86 - (Number.isFinite(firstY) ? firstY : 100) * scale };
    applyTransform();
  }

  function focusTech(id, select = false) {
    const position = state.layout.get(id);
    if (!position) return;
    if (select) selectTech(id);
    const rect = dom.viewport.getBoundingClientRect();
    const panelWidth = dom.detailPanel.classList.contains("open") && !matchMedia("(max-width: 820px)").matches
      ? dom.detailPanel.getBoundingClientRect().width : 0;
    const scale = clamp(Math.max(state.transform.scale, .82), .62, 1.12);
    state.transform = {
      scale,
      x: (rect.width - panelWidth) / 2 - (position.x + CARD_W / 2) * scale,
      y: rect.height / 2 - (position.y + CARD_H / 2) * scale,
    };
    applyTransform();
  }

  function scheduleMinimap() {
    if (state.minimapFrame) return;
    state.minimapFrame = requestAnimationFrame(() => {
      state.minimapFrame = 0;
      drawMinimap();
    });
  }

  function drawMinimap() {
    const canvas = dom.minimap;
    const context = canvas.getContext("2d");
    if (!context) return;
    const width = canvas.width, height = canvas.height, padding = 12;
    context.clearRect(0, 0, width, height);
    context.fillStyle = "rgba(5,14,20,.95)"; context.fillRect(0, 0, width, height);
    if (!state.visible.length) return;
    const scale = Math.min((width - padding * 2) / state.worldWidth, (height - padding * 2) / state.worldHeight);
    const ox = (width - state.worldWidth * scale) / 2;
    const oy = (height - state.worldHeight * scale) / 2;
    for (const tech of state.visible) {
      const position = state.layout.get(tech.id); if (!position) continue;
      context.globalAlpha = state.selectedId && tech.id !== state.selectedId ? .28 : .8;
      context.fillStyle = tech.id === state.selectedId ? "#fff2a2" : AREA_COLORS[tech.area];
      context.fillRect(ox + position.x * scale, oy + position.y * scale, Math.max(2, CARD_W * scale), Math.max(1.3, CARD_H * scale));
    }
    context.globalAlpha = 1;
    const viewportRect = dom.viewport.getBoundingClientRect();
    const worldX = -state.transform.x / state.transform.scale;
    const worldY = -state.transform.y / state.transform.scale;
    const worldW = viewportRect.width / state.transform.scale;
    const worldH = viewportRect.height / state.transform.scale;
    context.strokeStyle = "rgba(205,245,255,.9)"; context.lineWidth = 2;
    context.strokeRect(ox + worldX * scale, oy + worldY * scale, worldW * scale, worldH * scale);
  }

  function showSearchResults() {
    const query = dom.searchInput.value.trim();
    state.searchQuery = query;
    state.searchIndex = -1;
    dom.searchResults.replaceChildren();
    if (!query) {
      state.searchResults = [];
      state.searchScores.clear();
      dom.searchResults.hidden = true;
      dom.searchInput.setAttribute("aria-expanded", "false");
      applyHighlights();
      return;
    }
    const scored = state.data.technologies.filter(tech => dlcAllows(tech) && (state.sourceLayer === "all" || String(tech.sourceLayer) === state.sourceLayer)).map(tech => ({ tech, ...window.TechSearch.match(tech.search, query) }));
    state.searchScores = new Map(scored.map(item => [item.tech.id, item.score]));
    state.searchResults = scored
      .filter(item => Number.isFinite(item.score))
      .sort((a, b) => a.score - b.score || a.tech.tier - b.tech.tier)
      .slice(0, 12);
    if (!state.searchResults.length) {
      dom.searchResults.append(el("div", "search-empty", text("noResults")));
    } else {
      state.searchResults.forEach((result, index) => {
        const { tech } = result;
        const button = el("button", "search-result");
        button.type = "button"; button.role = "option"; button.dataset.index = String(index);
        const image = el("span", "tech-icon"); image.setAttribute("aria-hidden", "true"); applyIcon(image, tech);
        const copy = el("span"); copy.append(el("strong", "", languageText(tech).primary));
        if (state.language === "bilingual") copy.append(el("em", "", tech.name.en));
        if (result.field && result.field.kind !== "name") {
          const snippet = el("span", "search-match", `${text(`match_${result.field.kind}`)}: ${window.TechSearch.snippet(result, state.language)}`);
          copy.append(snippet);
        }
        button.append(image, copy, el("small", "", `${state.language === "en" ? AREA_NAMES[tech.area][1] : AREA_NAMES[tech.area][0]} · T${tech.tier}`));
        button.addEventListener("mousedown", event => event.preventDefault());
        button.addEventListener("click", () => chooseSearchResult(index));
        dom.searchResults.append(button);
      });
    }
    dom.searchResults.hidden = false;
    dom.searchInput.setAttribute("aria-expanded", "true");
    applyHighlights();
  }

  function chooseSearchResult(index) {
    const result = state.searchResults[index];
    if (!result) return;
    dom.searchResults.hidden = true;
    dom.searchInput.setAttribute("aria-expanded", "false");
    navigateToTech(result.tech.id);
    dom.viewport.focus({ preventScroll: true });
  }

  function moveSearchSelection(delta) {
    if (!state.searchResults.length) return;
    state.searchIndex = (state.searchIndex + delta + state.searchResults.length) % state.searchResults.length;
    $$(".search-result", dom.searchResults).forEach((item, index) => item.classList.toggle("active", index === state.searchIndex));
    $$(".search-result", dom.searchResults)[state.searchIndex]?.scrollIntoView({ block: "nearest" });
  }

  function registerEvents() {
    dom.areaTabs.addEventListener("click", event => {
      const button = event.target.closest("button[data-area]");
      if (!button || button.dataset.area === state.area) return;
      state.area = button.dataset.area;
      state.category = "all";
      syncAreaTabs(); updateCategoryOptions(); renderGraph(true);
    });
    dom.fieldJumps.addEventListener("click", event => {
      const button = event.target.closest("button[data-jump]");
      if (button) readableView(button.dataset.jump);
    });
    dom.categoryFilter.addEventListener("change", () => { state.category = dom.categoryFilter.value; renderGraph(true); });
    $$('input[data-flag]', dom.filterPanel).forEach(input => input.addEventListener("change", () => {
      if (input.checked) state.flags.add(input.dataset.flag); else state.flags.delete(input.dataset.flag);
      renderGraph(true);
    }));
    dom.resetFilters.addEventListener("click", () => {
      state.tiers = new Set(state.data.technologies.map(tech => tech.tier)); state.flags.clear(); state.category = "all";
      state.dlcMode = "all";
      state.sourceLayer = "all";
      if ($("#sourceFilter")) $("#sourceFilter").value = "all";
      syncDlcControls();
      try { localStorage.setItem("stellaris-tech-dlcs", JSON.stringify({ mode: state.dlcMode, owned: [...state.ownedDlcs] })); } catch { /* session-only */ }
      syncFilterControls(); renderGraph(true);
      if (state.selectedId) renderDetails(state.techById.get(state.selectedId));
      if (state.searchQuery) showSearchResults();
    });
    $$('button[data-language]').forEach(button => button.addEventListener("click", () => {
      state.language = button.dataset.language;
      try { localStorage.setItem("stellaris-tech-language", state.language); } catch { /* preferences remain session-only */ }
      syncLanguageButtons(); updateCategoryOptions(); renderGraph(false);
      state.renderer.invalidateCards();
      if (state.selectedId) renderDetails(state.techById.get(state.selectedId));
      if (state.searchQuery) showSearchResults();
    }));

    dom.searchInput.addEventListener("input", showSearchResults);
    dom.searchInput.addEventListener("focus", () => { if (dom.searchInput.value.trim()) showSearchResults(); });
    dom.searchInput.addEventListener("keydown", event => {
      if (event.key === "ArrowDown") { event.preventDefault(); moveSearchSelection(1); }
      if (event.key === "ArrowUp") { event.preventDefault(); moveSearchSelection(-1); }
      if (event.key === "Enter" && state.searchIndex >= 0) { event.preventDefault(); chooseSearchResult(state.searchIndex); }
      if (event.key === "Escape") { dom.searchResults.hidden = true; dom.searchInput.setAttribute("aria-expanded", "false"); dom.searchInput.blur(); }
    });
    document.addEventListener("pointerdown", event => {
      if (!dom.searchShell.contains(event.target)) { dom.searchResults.hidden = true; dom.searchInput.setAttribute("aria-expanded", "false"); }
      if (!dom.helpPopover.contains(event.target) && event.target !== dom.helpButton) dom.helpPopover.hidden = true;
    });

    dom.viewport.addEventListener("wheel", event => {
      event.preventDefault();
      const rect = dom.viewport.getBoundingClientRect();
      const factor = Math.exp(-event.deltaY * .00135);
      setScaleAt(state.transform.scale * factor, event.clientX - rect.left, event.clientY - rect.top);
      markInteraction();
    }, { passive: false });

    dom.viewport.addEventListener("pointerdown", event => {
      if (event.button !== 0 || event.target.closest(".tech-node")) return;
      const rect = dom.viewport.getBoundingClientRect();
      const point = { x: event.clientX - rect.left, y: event.clientY - rect.top };
      state.pointers.set(event.pointerId, point);
      dom.viewport.setPointerCapture(event.pointerId);
      if (state.pointers.size === 1) {
        state.panStart = { point, transform: { ...state.transform }, moved: false };
        dom.viewport.classList.add("dragging");
      } else if (state.pointers.size === 2) {
        const [a, b] = [...state.pointers.values()];
        const center = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
        state.pinchStart = {
          distance: Math.hypot(a.x - b.x, a.y - b.y), center, transform: { ...state.transform },
          world: { x: (center.x - state.transform.x) / state.transform.scale, y: (center.y - state.transform.y) / state.transform.scale },
        };
      }
    });
    dom.viewport.addEventListener("pointermove", event => {
      if (!state.pointers.has(event.pointerId)) return;
      const rect = dom.viewport.getBoundingClientRect();
      state.pointers.set(event.pointerId, { x: event.clientX - rect.left, y: event.clientY - rect.top });
      if (state.pointers.size >= 2 && state.pinchStart) {
        const [a, b] = [...state.pointers.values()];
        const center = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
        const distance = Math.hypot(a.x - b.x, a.y - b.y);
        const scale = clamp(state.pinchStart.transform.scale * distance / Math.max(1, state.pinchStart.distance), state.data.meta.localViewer ? .001 : MIN_SCALE, MAX_SCALE);
        state.transform = { scale, x: center.x - state.pinchStart.world.x * scale, y: center.y - state.pinchStart.world.y * scale };
        applyTransform(); markInteraction();
      } else if (state.panStart) {
        const point = state.pointers.get(event.pointerId);
        const dx = point.x - state.panStart.point.x, dy = point.y - state.panStart.point.y;
        if (Math.hypot(dx, dy) > 3) state.panStart.moved = true;
        state.transform.x = state.panStart.transform.x + dx;
        state.transform.y = state.panStart.transform.y + dy;
        applyTransform(); markInteraction();
      }
    });
    const endPointer = event => {
      const wasClick = state.panStart && !state.panStart.moved && state.pointers.size === 1;
      state.pointers.delete(event.pointerId);
      if (state.pointers.size === 1) {
        const point = [...state.pointers.values()][0];
        state.panStart = { point, transform: { ...state.transform }, moved: true };
      } else {
        state.panStart = null; state.pinchStart = null; dom.viewport.classList.remove("dragging");
      }
      if (wasClick && event.type !== "pointercancel") {
        const id = state.renderer.hitTest(event.clientX, event.clientY);
        if (id) focusTech(id, true); else closeDetails();
      }
    };
    dom.viewport.addEventListener("pointerup", endPointer);
    dom.viewport.addEventListener("pointercancel", endPointer);

    dom.zoomIn.addEventListener("click", () => {
      const rect = dom.viewport.getBoundingClientRect(); setScaleAt(state.transform.scale * 1.2, rect.width / 2, rect.height / 2); markInteraction();
    });
    dom.zoomOut.addEventListener("click", () => {
      const rect = dom.viewport.getBoundingClientRect(); setScaleAt(state.transform.scale / 1.2, rect.width / 2, rect.height / 2); markInteraction();
    });
    dom.fitView.addEventListener("click", fitView);
    dom.detailClose.addEventListener("click", closeDetails);

    dom.minimap.addEventListener("pointerdown", event => {
      const rect = dom.minimap.getBoundingClientRect();
      const px = (event.clientX - rect.left) / rect.width * dom.minimap.width;
      const py = (event.clientY - rect.top) / rect.height * dom.minimap.height;
      const padding = 12;
      const miniScale = Math.min((dom.minimap.width - padding * 2) / state.worldWidth, (dom.minimap.height - padding * 2) / state.worldHeight);
      const ox = (dom.minimap.width - state.worldWidth * miniScale) / 2;
      const oy = (dom.minimap.height - state.worldHeight * miniScale) / 2;
      const worldX = (px - ox) / miniScale, worldY = (py - oy) / miniScale;
      const viewportRect = dom.viewport.getBoundingClientRect();
      state.transform.x = viewportRect.width / 2 - worldX * state.transform.scale;
      state.transform.y = viewportRect.height / 2 - worldY * state.transform.scale;
      applyTransform(); markInteraction();
    });

    dom.mobileFilter.addEventListener("click", () => { dom.filterPanel.classList.add("open"); dom.scrim.classList.add("show"); });
    dom.scrim.addEventListener("click", () => { dom.filterPanel.classList.remove("open"); closeDetails(); dom.scrim.classList.remove("show"); });
    dom.helpButton.addEventListener("click", () => { dom.helpPopover.hidden = !dom.helpPopover.hidden; });
    window.addEventListener("keydown", event => {
      const typing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName || "") || document.activeElement?.isContentEditable;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") { event.preventDefault(); dom.searchInput.focus(); dom.searchInput.select(); }
      if (!typing && event.key.toLowerCase() === "f") { event.preventDefault(); fitView(); }
      if (event.key === "Escape") { dom.filterPanel.classList.remove("open"); dom.helpPopover.hidden = true; closeDetails(); }
    });
    state.keyboardPan = new window.KeyboardPan({
      canPan: () => state.pointers.size === 0 && !dom.scrim.classList.contains("show"),
      onPan: (x, y) => {
        state.transform.x += x; state.transform.y += y;
        applyTransform(); markInteraction();
      },
    });
    new ResizeObserver(() => scheduleMinimap()).observe(dom.viewport);
  }

  function markInteraction() {
    state.interactionCount += 1;
    if (state.interactionCount > 1) dom.gestureHint.classList.add("hidden");
  }

  function applyIcon(element, tech) {
    if (state.data.meta.localViewer && tech.iconFallback) {
      element.style.backgroundImage = `url("assets/field-${tech.area}.svg")`;
      element.style.backgroundSize = "cover";
      element.style.backgroundPosition = "center";
      return;
    }
    const atlas = state.data.meta.iconAtlas;
    if (atlas && Number.isInteger(tech.iconIndex) && !state.atlasFailed) {
      const column = tech.iconIndex % atlas.columns, row = Math.floor(tech.iconIndex / atlas.columns);
      element.style.backgroundImage = "var(--tech-atlas)";
      element.style.backgroundSize = `${atlas.columns * 100}% ${atlas.rows * 100}%`;
      element.style.backgroundPosition = `${atlas.columns > 1 ? column / (atlas.columns - 1) * 100 : 0}% ${atlas.rows > 1 ? row / (atlas.rows - 1) * 100 : 0}%`;
    } else {
      element.style.backgroundImage = tech.icon ? `url("${tech.icon}")` : "none";
      element.style.backgroundSize = "cover";
      element.style.backgroundPosition = "center";
    }
  }

  function prepareIcons() {
    const atlas = state.data.meta.iconAtlas;
    if (!atlas) return;
    document.documentElement.style.setProperty("--tech-atlas", `url("${atlas.src}")`);
    // Retain the decoded atlas even when viewport culling removes every card.
    state.iconImage = new Image();
    state.iconImage.decoding = "async";
    state.iconImage.fetchPriority = "high";
    state.iconImage.src = atlas.src;
    state.iconImage.decode().then(() => state.renderer?.setIconAtlas(state.iconImage, atlas)).catch(() => {
      state.atlasFailed = true;
      renderGraph(false);
      if (state.selectedId) renderDetails(state.techById.get(state.selectedId));
    });
    if (!state.data.meta.localViewer && "serviceWorker" in navigator) {
      navigator.serviceWorker.register("sw.js", { scope: "./" }).then(() => navigator.serviceWorker.ready)
        .then(registration => registration.active?.postMessage({ type: "cache-atlas", src: new URL(atlas.src, location.href).href }))
        .catch(error => console.warn("Persistent icon cache unavailable", error));
    }
  }

  async function init() {
    syncLanguageButtons();
    try {
      const response = await fetch("data/techs.json", { cache: "no-cache" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      state.data = await response.json();
      prepareIcons();
      state.renderer = new window.TechViewport({ viewport: dom.viewport, canvas: dom.graphCanvas,
        nodes: dom.nodes, makeNode, label: languageText });
      buildDataIndexes();
      setupControls();
      registerEvents();
      renderGraph(true);
      dom.app.dataset.ready = "true";
    } catch (error) {
      console.error("Unable to load the technology snapshot", error);
      dom.fatalError.hidden = false;
    }
  }

  init();
})();

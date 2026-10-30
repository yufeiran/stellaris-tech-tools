/* One source of UI copy; EN and ZH are single-language, Both shows both texts. */
(() => {
  "use strict";
  const messages = {
    brand: ["科技图谱", "Tech Atlas"], localData: ["本地数据", "Local game data"],
    filters: ["筛选", "Filters"], reset: ["重置", "Reset"], tiers: ["科技等级", "Technology tier"],
    categories: ["科技类别", "Category"], traits: ["特性", "Properties"],
    rare: ["稀有科技", "Rare"], dangerous: ["危险科技", "Dangerous"], repeatable: ["循环科技", "Repeatable"],
    start: ["初始科技", "Starting technology"], insight: ["洞察科技", "Insight"], conditional: ["有特殊条件", "Conditional"],
    dlc: ["DLC · 扩展内容", "DLC content"], allDlc: ["全部 DLC", "All DLCs"], selectedDlc: ["我选择的 DLC", "Selected DLCs"],
    baseDlc: ["不启用 DLC", "No DLCs enabled"], excludeSeason: ["排除 Season 10", "Exclude Season 10"],
    chooseDlc: ["选择我拥有的 DLC", "Choose my DLCs"], all: ["全选", "Select all"], none: ["全不选", "Select none"],
    dlcNote: ["选择会记在此浏览器。安装文件 ≠ 已购买；联机时按房主 DLC。仍可能需要特定起源、帝国或事件。", "Saved in this browser. Installed files do not prove ownership; use the host’s DLCs in multiplayer. Origins, empire types or events may still be required."],
    prerequisite: ["前置关系", "Prerequisite"], chain: ["当前链路", "Selected chain"],
    canvas: ["可缩放科技树画布", "Zoomable technology tree"], fields: ["研究领域", "Research fields"],
    fieldNav: ["全部视图领域导航", "All-fields navigation"], controls: ["画布控制", "Canvas controls"],
    openFilters: ["打开筛选器", "Open filters"], zoomIn: ["放大", "Zoom in"], zoomOut: ["缩小", "Zoom out"],
    fit: ["适应画布", "Fit view"], map: ["科技树缩略图", "Technology minimap"], navigation: ["导航图", "Navigator"],
    drag: ["拖拽 移动画布", "Drag to pan"], wheel: ["滚轮 缩放", "Scroll to zoom"], doubleClick: ["双击 聚焦", "Double-click to focus"],
    keys: ["WASD / 方向键 移动", "WASD / arrows to pan"],
    details: ["科技详情", "Technology details"], close: ["关闭详情", "Close details"],
    description: ["科技说明", "Description"], prerequisites: ["前置科技", "Prerequisites"], successors: ["后续科技", "Leads to"],
    effects: ["直接效果", "Effects"], dlcConditions: ["DLC 条件", "DLC conditions"],
    dlcConditionNote: ["勾叉按左侧 DLC 选择判断；全部模式假定启用，并不代表已购买。帝国与事件条件仍由游戏判断。", "Checks use your DLC filter. All-DLC mode assumes enabled, not purchased. Empire and event conditions are still evaluated by the game."],
    assumed: ["假定启用", "Assumed enabled"], enabled: ["已选", "Selected"], disabled: ["未选", "Not selected"],
    rule: ["可用条件", "Availability rule"], gameplay: ["帝国 / 事件条件", "Empire / event conditions"],
    eligible: ["DLC 条件未阻止此科技", "Not blocked by DLC conditions"], blocked: ["DLC 条件未满足", "DLC conditions not met"],
    and: ["且", "AND"], or: ["或", "OR"], not: ["未启用", "NOT"],
    allRules: ["全部满足", "ALL OF"], anyRules: ["任一满足", "ANY OF"], notRule: ["不满足以下条件", "NOT"],
    unrestricted: ["无额外限制", "No additional restriction"], unavailable: ["不可用", "Unavailable"],
    noPrerequisite: ["无前置科技", "No prerequisite"], noSuccessor: ["没有直接后续科技", "No direct successor"],
    alternative: ["部分前置为替代路径（OR），不要求同时研究", "Some prerequisites are alternatives (OR), not all are required"],
    excluded: ["当前 DLC 筛选不包含此科技", "Excluded by the current DLC filter"],
    researchCost: ["研究成本", "Research cost"], weight: ["抽取权重", "Draw weight"], tier: ["等级", "Tier"],
    noDescription: ["暂无说明。", "No description available."], unlock: ["解锁", "Unlock"],
    noChinese: ["游戏文本未提供中文 · 显示英文/内部名", "Chinese text is unavailable in the game files"],
    noResults: ["没有找到匹配项，试试科技 ID 或英文关键词。", "No matches. Try a technology ID or another keyword."],
    noTechs: ["没有符合当前条件的科技", "No technologies match these filters"], loading: ["正在解析科技网络…", "Loading the technology network…"],
    loadingGame: ["读取本地游戏数据…", "Loading local game data…"],
    search: ["搜索科技", "Search technologies"], searchPlaceholder: ["搜索科技、建筑、效果、拼音、ID…", "Search technologies, buildings, effects, pinyin, IDs…"],
    match_variant: ["科技变体", "Variant"], match_category: ["类别", "Category"], match_description: ["科技说明", "Description"],
    match_effect: ["直接效果", "Effect"], match_building: ["建筑", "Building"], match_unlock: ["解锁内容", "Unlock"],
    match_upgrade: ["升级自", "Upgrades from"], match_unlockDescription: ["解锁说明", "Unlock description"],
    match_unlockEffect: ["解锁效果", "Unlock effect"], match_feature: ["特性", "Feature"],
    language: ["显示语言", "Display language"], help: ["使用说明", "Help"],
    helpTitle: ["在科技网络中导航", "Explore the technology network"],
    helpText: ["拖动画布移动；滚轮以指针位置为中心缩放。WASD 或方向键平移，按住连续移动。输入框内不会触发移动。选择节点会高亮前置与后续链路。", "Drag to pan and scroll to zoom around the pointer. Hold WASD or arrow keys to pan continuously. Typing in inputs does not move the canvas. Select a node to highlight its prerequisite and successor chains."],
    closePanels: ["关闭面板", "Close panels"], errorTitle: ["科技数据未能载入", "Could not load technology data"],
    errorText: ["请通过本项目的启动脚本打开页面，或重新生成本地数据。", "Open the project using its launcher, or regenerate the local game data."],
  };
  const bindings = [
    [".brand-copy strong", "brand"], [".panel-heading span", "filters"], ["#resetFilters", "reset"],
    ["#tierFilterLegend", "tiers"], ["#categoryFilterLegend", "categories"],
    ["#traitFilterLegend", "traits"], [".dlc-group legend", "dlc"],
    ...["rare", "dangerous", "repeatable", "start"].map(key => [`[data-flag="${key}"] ~ span:last-of-type`, key]),
    ["#dlcMode option[value=all]", "allDlc"], ["#dlcMode option[value=owned]", "selectedDlc"], ["#dlcMode option[value=base]", "baseDlc"],
    ["#excludeSeason10", "excludeSeason"], ["#dlcPicker .picker-label", "chooseDlc"],
    ["#dlcSelectAll", "all"], ["#dlcSelectNone", "none"], [".dlc-group > .filter-note", "dlcNote"],
    [".legend .required-label", "prerequisite"], [".legend .active-label", "chain"], ["#mobileFilter", "filters"],
    ["#minimapWrap > span", "navigation"], ["#gestureHint span:nth-child(1)", "drag"], ["#gestureHint span:nth-child(2)", "wheel"],
    ["#gestureHint span:nth-child(3)", "doubleClick"], ["#gestureHint span:nth-child(4)", "keys"],
    ["#dlcSection > .filter-note", "dlcConditionNote"], ["#helpPopover > strong", "helpTitle"], ["#helpPopover > p", "helpText"],
    ["#helpPopover dl div:nth-child(1) dd", "search"], ["#helpPopover dl div:nth-child(2) dd", "fit"],
    ["#helpPopover dl div:nth-child(3) dd", "closePanels"], ["#helpPopover dl div:nth-child(4) dd", "keys"],
    ["#fatalError strong", "errorTitle"], ["#fatalError p", "errorText"],
    ["#canvasStatus span:last-child", "loading"],
  ];
  const attributes = [
    [".brand", "aria-label", "brand"], [".language-switch", "aria-label", "language"],
    ["#areaTabs", "aria-label", "fields"], ["#filterPanel", "aria-label", "filters"],
    ["#searchInput", "aria-label", "search"], ["#searchInput", "placeholder", "searchPlaceholder"],
    ["#categoryFilter", "aria-label", "categories"], ["#dlcMode", "aria-label", "dlc"],
    ["#canvasShell", "aria-label", "canvas"], ["#viewport", "aria-label", "canvas"], ["#fieldJumps", "aria-label", "fieldNav"],
    [".zoom-tools", "aria-label", "controls"], ["#mobileFilter", "aria-label", "openFilters"],
    ["#zoomIn", "aria-label", "zoomIn"], ["#zoomOut", "aria-label", "zoomOut"], ["#fitView", "aria-label", "fit"],
    ["#zoomIn", "title", "zoomIn"], ["#zoomOut", "title", "zoomOut"], ["#fitView", "title", "fit"],
    ["#minimap", "aria-label", "map"], ["#detailPanel", "aria-label", "details"], ["#detailClose", "aria-label", "close"],
    ["#helpButton", "aria-label", "help"], ["#helpButton", "title", "help"],
  ];
  const text = (key, language) => (messages[key] || [key, key])[language === "en" ? 1 : 0];
  const pair = (zh, en, language) => language === "en" ? en : language === "zh" || zh === en ? zh : `${zh} · ${en}`;
  function apply(language) {
    for (const [selector, key] of bindings) for (const node of document.querySelectorAll(selector)) node.textContent = text(key, language);
    for (const [selector, attribute, key] of attributes) document.querySelector(selector)?.setAttribute(attribute, text(key, language));
    for (const node of document.querySelectorAll("[data-heading]")) {
      const key = node.dataset.heading;
      node.textContent = text(key, language);
      if (language === "bilingual") {
        const secondary = document.createElement("span"); secondary.textContent = ` ${text(key, "en")}`; node.append(secondary);
      }
    }
    for (const node of document.querySelectorAll("button[data-language]")) {
      node.textContent = node.dataset.language === "en" ? "EN" : node.dataset.language === "zh" ? (language === "en" ? "ZH" : "中") : (language === "en" ? "Both" : "双语");
    }
  }
  window.TechI18n = { text, pair, apply };
})();

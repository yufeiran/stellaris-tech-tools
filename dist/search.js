/* Cached, bilingual full-text search with weighted fields and small typo tolerance. */
(() => {
  "use strict";
  const normalize = value => String(value || "").toLowerCase().normalize("NFKC")
    .replace(/[\s_\-—–·:：.,，。/\\()[\]{}]+/g, " ").trim();
  const canonical = value => normalize(value)
    .replace(/化工厂/g, "化学厂")
    .replace(/chemical (?:factories|factory|plants)/g, "chemical plant")
    .replace(/合成|人工生产/g, "人造");
  let pinyinTable = {};
  const configure = table => { pinyinTable = table || {}; };
  const phoneticKey = value => String(value || "").toLowerCase().replace(/[üv]/g, "u")
    .normalize("NFD").replace(/[\u0300-\u036f]/g, "").replace(/[\s'’\-0-5]+/g, "");
  const initialKinds = new Set(["name", "variant", "category", "building", "unlock", "upgrade", "effect", "unlockEffect"]);

  function build(tech) {
    const fields = [];
    function add(kind, zh, en, weight, extra = "", phoneticText = zh) {
      if (!zh && !en && !extra) return;
      const source = String(phoneticText || "").trim();
      const readings = [source, ...(source.includes("化学厂") ? [source.replace(/化学厂/g, "化工厂")] : [])];
      const pinyin = readings.filter(value => pinyinTable[value]).map(value => {
        const [spaced, initials] = pinyinTable[value];
        const syllables = spaced.split(" ").map(phoneticKey), starts = [];
        let offset = 0;
        for (const syllable of syllables) { starts.push(offset); offset += syllable.length; }
        return { full: syllables.join(""), initials, starts };
      });
      fields.push({ kind, zh: zh || en || extra, en: en || extra, weight,
        values: [...new Set([zh, en, extra].filter(Boolean).map(canonical))], pinyin });
    }
    add("name", tech.name.zh, tech.name.en, 0, tech.id);
    for (const variant of tech.variants || []) {
      add("variant", variant.name.zh, variant.name.en, 6, variant.key);
      add("description", variant.description.zh, variant.description.en, 56);
    }
    add("category", tech.categoryName.zh, tech.categoryName.en, 30, tech.category);
    add("description", tech.description.zh, tech.description.en, 56);
    for (const effect of tech.effects || []) add("effect", `${effect.zh || effect.en} ${effect.value || ""}`, `${effect.en || ""} ${effect.value || ""}`, 34, `${effect.key} ${effect.value}`, effect.zh);
    for (const item of tech.unlocks || []) {
      const kind = item.kind === "building" ? "building" : "unlock";
      add(kind, item.zh, item.en, 14, item.key);
      add("unlockDescription", item.description?.zh, item.description?.en, 48);
      for (const parent of item.upgradeFrom || []) add("upgrade", parent.zh, parent.en, 22, parent.key);
      for (const effect of item.effects || []) add("unlockEffect", `${effect.zh || effect.en} ${effect.value || ""}`, `${effect.en || ""} ${effect.value || ""}`, 38, `${effect.key} ${effect.value}`, effect.zh);
    }
    add("feature", "", "", 66, (tech.featureFlags || []).join(" "));
    const words = [...new Set(fields.filter(field => ["name", "variant", "building", "unlock"].includes(field.kind))
      .flatMap(field => field.values.flatMap(value => value.split(" "))).filter(word => /^[a-z]{4,24}$/.test(word)))];
    return { fields, words };
  }

  function distance(a, b, cap) {
    if (Math.abs(a.length - b.length) > cap) return cap + 1;
    let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
    for (let i = 1; i <= a.length; i++) {
      const row = [i];
      for (let j = 1; j <= b.length; j++) row.push(Math.min(row[j - 1] + 1, prev[j] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1)));
      if (Math.min(...row) > cap) return cap + 1;
      prev = row;
    }
    return prev[b.length];
  }

  function match(index, rawQuery) {
    const query = canonical(rawQuery), tokens = query.split(" ").filter(Boolean);
    if (!query) return { score: Infinity, field: null, tokens };
    const py = phoneticKey(rawQuery);
    const usePinyin = /^[a-z]{2,64}$/.test(py);
    let best = { score: Infinity, field: null, tokens };
    const tokenHits = new Map(tokens.map(token => [token, []]));
    for (const field of index.fields) {
      for (const value of field.values) {
        const at = value.indexOf(query);
        if (at >= 0) {
          const score = field.weight + (value === query ? 0 : at === 0 ? 4 : 10) + Math.min(at * .01, 3);
          if (score < best.score) best = { score, field, tokens };
        }
        if (tokens.length > 1) for (const token of tokens) if (value.includes(token)) tokenHits.get(token).push(field);
      }
      if (usePinyin) for (const reading of field.pinyin) {
        let at = reading.full.indexOf(py);
        while (at >= 0 && !reading.starts.includes(at)) at = reading.full.indexOf(py, at + 1);
        if (py.length >= 3 && at >= 0) {
          const score = field.weight + (reading.full === py ? 16 : at === 0 ? 18 : 22);
          if (score < best.score) best = { score, field, tokens, pinyin: true, hanIndex: reading.starts.indexOf(at) };
        }
        if (initialKinds.has(field.kind) && reading.initials.length <= 40 && py.length <= 12) {
          const initialAt = reading.initials.indexOf(py);
          if (initialAt === 0 || py.length >= 3 && initialAt > 0) {
            const score = field.weight + (reading.initials === py ? 12 : initialAt === 0 ? 28 : 32);
            if (score < best.score) best = { score, field, tokens, pinyin: true, hanIndex: initialAt };
          }
        }
      }
    }
    if (tokens.length > 1 && [...tokenHits.values()].every(hits => hits.length)) {
      const fields = [...new Set([...tokenHits.values()].flat())];
      fields.sort((a, b) => tokens.filter(token => b.values.some(value => value.includes(token))).length
        - tokens.filter(token => a.values.some(value => value.includes(token))).length || a.weight - b.weight);
      const score = Math.max(...[...tokenHits.values()].map(hits => Math.min(...hits.map(field => field.weight)))) + 8;
      if (score < best.score) best = { score, field: fields[0], tokens };
    }
    if (!Number.isFinite(best.score) && /^[a-z]{4,24}$/.test(query)) {
      const cap = query.length > 8 ? 2 : 1;
      for (const word of index.words) {
        const edits = distance(query, word, cap);
        if (edits <= cap && 42 + edits * 7 < best.score) {
          best = { score: 42 + edits * 7, field: index.fields.find(field => field.values.some(value => value.includes(word))), tokens };
        }
      }
    }
    return best;
  }

  function snippet(result, language) {
    if (!result.field) return "";
    function excerpt(value) {
      const content = String(value || "").replace(/\s+/g, " ").trim();
      const at = result.pinyin && language !== "en"
        ? [...content.matchAll(/[\u3400-\u9fff]/g)][result.hanIndex]?.index || 0
        : result.tokens.map(token => canonical(content).indexOf(token)).find(index => index >= 0) || 0;
      const start = Math.max(0, at - 24), limit = 96;
      return `${start ? "…" : ""}${content.slice(start, start + limit)}${content.length > start + limit ? "…" : ""}`;
    }
    const zh = excerpt(result.field.zh), en = excerpt(result.field.en) || "No English text available.";
    return language === "en" ? en : language === "zh" || zh === en ? zh : `${zh} · ${en}`;
  }
  window.TechSearch = { build, match, snippet, normalize, configure };
})();

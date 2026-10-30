//! One shared tree per field/size, with a selected prerequisite closure. No
//! independent route window/layout is generated for each technology.
use crate::{
    catalog, diagnostics, gui,
    planning::{self, t, Tech, AREAS, MAX_GOALS},
    script::{self, Result},
    sources::Sources,
};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

pub type Localisation = BTreeMap<String, BTreeMap<String, String>>;
pub(crate) fn safe_localisation_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}
pub struct View {
    pub effects: Vec<String>,
    pub buttons: Vec<String>,
    pub events: Vec<String>,
    pub getters: Vec<String>,
    pub gui: Vec<String>,
    pub gfx: Vec<String>,
    pub assets: BTreeMap<String, Vec<u8>>,
    pub localisation: Localisation,
}
impl View {
    fn loc(&mut self, key: impl Into<String>, zh: impl Into<String>, en: impl Into<String>) {
        let key = key.into();
        self.localisation
            .get_mut("english")
            .unwrap()
            .insert(key.clone(), en.into());
        self.localisation
            .get_mut("simp_chinese")
            .unwrap()
            .insert(key, zh.into());
    }
}
pub fn option(name: &str, effect: &str, trigger: &str) -> String {
    t!(
        "option={name=@name@ @close@ @trigger@ @effect@}",
        name = name,
        close = if name == "tte_close" {
            "default_hide_option=yes"
        } else {
            ""
        },
        trigger = if trigger.is_empty() {
            String::new()
        } else {
            format!("trigger={{ {trigger} }}")
        },
        effect = if effect.is_empty() {
            String::new()
        } else {
            format!("hidden_effect={{ {effect} }}")
        }
    )
}
pub fn event(
    id: &str,
    title: &str,
    desc: &str,
    choices: &[String],
    window: &str,
    option_gui: &str,
    immediate: &str,
) -> String {
    t!("country_event={id=@id@ title=@title@ desc=@desc@ is_triggered_only=yes diplomatic=yes custom_gui=@window@ custom_gui_option=@option@ @immediate@\n@choices@\n}",id=id,title=title,desc=desc,window=window,option=option_gui,immediate=if immediate.is_empty(){String::new()}else{format!("immediate={{ {immediate} }}")},choices=choices.join("\n"))
}
fn jump(id: &str) -> String {
    format!("country_event={{id={id}}}")
}
fn button(name: &str, potential: &str, effect: &str, tip: &str) -> String {
    t!("@name@={potential={from={is_ai=no @potential@}} allow={from={is_ai=no}} effect={@tip@ from={hidden_effect={@effect@}}}}",name=name,potential=potential,tip=if tip.is_empty(){String::new()}else{format!("custom_tooltip={tip}")},effect=effect)
}
fn chosen_event(index: usize) -> String {
    format!("tte.{}", 1000 + index)
}
fn graph_event(mode: usize, area: usize) -> String {
    format!("tte.{}", 900 + mode * 10 + area)
}
fn category(tech: &Tech) -> String {
    if tech.block.all("category").count() > 1 {
        return "tte_category_other".into();
    }
    tech.block
        .first("category")
        .and_then(|v| v.entries().first())
        .and_then(|e| e.value.atom())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or("tte_category_other")
        .into()
}
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in s.chars() {
        if skip {
            skip = false;
            continue;
        }
        if c == '§' {
            skip = true;
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out
}
pub(crate) fn compact_name(s: &str, width: usize, lines: usize) -> String {
    // Dynamic localisation is evaluated by the game after this build. Never
    // cut an engine reference/icon into a different or unterminated token.
    if s.contains('$') || s.contains('[') || s.contains('£') {
        return s.to_owned();
    }
    let text: Vec<char> = plain(s)
        .replace("\\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    let units = |c: char| {
        if (0x2e80..=0xffff).contains(&(c as u32)) {
            2
        } else {
            1
        }
    };
    let mut pos = 0;
    let mut rows: Vec<String> = Vec::new();
    while pos < text.len() && rows.len() < lines {
        let start = pos;
        let mut size = 0;
        while pos < text.len() && size + units(text[pos]) <= width {
            size += units(text[pos]);
            pos += 1;
        }
        if pos < text.len() {
            if let Some(space) = (start..pos)
                .rev()
                .find(|i| text[*i] == ' ' && *i > start + (pos - start) / 3)
            {
                pos = space;
            }
        }
        rows.push(text[start..pos].iter().collect::<String>().trim().into());
        while pos < text.len() && text[pos] == ' ' {
            pos += 1;
        }
    }
    if pos < text.len() {
        if let Some(last) = rows.last_mut() {
            while last.chars().map(units).sum::<usize>() + 3 > width {
                last.pop();
            }
            last.push_str("...");
        }
    }
    rows.join("\\n")
}
fn initial(s: &str, pinyin: bool) -> char {
    static INDEX: std::sync::OnceLock<Json> = std::sync::OnceLock::new();
    let v = INDEX.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/pinyin-initials.json")).unwrap()
    });
    let start = v["start"].as_u64().unwrap() as u32;
    let letters = v["initials"].as_str().unwrap().as_bytes();
    for c in plain(s).chars() {
        if c.is_ascii_alphabetic() {
            return c.to_ascii_uppercase();
        }
        if pinyin && (c as u32) >= start {
            let i = (c as u32 - start) as usize;
            if i < letters.len() {
                return letters[i] as char;
            }
        }
        if c.is_alphanumeric() {
            return '#';
        }
    }
    '#'
}
fn dds(width: usize, height: usize, color: [u8; 4]) -> Vec<u8> {
    let mut out = b"DDS ".to_vec();
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x100f;
    h[2] = height as u32;
    h[3] = width as u32;
    h[4] = (width * 4) as u32;
    h[18] = 32;
    h[19] = 0x41;
    h[21] = 32;
    h[22] = 0xff0000;
    h[23] = 0xff00;
    h[24] = 0xff;
    h[25] = 0xff000000;
    h[26] = 0x1000;
    for v in h {
        out.extend(v.to_le_bytes());
    }
    for _ in 0..width * height {
        out.extend([color[2], color[1], color[0], color[3]]);
    }
    out
}
// Percent-width routing avoids native dual-axis scrollbar failures.
fn line(
    out: &mut View,
    name: &str,
    first: (i64, i64),
    last: (i64, i64),
    widgets: &mut Vec<String>,
) {
    if first == last {
        return;
    }
    let x = first.0.min(last.0);
    let y = first.1.min(last.1);
    fn sprite(out: &mut View, vertical: bool, length: i64) -> String {
        let axis = if vertical { "v" } else { "h" };
        let key = format!("GFX_tte_native_line_{axis}_{length}");
        let texture = format!("gfx/interface/tte_native_line_{axis}_{length}.dds");
        if !out.assets.contains_key(&texture) {
            let (w, h) = if vertical { (2, length) } else { (length, 2) };
            out.assets.insert(
                texture.clone(),
                dds(w as usize, h as usize, [56, 111, 132, 180]),
            );
            out.gfx.push(format!(
                "spriteType={{name={key} texturefile=\"{texture}\"}}"
            ));
        }
        key
    }
    // Reuse unscaled thin sprites by axis/length, as in the public atlas.
    // Limit individual vertical textures to 2048 pixels.
    if first.0 == last.0 {
        let mut remaining = (first.1 - last.1).abs();
        let mut offset = 0;
        while remaining > 0 {
            let length = remaining.min(2048);
            let key = sprite(out, true, length.max(2));
            widgets.push(t!("containerWindowType={name=@name@_@offset@ position={x=@x@% y=@y@} size={width=2 height=2} iconType={name=stroke spriteType=@sprite@ alwaysTransparent=yes}}",name=name,offset=offset,x=x,y=y+offset,sprite=key));
            remaining -= length;
            offset += length;
        }
    } else {
        let percent = (first.0 - last.0).abs();
        let width = |viewport: i64| ((viewport - 376).max(0) * percent / 100).max(2);
        let base = sprite(out, false, width(640));
        let mut variants = String::new();
        for (min, max) in RESOLUTION_BANDS {
            let key = sprite(out, false, width(min));
            variants.push_str(&format!(
                " if_scaled_resolution={{min_width={min} max_width={max} spriteType={key}}}"
            ));
        }
        // Native icons parse x=50% as 50px. Only the parent container performs
        // percentage positioning; the fixed-size sprite stays at its origin.
        widgets.push(t!("containerWindowType={name=@name@ position={x=@x@% y=@y@} size={width=2 height=2} iconType={name=stroke spriteType=@sprite@ @variants@ alwaysTransparent=yes}}",name=name,x=x,y=y,sprite=base,variants=variants));
    }
}
fn graph_option(curator: &str) -> Result<String> {
    let base = gui::option_class(curator)?;
    let parsed = script::parse(&base)?;
    let root = &parsed.entries()[0];
    let b = gui::named(&root.value, "option_button")?;
    let child = gui::property(&base[b.span.clone()], "size", "{x=292 y=33}")?;
    let base = gui::edits(&base, vec![(b.span.start, b.span.end, child)])?;
    gui::property(
        &gui::property(&base, "name", "tte_graph_option")?,
        "size",
        "{width=294 height=35}",
    )
}
fn goal_controls(shell: &str) -> Result<String> {
    let parsed = script::parse(shell)?;
    let panel = gui::named(&parsed.entries()[0].value, "EVENT_DIPLO")?;
    let list = gui::named(&panel.value, "option_list")?;
    let raw = gui::property(&shell[list.span.clone()], "position", "{x=0 y=316}")?;
    let raw = gui::property(&raw, "size", "{x=294 y=420}")?;
    let shell = gui::edits(shell, vec![(list.span.start, list.span.end, raw)])?;
    let mut controls = String::from("containerWindowType={name=ttegs_controls position={x=-314 y=52} orientation=upper_right size={width=294 height=310} effectbuttonType={name=ttegs_selected position={x=0 y=0} size={x=292 y=50} quadTextureSprite=GFX_tiled_window_transparent font=\"cg_16b\" multiline=yes buttonText=ttegs_selected_label effect=ttegs_selected_tip}");
    for slot in 1..=MAX_GOALS {
        controls.push_str(&format!("effectbuttonType={{name=ttegs_slot_{slot} position={{x=0 y={}}} size={{x=292 y=44}} quadTextureSprite=GFX_tiling_button_standard font=\"cg_16b\" multiline=yes buttonText=ttegs_slot_{slot} effect=ttegs_slot_{slot} clicksound=click}}",52+(slot-1)*46));
    }
    controls.push_str("effectbuttonType={name=ttegs_remove position={x=0 y=286} size={x=292 y=28} quadTextureSprite=GFX_tiling_button_standard font=\"cg_16b\" buttonText=tte_untrack_selected effect=ttegs_remove clicksound=click}} ");
    gui::append(&shell, &controls)
}

fn compile_goal_slots(out: &mut View, techs: &[Tech]) -> Result<()> {
    out.events.insert(0, "namespace=ttegoal".into());
    let ids = techs.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    let raw = crate::goal_slots::scripts(&ids, "tte_viewed_")?;
    let parsed = script::parse(&raw)?;
    for e in parsed.entries() {
        if e.key.as_deref() == Some("country_event") {
            out.events.push(raw[e.span.clone()].into());
        } else {
            out.effects.push(raw[e.span.clone()].into());
        }
    }
    let checks=techs.iter().map(|t|format!("if={{limit={{has_country_flag=ttegs_pending_{} {}}} set_country_flag=ttegs_pending_valid}} if={{limit={{has_country_flag=tte_viewed_{} {}}} set_country_flag=ttegs_selected_valid}}",t.id,t.addable(),t.id,t.addable())).collect::<String>();
    out.effects
        .push("ttegs_recheck={country_event={id=ttegoal.14}}".into());
    out.events.push(format!("country_event={{id=ttegoal.14 hide_window=yes is_triggered_only=yes immediate={{remove_country_flag=ttegs_pending_valid remove_country_flag=ttegs_selected_valid {checks}}}}}"));
    for (name, flag) in [
        ("GetTTEGSSelected", "tte_viewed_"),
        ("GetTTEGSPending", "ttegs_pending_"),
    ] {
        let branches = techs
            .iter()
            .map(|t| {
                format!(
                    "text={{trigger={{has_country_flag={flag}{}}} localization_key={}}}",
                    t.id, t.id
                )
            })
            .collect::<String>();
        out.getters.push(format!(
            "defined_text={{name={name} random=no {branches} default=ttegs_pick}}"
        ));
    }
    let slots=(1..=MAX_GOALS).map(|n|format!("text={{trigger={{check_variable={{which=ttegs_destination value={n}}}}} localization_key=ttegs_occupant_{n}}}")).collect::<String>();
    out.getters.push(format!(
        "defined_text={{name=GetTTEGSOccupant random=no {slots} default=tte_goal_none}}"
    ));
    let numbers=(1..=MAX_GOALS).map(|n|format!("text={{trigger={{check_variable={{which=ttegs_destination value={n}}}}} localization_key=ttegs_number_{n}}}")).collect::<String>();
    out.getters.push(format!(
        "defined_text={{name=GetTTEGSSlot random=no {numbers} default=tte_goal_none}}"
    ));
    for n in 1..=MAX_GOALS {
        out.loc(
            format!("ttegs_occupant_{n}"),
            format!("[Root.GetTTEGoal{n}]"),
            format!("[Root.GetTTEGoal{n}]"),
        );
        out.loc(format!("ttegs_number_{n}"), n.to_string(), n.to_string());
        out.buttons.push(t!("ttegs_slot_@n@={potential={from={is_ai=no}} allow={hidden_trigger={from={is_ai=no has_country_flag=ttegs_selected_valid NOT={has_country_flag=ttegs_dialog}}}} effect={custom_tooltip=ttegs_tip_@n@ from={hidden_effect={ttegs_capture=yes set_variable={which=ttegs_destination value=@n@} ttegs_request=yes}}}}",n=n));
    }
    out.loc(
        "ttegs_selected_label",
        "[From.GetTTEGSSelected]",
        "[From.GetTTEGSSelected]",
    );
    let tips=techs.iter().map(|t|format!("text={{trigger={{has_country_flag=tte_viewed_{}}} localization_key=tte_detail_{}}}",t.id,t.id)).collect::<String>();
    out.getters.push(format!(
        "defined_text={{name=GetTTEGSSelectedTip random=no {tips} default=ttegs_pick}}"
    ));
    out.loc(
        "ttegs_selected_details",
        "[From.GetTTEGSSelectedTip]",
        "[From.GetTTEGSSelectedTip]",
    );
    out.buttons.push(button(
        "ttegs_selected_tip",
        "",
        "",
        "ttegs_selected_details",
    ));
    out.buttons.push(button(
        "ttegs_remove",
        "has_country_flag=tte_has_view_selection NOT={has_country_flag=ttegs_dialog}",
        "tte_untrack_selected=yes",
        "tte_untrack_selected",
    ));
    let labels: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_str(include_str!("../assets/goal-slots-ui.json"))
            .map_err(|e| e.to_string())?;
    for (lang, rows) in labels {
        let dest = out.localisation.entry(lang).or_default();
        for (key, value) in rows {
            if key == "slot" || key == "tip" {
                for n in 1..=MAX_GOALS {
                    dest.insert(
                        format!("ttegs_{key}_{n}"),
                        value.replace("{n}", &n.to_string()),
                    );
                }
            } else {
                dest.insert(format!("ttegs_{key}"), value);
            }
        }
    }
    out.events.push("country_event={id=ttegoal.1 title=ttegs_confirm_title desc=ttegs_confirm_desc picture=GFX_evt_physics_research is_triggered_only=yes option={name=ttegs_confirm hidden_effect={ttegs_commit=yes}} option={name=ttegs_cancel default_hide_option=yes hidden_effect={ttegs_cancel=yes}}}".into());
    Ok(())
}
pub fn compile(techs: &[Tech], report: &Json, sources: &Sources) -> Result<View> {
    let curator_source = gui::effective(
        sources,
        &["enclave_curator_window", "enclave_curator_option"],
    )?;
    let curator = curator_source.as_str();
    let mut out = View {
        effects: Vec::new(),
        buttons: Vec::new(),
        events: vec![
            "namespace=tte".into(),
            "namespace=tteview".into(),
            "namespace=tteselect".into(),
            "namespace=ttepreviewsmall".into(),
            "namespace=ttepreviewlarge".into(),
        ],
        getters: Vec::new(),
        gui: vec![
            gui::option_class(curator)?,
            graph_option(curator)?,
            gui::shell(curator, "tte_home_window", false)?,
        ],
        gfx: Vec::new(),
        assets: BTreeMap::new(),
        localisation: serde_json::from_str(include_str!("../assets/view-base.json"))
            .map_err(|e| e.to_string())?,
    };
    out.assets.insert(
        "gfx/interface/tte_caption_blank.dds".into(),
        dds(3, 1, [0, 0, 0, 0]),
    );
    out.gfx.push("corneredTileSpriteType={name=GFX_tte_caption_blank texturefile=\"gfx/interface/tte_caption_blank.dds\" borderSize={x=0 y=0} noOfFrames=3}".into());
    out.loc("tte_intro","§H科研目标 · 从上到下优先§!\\n1. [Root.GetTTEGoal1]\\n2. [Root.GetTTEGoal2]\\n3. [Root.GetTTEGoal3]\\n4. [Root.GetTTEGoal4]\\n5. [Root.GetTTEGoal5]\\n[Root.GetTTEAutomation]\\n手动：关闭齿轮，自由选卡，参考选卡建议。\\n自动：开启要托管学科的齿轮。","§HResearch goals · top first§!\\n1. [Root.GetTTEGoal1]\\n2. [Root.GetTTEGoal2]\\n3. [Root.GetTTEGoal3]\\n4. [Root.GetTTEGoal4]\\n5. [Root.GetTTEGoal5]\\n[Root.GetTTEAutomation]\\nManual: gears off; choose freely with advice.\\nAuto: gears on for the fields you want automated.");
    for (key,zh,en) in [
        ("tte_select_sidebar","科研目标与帮助","Research goals and help"),("tte_category_other","其他","Other"),("tte_cross_badge","跨科","Cross"),("tte_settings","设置","Settings"),("tte_help","帮助","Help"),
        ("tte_clear_path","取消路线高亮","Clear route highlight"),("tte_path_marker",">",">"),("tte_select_names","按英文名称 · A–Z","English names · A–Z"),("tte_select_pinyin","按中文名称 · 拼音首字母","Chinese names · pinyin initials"),("tte_select_purpose","按学科与专业","Browse by field and category"),
        ("tte_select_offered","当前科研候选（含研究中）","Current research options (including active research)"),("tte_select_tracked","已追踪目标","Tracked goals"),("tte_select_recent","最近查看（最多 8 项）","Recently viewed (up to eight)"),("tte_select_help","按用途、学科、专业、英文或拼音首字母选择科技。点击科技查看前置路线，再决定是否追踪。\\n\\n$tte_intro$","Browse by purpose, field, category, English or pinyin initial. Click a technology to inspect its prerequisites, then choose whether to track it.\\n\\n$tte_intro$"),
        ("tte_plan_no_goal","当前未推进目标","No current goal being advanced"),("tte_no_research","未在研究","No research in progress"),("tte_plan_paused","目标优先已暂停；保留原版或手动选择。","Goal priority is paused; vanilla or manual choices remain."),
        ("tte_plan_summary","$tte_intro$\\n\\n§H物理学§! · [Root.GetTTECurrent_physics]\\n[Root.GetTTEPlanGoal_physics]\\n[Root.GetTTEPlanRole_physics]\\n\\n§H社会学§! · [Root.GetTTECurrent_society]\\n[Root.GetTTEPlanGoal_society]\\n[Root.GetTTEPlanRole_society]\\n\\n§H工程学§! · [Root.GetTTECurrent_engineering]\\n[Root.GetTTEPlanGoal_engineering]\\n[Root.GetTTEPlanRole_engineering]","$tte_intro$\\n\\n§HPhysics§! · [Root.GetTTECurrent_physics]\\n[Root.GetTTEPlanGoal_physics]\\n[Root.GetTTEPlanRole_physics]\\n\\n§HSociety§! · [Root.GetTTECurrent_society]\\n[Root.GetTTEPlanGoal_society]\\n[Root.GetTTEPlanRole_society]\\n\\n§HEngineering§! · [Root.GetTTECurrent_engineering]\\n[Root.GetTTEPlanGoal_engineering]\\n[Root.GetTTEPlanRole_engineering]"),
        ("tte_full_legend","绿色 已研究 / 蓝色 研究中 / 黄色 候选 · > 当前查看的前置路线（可选分支只需一支） · 滚轮连续浏览","Green: researched / Blue: researching / Yellow: offered · > Prerequisites for the selected technology (one alternative suffices) · Scroll to browse"),
        ("tte_generated_help","这是为本次播放集生成的本地适配版本。请只启用一个规划器，并排在来源 Mod 之后。来源科技或播放集组合更新后，请重新运行适配工具。\\n特殊事件、设置和自定义条件可能未知，以游戏判定为准。","This local planner was generated for this playset. Enable only one planner, after the source mods. Rerun the adapter when their technologies or the source combination change.\\nSpecial events, settings and custom conditions may be unknown; the game determines eligibility.")
    ]{out.loc(key,zh,en);}
    out.loc(
        "tte_open_planner_label",
        "打开科研规划器",
        "Open Research Planner",
    );
    out.loc(
        "tte_unknown_reference",
        "未识别的前置引用：",
        "Missing external prerequisites: ",
    );
    for (area, zh, en) in [
        ("physics", "物理学", "Physics"),
        ("society", "社会学", "Society"),
        ("engineering", "工程学", "Engineering"),
    ] {
        out.loc(format!("tte_area_{area}"), zh, en);
        out.loc(
            format!("tte_jump_{area}"),
            format!("直达 · {zh}"),
            format!("Jump · {en}"),
        );
    }
    for (suffix, zh, en) in [
        ("need_conditions", "条件不符", "Requirements not met"),
        ("need_prerequisites", "等待前置科技", "Needs prerequisites"),
        (
            "need_megastructure",
            "需建造或修复巨构",
            "Build or repair a megastructure",
        ),
        ("need_wonders", "需选择星河奇迹", "Needs Galactic Wonders"),
        ("need_tier", "科技等级未解锁", "Tier not unlocked"),
        ("need_special", "等待特殊解锁", "Needs a special unlock"),
        (
            "wait_draw",
            "等待候选或其他条件",
            "Awaiting draw or other conditions",
        ),
    ] {
        out.loc(
            format!("tte_{suffix}"),
            format!("§L{zh}§!"),
            format!("§L{en}§!"),
        );
    }
    let any_goal = techs
        .iter()
        .map(|t| {
            format!(
                "text={{trigger={{has_country_flag=tte_goal_{}}} localization_key={}}}",
                t.id, t.id
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    out.getters.push(format!(
        "defined_text={{name=GetTTEGoal random=no {any_goal} default=tte_goal_none}}"
    ));
    for slot in 1..=MAX_GOALS {
        let texts=techs.iter().map(|tech|t!("text={trigger={has_country_flag=tte_goal_@key@ check_variable={which=tte_rank_@key@ value=@slot@}} localization_key=tte_goal_entry_@key@}",key=tech.id,slot=slot)).collect::<Vec<_>>().join(" ");
        out.getters.push(t!(
            "defined_text={name=GetTTEGoal@slot@ random=no @texts@ default=tte_goal_none}",
            slot = slot,
            texts = texts
        ));
        out.loc(
            format!("tte_goal_slot_{slot}"),
            format!("目标科技 {slot} · [Root.GetTTEGoal{slot}]"),
            format!("Goal {slot} · [Root.GetTTEGoal{slot}]"),
        );
        let jumps=techs.iter().enumerate().map(|(i,tech)|t!("if={limit={has_country_flag=tte_goal_@key@ check_variable={which=tte_rank_@key@ value=@slot@}} @jump@}",key=tech.id,slot=slot,jump=jump(&chosen_event(i)))).collect::<Vec<_>>().join(" ");
        out.effects.push(t!(
            "tte_open_tracked_@slot@={@jumps@}",
            slot = slot,
            jumps = jumps
        ));
    }
    out.getters.push("defined_text={name=GetTTEAutomation random=no text={trigger={has_country_flag=tte_automation} localization_key=tte_auto_on} default=tte_auto_off}".into());
    let clear_view = techs
        .iter()
        .map(|tech| {
            t!(
                "remove_country_flag=tte_view_@key@ remove_country_flag=tte_viewed_@key@",
                key = tech.id
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let clear_view = format!("remove_country_flag=tte_has_view_selection remove_country_flag=ttegs_selected_valid {clear_view}");
    out.effects
        .push("tte_view_clear={country_event={id=tteview.1}}".into());
    out.events.push(format!("country_event={{id=tteview.1 hide_window=yes is_triggered_only=yes immediate={{ {clear_view} }} }}"));
    let mut home = vec![
        option("tte_full", "tte_open_atlas=yes", ""),
        option("ttegs_browse", &jump("tteselect.1"), ""),
        option("tte_plan_details", &jump("tte.5"), ""),
    ];
    for slot in 1..=MAX_GOALS {
        home.push(option(&format!("tte_goal_slot_{slot}"),&t!("if={limit={has_country_flag=tte_slot_@slot@} tte_open_tracked_@slot@=yes} else={tte_open_atlas=yes}",slot=slot),""));
    }
    home.extend([
        option(
            "tte_pause",
            &format!("remove_country_flag=tte_automation {}", jump("tte.1")),
            "has_country_flag=tte_automation",
        ),
        option(
            "tte_resume",
            &format!("set_country_flag=tte_automation {}", jump("tte.1")),
            "has_country_flag=tte_has_goal NOT={has_country_flag=tte_automation}",
        ),
        option(
            "tte_clear",
            "set_country_flag=tte_reopen_clear tte_clear_goal=yes",
            "has_country_flag=tte_has_goal",
        ),
        option("tte_settings", &jump("tte.6"), ""),
        option("tte_close", "", ""),
    ]);
    out.events.push(event(
        "tte.1",
        "tte_title",
        "tte_intro",
        &home,
        "tte_home_window",
        "tte_option",
        "ttegs_cancel=yes tte_refresh=yes",
    ));
    out.events.push(event(
        "tte.5",
        "tte_plan_details",
        "tte_plan_summary",
        &[
            option("tte_refresh_label", &jump("tte.5"), ""),
            option("tte_back", &jump("tte.1"), ""),
            option("tte_close", "", ""),
        ],
        "tte_home_window",
        "tte_option",
        "tte_refresh=yes tte_explain_plan=yes",
    ));
    out.events.push(event(
        "tte.6",
        "tte_settings",
        "tte_generated_help",
        &[
            option("tte_help", &jump("tte.7"), ""),
            option("tte_back", &jump("tte.1"), ""),
            option("tte_close", "", ""),
        ],
        "tte_home_window",
        "tte_option",
        "",
    ));
    out.events.push(event(
        "tte.7",
        "tte_help",
        "tte_generated_help",
        &[
            option("tte_back", &jump("tte.1"), ""),
            option("tte_close", "", ""),
        ],
        "tte_home_window",
        "tte_option",
        "",
    ));
    let mut graph_jumps = Vec::new();
    for mode in 0..2 {
        for a in 0..3 {
            graph_jumps.push(t!("@op@={limit={check_variable={which=tte_atlas_mode value=@mode@} check_variable={which=tte_atlas_area value=@area@}} @jump@}",op=if mode==0&&a==0{"if"}else{"else_if"},mode=mode,area=a,jump=jump(&graph_event(mode,a))));
        }
    }
    graph_jumps.push(format!("else={{ {} }}", jump("tte.900")));
    out.effects
        .push(format!("tte_open_atlas={{ {} }}", graph_jumps.join(" ")));
    // GUI effect buttons do not dismiss an existing native event. Selecting a
    // card must only change flags: opening another visible event here stacks
    // windows and resets scrolling. Event OPTIONS below perform navigation.
    for (name, action) in [
        ("tte_inspect_selected", "inspect"),
        ("tte_track_selected", "track"),
        ("tte_untrack_selected", "untrack"),
    ] {
        let calls = techs
            .iter()
            .enumerate()
            .map(|(i, tech)| {
                let body = match action {
                    "inspect" => jump(&chosen_event(i)),
                    "track" => format!("tte_target_{}=yes", tech.id),
                    _ => format!("tte_remove_{}=yes", tech.id),
                };
                t!(
                    "if={limit={has_country_flag=tte_viewed_@key@} @body@}",
                    key = tech.id,
                    body = body
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let fallback = if action == "inspect" {
            "if={limit={NOT={has_country_flag=tte_has_view_selection}} tte_open_atlas=yes}"
        } else {
            ""
        };
        out.effects.push(format!("{name}={{ {calls} {fallback} }}"));
    }
    out.loc(
        "tte_inspect_selected",
        "查看选中科技",
        "Inspect selected technology",
    );
    out.loc(
        "tte_track_selected",
        "追踪选中科技",
        "Track selected technology",
    );
    out.loc(
        "tte_untrack_selected",
        "停止追踪选中科技",
        "Untrack selected technology",
    );
    out.loc("tte_pick_first", "点击卡片高亮前置；悬停查看详情，然后使用右侧按钮追踪。", "Click a card to highlight prerequisites; hover for details, then track it using the buttons on the right.");
    compile_goal_slots(&mut out, techs)?;
    out.buttons.push(button("tte_open", "", &jump("tte.1"), ""));
    let parents: BTreeMap<_, _> = techs
        .iter()
        .map(|tech| {
            (
                tech.id.clone(),
                catalog::ids(&tech.condition)
                    .into_iter()
                    .filter(|id| techs.iter().any(|t| &t.id == id))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let mut icons = BTreeMap::new();
    for (i, tech) in techs.iter().enumerate() {
        let k = &tech.id;
        let e = chosen_event(i);
        let select = format!("tte_select_node_{k}=yes");
        out.effects.push(t!("tte_select_node_@key@={tte_view_clear=yes set_country_flag=tte_has_view_selection set_country_flag=tte_viewed_@key@ if={limit={@addable@} set_country_flag=ttegs_selected_valid} tte_view_visit_@key@=yes tte_remember_@key@=yes}",key=k,addable=tech.addable()));
        let area = AREAS.iter().position(|a| a == &tech.area).unwrap();
        let icon = tech
            .block
            .scalar("icon")
            .filter(|_| tech.block.all("icon").count() == 1)
            .filter(|icon| {
                icon.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
            .unwrap_or(k);
        let path = format!("gfx/interface/icons/technologies/{icon}.dds");
        if sources.has_resource(&path) {
            out.gfx.push(t!(
                "spriteType={name=GFX_tte_@key@ texturefile=@path@}",
                key = k,
                path = planning::atom(&path)
            ));
            icons.insert(k.clone(), format!("GFX_tte_{k}"));
        }
        out.loc(
            format!("tte_goal_entry_{k}"),
            if tech.repeatable {
                format!("${k}$ · 持续研究")
            } else {
                format!("${k}$")
            },
            if tech.repeatable {
                format!("${k}$ · Continuous research")
            } else {
                format!("${k}$")
            },
        );
        for (state, color) in [
            ("done", "G"),
            ("busy", "B"),
            ("offered", "Y"),
            ("locked", "L"),
        ] {
            out.loc(
                format!("tte_caption_{state}_{k}"),
                format!(
                    "§{color}{}§!",
                    compact_name(
                        report["localisation"]["simp_chinese"][k]["name"]
                            .as_str()
                            .unwrap_or(k),
                        15,
                        2
                    )
                ),
                format!(
                    "§{color}{}§!",
                    compact_name(
                        report["localisation"]["english"][k]["name"]
                            .as_str()
                            .unwrap_or(k),
                        15,
                        2
                    )
                ),
            );
            out.loc(
                format!("tte_caption_full_{state}_{k}"),
                format!("§{color}${k}$§!"),
                format!("§{color}${k}$§!"),
            );
        }
        // A single country-bound effect button uses the same four state
        // predicates for its caption. Avoid four overlapping GUI instances.
        out.buttons.push(button(
            &format!("tte_node_{k}"),
            "",
            &select,
            &format!("tte_detail_{k}"),
        ));
        for (mode, caption) in [(0, "tte_caption"), (1, "tte_caption_full")] {
            let states = ["done", "busy", "offered"]
                .iter()
                .map(|state| {
                    format!(
                        "text={{trigger={{ {} }} localization_key={caption}_{state}_{k}}}",
                        tech.state(state)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.getters.push(format!("defined_text={{name=GetTTECard{mode}_{k} random=no {states} default={caption}_locked_{k} }}"));
        }
        let view_name = format!("tte_view_marker_{k}");
        if parents[k]
            .iter()
            .any(|p| techs.iter().any(|t| &t.id == p && t.area != tech.area))
        {
            out.buttons.push(button(
                &format!("tte_cross_{k}"),
                "",
                &select,
                &format!("tte_detail_{k}"),
            ));
        }
        out.buttons.push(button(
            &view_name,
            &format!("has_country_flag=tte_view_{k}"),
            &select,
            &format!("tte_detail_{k}"),
        ));
        let state_texts = [
            ("done", "tte_state_done"),
            ("busy", "tte_state_busy"),
            ("offered", "tte_state_offered"),
        ]
        .into_iter()
        .map(|(s, label)| {
            t!(
                "text={trigger={@trigger@} localization_key=@label@}",
                trigger = tech.state(s),
                label = label
            )
        })
        .chain(
            diagnostics::readiness(tech)?
                .into_iter()
                .map(|(trigger, label)| {
                    t!(
                        "text={trigger={@trigger@} localization_key=@label@}",
                        trigger = trigger,
                        label = label
                    )
                }),
        )
        .collect::<Vec<_>>()
        .join(" ");
        out.getters.push(t!(
            "defined_text={name=GetTTEState_@key@ random=no @texts@ default=tte_state_locked}",
            key = k,
            texts = state_texts
        ));
        let source = report["sources"]
            [report["technologies"][k]["sourceLayer"].as_u64().unwrap() as usize]["name"]
            .as_str()
            .unwrap_or("Unknown");
        let source = plain(source)
            .replace('[', "(")
            .replace(']', ")")
            .replace(['$', '£'], "");
        let source_key = format!(
            "tte_source_name_{}",
            report["technologies"][k]["sourceLayer"].as_u64().unwrap()
        );
        out.loc(&source_key, &source, &source);
        let source = format!("${source_key}$");
        let prereqs = catalog::ids(&tech.condition)
            .into_iter()
            .map(|p| format!("${p}$"))
            .collect::<Vec<_>>()
            .join(" / ");
        let meta = &report["browseMetadata"][k];
        let unlocks = meta["unlocks"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|u| u["localizationKey"].as_str())
                    .map(|s| format!("${s}$"))
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let effects = meta["effects"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(Json::as_str)
                    .collect::<Vec<_>>()
                    .join("\\n")
            })
            .unwrap_or_default();
        let effects = if effects.is_empty() {
            effects
        } else {
            let key = format!("tte_source_effects_{k}");
            out.loc(&key, &effects, &effects);
            format!("${key}$")
        };
        let zh_meta = format!(
            "{}{}",
            if unlocks.is_empty() {
                String::new()
            } else {
                format!("\\n\\n§H解锁关联§!\\n{unlocks}")
            },
            if effects.is_empty() {
                String::new()
            } else {
                format!("\\n\\n§H直接效果§!\\n{effects}")
            }
        );
        let en_meta = format!(
            "{}{}",
            if unlocks.is_empty() {
                String::new()
            } else {
                format!("\\n\\n§HUnlocks§!\\n{unlocks}")
            },
            if effects.is_empty() {
                String::new()
            } else {
                format!("\\n\\n§HDirect effects§!\\n{effects}")
            }
        );
        out.loc(format!("tte_detail_{k}"),format!("§H${k}$§! · T{}\\n[Root.GetTTEState_{k}]\\n${k}_desc${zh_meta}\\n\\n前置：{prereqs}\\n§L来源：{source}。可选前置只需满足一支。§!\\n\\n$tte_intro$",tech.tier),format!("§H${k}$§! · T{}\\n[Root.GetTTEState_{k}]\\n${k}_desc${en_meta}\\n\\nPrerequisites: {prereqs}\\n§LSource: {source}. One OR branch suffices.§!\\n\\n$tte_intro$",tech.tier));
        if let Some(missing) = report["technologies"][k]["missingPrerequisites"].as_array() {
            let key = format!("tte_source_missing_{k}");
            let text = missing
                .iter()
                .filter_map(Json::as_str)
                .collect::<Vec<_>>()
                .join(" / ");
            out.loc(&key, &text, &text);
            for lang in ["english", "simp_chinese"] {
                let value = out
                    .localisation
                    .get_mut(lang)
                    .unwrap()
                    .get_mut(&format!("tte_detail_{k}"))
                    .unwrap();
                value.push_str(&format!("\\n\\n§Y$tte_unknown_reference$${key}$§!"));
            }
        }
        let calls = parents[k]
            .iter()
            .map(|p| format!("tte_view_visit_{p}=yes"))
            .collect::<Vec<_>>()
            .join(" ");
        out.effects.push(t!(
            "tte_view_visit_@key@={country_event={id=tteview.@id@}}",
            key = k,
            id = 1000 + i
        ));
        out.events.push(t!("country_event={id=tteview.@id@ hide_window=yes is_triggered_only=yes immediate={if={limit={NOT={has_country_flag=tte_view_@key@}} set_country_flag=tte_view_@key@ @calls@}}}",id=1000+i,key=k,calls=calls));
        let mut choices = vec![
            option("tte_inspect_selected", "tte_inspect_selected=yes", ""),
            option(
                "tte_track_selected",
                "tte_track_selected=yes tte_inspect_selected=yes",
                "",
            ),
            option(
                "tte_untrack_selected",
                "tte_untrack_selected=yes tte_inspect_selected=yes",
                "",
            ),
            option("tte_queue_full", "", "tte_queue_full=yes"),
            option("tte_full", "tte_view_clear=yes tte_open_atlas=yes", ""),
            option("tte_select_open", &jump("tteselect.1"), ""),
            option("tte_refresh_label", &jump(&e), ""),
            option("tte_back", &jump("tte.1"), ""),
            option("tte_close", "", ""),
        ];
        for parent in &parents[k] {
            if let Some((index, p)) = techs.iter().enumerate().find(|(_, t)| &t.id == parent) {
                let label = format!("tte_parent_{}", p.id);
                out.loc(
                    label.clone(),
                    format!("前置：${}$ · $tte_area_{}$", p.id, p.area),
                    format!("Prerequisite: ${}$ · $tte_area_{}$", p.id, p.area),
                );
                choices.insert(
                    choices.len() - 1,
                    option(&label, &jump(&chosen_event(index)), ""),
                );
            }
        }
        choices.insert(
            choices.len() - 1,
            option(
                "tte_zoom_large",
                &format!("set_variable={{which=tte_atlas_mode value=1}} {}", jump(&e)),
                "NOT={check_variable={which=tte_atlas_mode value=1}}",
            ),
        );
        choices.insert(
            choices.len() - 1,
            option(
                "tte_zoom_small",
                &format!("set_variable={{which=tte_atlas_mode value=0}} {}", jump(&e)),
                "check_variable={which=tte_atlas_mode value=1}",
            ),
        );
        // The class is shared with the whole field. Selecting a technology only
        // changes country flags and event text, never creates another layout.
        let imm=t!("tte_refresh=yes tte_view_clear=yes set_country_flag=tte_has_view_selection set_country_flag=tte_viewed_@key@ tte_view_visit_@key@=yes set_variable={which=tte_atlas_area value=@area@}",key=k,area=area);
        out.events.push(t!("country_event={id=@event@ hide_window=yes is_triggered_only=yes immediate={tte_remember_@key@=yes if={limit={check_variable={which=tte_atlas_mode value=1}} country_event={id=ttepreviewlarge.@id@}} else={country_event={id=ttepreviewsmall.@id@}}}}",event=e,id=1000+i,key=k));
        for (mode, namespace) in [(0, "ttepreviewsmall"), (1, "ttepreviewlarge")] {
            out.events.push(event(
                &format!("{namespace}.{}", 1000 + i),
                k,
                &format!("tte_detail_{k}"),
                &choices,
                &format!("tte_graph_{mode}_{area}"),
                "tte_graph_option",
                &imm,
            ));
        }
    }
    compile_graphs(&mut out, techs, &parents, &icons, curator)?;
    // Use one native vertical axis; tier columns fit the available width.
    let vertical = gui::effective(sources, &["right_vertical_slider"])?;
    let vertical = gui::extract(&vertical, "right_vertical_slider")?;
    let vertical = gui::property(&vertical, "name", "tte_tree_wheel")?;
    out.gui.push(gui::property(&vertical, "stepSize", "64")?);
    compile_picker(&mut out, techs, report)?;
    out.events.extend([
        "country_event={id=tte.2 hide_window=yes is_triggered_only=yes trigger={is_ai=no has_country_flag=tte_has_goal} immediate={tte_refresh=yes}}".into(),
        "event={id=tte.3 hide_window=yes is_triggered_only=yes immediate={every_country={limit={is_ai=no has_country_flag=tte_has_goal} tte_refresh=yes}}}".into(),
        "country_event={id=tte.4 hide_window=yes is_triggered_only=yes trigger={is_ai=no has_country_flag=tte_automation} immediate={tte_refresh_selection=yes}}".into()]);
    Ok(out)
}

// Sparse mod tiers are labels, not a request for dozens of empty columns.
// Retain the familiar vanilla columns and distribute all used columns over the
// complete percentage range, including the rounding remainder.
fn tier_columns(tiers: impl IntoIterator<Item = i64>) -> BTreeMap<i64, (i64, i64)> {
    let distinct: BTreeSet<_> = (0..=5).chain(tiers).collect();
    let count = distinct.len() as i64;
    distinct
        .into_iter()
        .enumerate()
        .map(|(i, tier)| {
            let start = i as i64 * 100 / count;
            let end = (i as i64 + 1) * 100 / count;
            (tier, (start, end - start))
        })
        .collect()
}

fn card_width(viewport: i64, column_percent: i64) -> i64 {
    // Outer margins, side panel, scrollbar allowance, and four-pixel gutters.
    ((viewport - 376).max(0) * column_percent / 100 - 8).max(1)
}
// Put trunks on column boundaries, with horizontal travel in the gap below a
// row. Centre-to-centre trunks crossed the transparent cards in sparse tiers.
fn route_edge(parent: (i64, i64, i64), child: (i64, i64, i64), height: i64) -> Vec<(i64, i64)> {
    let (px, py, pw) = parent;
    let (cx, cy, cw) = child;
    if px == cx {
        return vec![(px + pw, py + height / 2), (px + pw, cy + height / 2)];
    }
    let right = cx > px;
    let a = if right { px + pw } else { px };
    let b = if right { cx } else { cx + cw };
    if py == cy {
        return vec![(a, py + height / 2), (b, cy + height / 2)];
    }
    if a == b {
        return vec![(a, py + height / 2), (a, cy + height / 2)];
    }
    let gap = py + height + 6;
    vec![
        (a, py + height / 2),
        (a, gap),
        (b, gap),
        (b, cy + height / 2),
    ]
}
const RESOLUTION_BANDS: [(i64, i64); 6] = [
    (960, 1279),
    (1280, 1599),
    (1600, 1919),
    (1920, 2559),
    (2560, 3839),
    (3840, 16384),
];
fn card_sizes(column_percent: i64, height: i64) -> String {
    let mut result = format!("size={{x={} y={height}}}", card_width(640, column_percent));
    // Native buttons parse pixel sizes. Existing engine GUI uses these native
    // scaled-resolution conditions for responsive button dimensions.
    for (min, max) in RESOLUTION_BANDS {
        result.push_str(&format!(
            " if_scaled_resolution={{min_width={min} max_width={max} size={{x={} y={height} }} }}",
            card_width(min, column_percent)
        ));
    }
    result
}

fn compile_graphs(
    out: &mut View,
    techs: &[Tech],
    parents: &BTreeMap<String, Vec<String>>,
    icons: &BTreeMap<String, String>,
    curator: &str,
) -> Result<()> {
    for mode in 0..2 {
        let (height, step_y) = if mode == 0 { (48i64, 64i64) } else { (70, 86) };
        for (area_index, area) in AREAS.into_iter().enumerate() {
            let members: Vec<_> = techs.iter().filter(|t| t.area == area).collect();
            let tiers = tier_columns(members.iter().map(|t| t.tier));
            let categories: BTreeSet<_> = members.iter().map(|t| category(t)).collect();
            let mut positions: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
            let mut category_y = Vec::new();
            let mut top = 28i64;
            for cat in categories {
                let mut columns: BTreeMap<i64, Vec<&Tech>> = BTreeMap::new();
                for t in &members {
                    if category(t) == cat {
                        columns.entry(t.tier).or_default().push(t);
                    }
                }
                // Parent medians make chains line up without perturbing tier order.
                let mut ranks: BTreeMap<String, f64> = BTreeMap::new();
                for rows in columns.values_mut() {
                    rows.sort_by(|a, b| {
                        let score = |t: &Tech| {
                            let p: Vec<_> =
                                parents[&t.id].iter().filter_map(|k| ranks.get(k)).collect();
                            if p.is_empty() {
                                f64::MAX
                            } else {
                                p.iter().map(|v| **v).sum::<f64>() / p.len() as f64
                            }
                        };
                        score(a).total_cmp(&score(b)).then(a.id.cmp(&b.id))
                    });
                    for (i, t) in rows.iter().enumerate() {
                        ranks.insert(t.id.clone(), i as f64);
                    }
                }
                let count = columns.values().map(Vec::len).max().unwrap_or(0);
                category_y.push((cat, top));
                top += 24;
                for (tier, rows) in columns {
                    for (row, tech) in rows.iter().enumerate() {
                        positions.insert(
                            tech.id.clone(),
                            (tiers[&tier].0, top + row as i64 * step_y, tiers[&tier].1),
                        );
                    }
                }
                top += count as i64 * step_y + 20;
            }
            let mut widgets = Vec::new();
            for child in &members {
                for parent in &parents[&child.id] {
                    let (Some(&(px, py, pw)), Some(&(cx, cy, cw))) =
                        (positions.get(parent), positions.get(&child.id))
                    else {
                        continue;
                    };
                    let points = route_edge((px, py, pw), (cx, cy, cw), height);
                    for (segment, pair) in points.windows(2).enumerate() {
                        line(
                            out,
                            &format!("edge_{}_{}_{}", parent, child.id, segment),
                            pair[0],
                            pair[1],
                            &mut widgets,
                        );
                    }
                }
            }
            for (cat, y) in category_y {
                widgets.push(t!("instantTextBoxType={name=tte_category_@cat@ text=@text@ position={x=4 y=@y@} maxWidth=600 maxHeight=24 font=\"cg_16b\" alwaysTransparent=yes}",cat=cat,text=planning::atom(&format!("${cat}$")),y=y));
            }
            let mut columns: BTreeMap<i64, Vec<String>> = BTreeMap::new();
            for &tier in tiers.keys() {
                columns.entry(tier).or_default().push(t!("instantTextBoxType={name=tier_@tier@ text=\"T@tier@\" position={x=4 y=0} maxWidth=120 maxHeight=24 font=\"cg_16b\" alwaysTransparent=yes}",tier=tier));
            }
            for tech in &members {
                let (_, y, _) = positions[&tech.id];
                let x = 4;
                let column = columns.entry(tech.tier).or_default();
                column.push(t!("effectbuttonType={name=node_@key@ position={x=@x@ y=@y@} @sizes@ quadTextureSprite=GFX_tiling_button_standard font=\"cg_16b\" buttonText=\"[From.GetTTECard@mode@_@key@]\" multiline=yes borderSize={x=8 y=2} effect=tte_node_@key@ clicksound=click}",key=tech.id,x=x,y=y,mode=mode,sizes=card_sizes(tiers[&tech.tier].1,height)));
                if let Some(icon) = icons.get(&tech.id) {
                    column.push(t!("iconType={name=icon_@key@ spriteType=@icon@ position={x=@x@ y=@y@} scale=0.4 alwaysTransparent=yes}",key=tech.id,icon=icon,x=x+2,y=y+2));
                }
                if parents[&tech.id].iter().any(|p| !positions.contains_key(p)) {
                    column.push(t!("effectbuttonType={name=cross_@key@ position={x=-48 y=@y@} orientation=upper_right size={x=44 y=18} quadTextureSprite=GFX_tiling_button_standard font=\"cg_16b\" buttonText=tte_cross_badge effect=tte_cross_@key@ clicksound=click}",key=tech.id,y=y+height-18));
                }
                column.push(t!("effectbuttonType={name=path_@key@ position={x=-22 y=@y@} orientation=upper_right size={x=18 y=16} quadTextureSprite=GFX_tiling_button_standard font=\"cg_16b\" buttonText=tte_path_marker effect=tte_view_marker_@key@ clicksound=click}",key=tech.id,y=y-3));
            }
            let content_height = top.max(540);
            for (tier, items) in columns {
                let (x, width) = tiers[&tier];
                widgets.push(t!("containerWindowType={name=tier_column_@tier@ position={x=@x@% y=0} size={width=@w@% height=@h@} clipping=yes @items@}",tier=tier,x=x,w=width,h=content_height,items=items.join("\n")));
            }
            let graph=t!("containerWindowType={name=tte_graph_clip position={x=18 y=62} size={width=-332 height=-164} clipping=yes containerWindowType={name=tte_graph position={x=0 y=0} size={width=-22 height=100%} verticalScrollbar=tte_tree_wheel scroll_wheel_factor=128 background={name=graph_background quadTextureSprite=GFX_tile_outliner_bg} containerWindowType={name=tte_graph_content size={width=100% height=@h@} @widgets@}}} instantTextBoxType={name=legend text=tte_full_legend position={x=20 y=-28} orientation=lower_left maxWidth=1600 maxHeight=24 font=\"cg_16b\" alwaysTransparent=yes}",h=content_height,widgets=widgets.join("\n"));
            let name = format!("tte_graph_{mode}_{area_index}");
            out.gui.push(goal_controls(&gui::append(
                &gui::shell(curator, &name, true)?,
                &graph,
            )?)?);
            let mut choices = vec![
                option("tte_inspect_selected", "tte_inspect_selected=yes", ""),
                option(
                    if mode == 0 {
                        "tte_zoom_large"
                    } else {
                        "tte_zoom_small"
                    },
                    &jump(&graph_event(1 - mode, area_index)),
                    "",
                ),
                option(
                    "tte_refresh_label",
                    &jump(&graph_event(mode, area_index)),
                    "",
                ),
                option(
                    "tte_clear_path",
                    &format!(
                        "tte_view_clear=yes {}",
                        jump(&graph_event(mode, area_index))
                    ),
                    "",
                ),
            ];
            for (a, field) in AREAS.into_iter().enumerate() {
                choices.push(option(
                    &format!("tte_jump_{field}"),
                    &jump(&graph_event(mode, a)),
                    "",
                ));
            }
            for slot in 1..=MAX_GOALS {
                choices.push(option(
                    &format!("tte_goal_slot_{slot}"),
                    &format!("tte_open_tracked_{slot}=yes"),
                    &format!("has_country_flag=tte_slot_{slot}"),
                ));
            }
            choices.extend([
                option("tte_select_open", &jump("tteselect.1"), ""),
                option("tte_back", &jump("tte.1"), ""),
                option("tte_close", "", ""),
            ]);
            out.events.push(event(&graph_event(mode,area_index),"tte_full","ttegs_graph_help",&choices,&name,"tte_graph_option",&t!("tte_refresh=yes set_variable={which=tte_atlas_mode value=@mode@} set_variable={which=tte_atlas_area value=@area@}",mode=mode,area=area_index)));
        }
    }
    Ok(())
}

fn picker_page(out: &mut View, id: usize, title: &str, choices: Vec<String>) {
    let mut choices = choices;
    choices.extend([
        option("tte_select_open", &jump("tteselect.1"), ""),
        option("tte_back", &jump("tte.1"), ""),
        option("tte_close", "", ""),
    ]);
    out.events.push(event(
        &format!("tteselect.{id}"),
        title,
        "tte_select_help",
        &choices,
        "tte_home_window",
        "tte_option",
        "tte_refresh=yes",
    ));
}
fn compile_picker(out: &mut View, techs: &[Tech], report: &Json) -> Result<()> {
    let mut next = 20usize;
    let mut choices = Vec::new();
    out.loc("tte_select_purpose", "按用途选择科技", "Browse by purpose");
    let vocab = crate::metadata::vocabulary();
    let mut purpose = Vec::new();
    for topic in vocab["topics"].as_array().unwrap() {
        let topic_key = format!("tte_purpose_{}", topic["key"].as_str().unwrap());
        let topic_id = next;
        next += 1;
        out.loc(
            &topic_key,
            topic["zh"].as_str().unwrap(),
            topic["en"].as_str().unwrap(),
        );
        let mut topic_choices = Vec::new();
        for group_key in topic["groups"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Json::as_str)
        {
            let group = vocab["groups"]
                .as_array()
                .unwrap()
                .iter()
                .find(|g| g["key"] == group_key)
                .unwrap();
            let members: Vec<_> = techs
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    report["browseMetadata"][&t.id]["groups"]
                        .as_array()
                        .is_some_and(|v| v.iter().any(|g| g == group_key))
                })
                .map(|(i, t)| option(&t.id, &jump(&chosen_event(i)), ""))
                .collect();
            if members.is_empty() {
                continue;
            }
            let group_id = next;
            next += 1;
            let key = format!("tte_purpose_group_{group_key}");
            out.loc(
                &key,
                group["zh"].as_str().unwrap(),
                group["en"].as_str().unwrap(),
            );
            topic_choices.push(option(&key, &jump(&format!("tteselect.{group_id}")), ""));
            picker_page(out, group_id, &key, members);
        }
        if !topic_choices.is_empty() {
            purpose.push(option(
                &topic_key,
                &jump(&format!("tteselect.{topic_id}")),
                "",
            ));
            picker_page(out, topic_id, &topic_key, topic_choices);
        }
    }
    choices.push(option("tte_select_purpose", &jump("tteselect.10"), ""));
    picker_page(out, 10, "tte_select_purpose", purpose);
    for (a, area) in AREAS.into_iter().enumerate() {
        choices.push(option(
            &format!("tte_area_{area}"),
            &jump(&format!("tteselect.{}", 2 + a)),
            "",
        ));
        let cats: BTreeSet<_> = techs
            .iter()
            .filter(|t| t.area == area)
            .map(|t| category(t))
            .collect();
        let mut groups = Vec::new();
        for cat in cats {
            let id = next;
            next += 1;
            groups.push(option(&cat, &jump(&format!("tteselect.{id}")), ""));
            let members: Vec<_> = techs
                .iter()
                .enumerate()
                .filter(|(_, t)| t.area == area && category(t) == cat)
                .map(|(i, t)| option(&t.id, &jump(&chosen_event(i)), ""))
                .collect();
            picker_page(out, id, &cat, members);
        }
        picker_page(out, 2 + a, &format!("tte_area_{area}"), groups);
    }
    for (lang, pinyin, id, label) in [
        ("english", false, 5, "tte_select_names"),
        ("simp_chinese", true, 6, "tte_select_pinyin"),
    ] {
        let mut letters: BTreeMap<char, Vec<(String, String)>> = BTreeMap::new();
        for (i, tech) in techs.iter().enumerate() {
            let name = report["localisation"][lang][&tech.id]["name"]
                .as_str()
                .filter(|s| *s != tech.id)
                .or_else(|| report["localisation"]["english"][&tech.id]["name"].as_str())
                .unwrap_or(&tech.id);
            letters
                .entry(initial(name, pinyin))
                .or_default()
                .push((plain(name), option(&tech.id, &jump(&chosen_event(i)), "")));
        }
        let mut indexes = Vec::new();
        for (letter, mut members) in letters {
            members.sort_by(|a, b| a.0.cmp(&b.0));
            let page = next;
            next += 1;
            let key = format!(
                "tte_initial_{}_{}",
                if pinyin { "zh" } else { "en" },
                if letter == '#' {
                    "other".into()
                } else {
                    letter.to_string()
                }
            );
            out.loc(
                key.clone(),
                format!("{letter} · {} 项", members.len()),
                format!("{letter} · {} technologies", members.len()),
            );
            indexes.push(option(&key, &jump(&format!("tteselect.{page}")), ""));
            picker_page(
                out,
                page,
                &key,
                members.into_iter().map(|(_, c)| c).collect(),
            );
        }
        picker_page(out, id, label, indexes);
        choices.push(option(label, &jump(&format!("tteselect.{id}")), ""));
    }
    choices.extend([
        option("tte_select_offered", &jump("tteselect.7"), ""),
        option("tte_select_tracked", &jump("tteselect.8"), ""),
        option("tte_select_recent", &jump("tteselect.9"), ""),
    ]);
    let offered = techs
        .iter()
        .enumerate()
        .map(|(i, tech)| {
            option(
                &tech.id,
                &jump(&chosen_event(i)),
                &format!(
                    "OR={{has_tech_option={} is_researching_technology={}}}",
                    tech.id, tech.id
                ),
            )
        })
        .collect();
    picker_page(out, 7, "tte_select_offered", offered);
    picker_page(
        out,
        8,
        "tte_select_tracked",
        (1..=MAX_GOALS)
            .map(|slot| {
                option(
                    &format!("tte_goal_slot_{slot}"),
                    &format!("tte_open_tracked_{slot}=yes"),
                    &format!("has_country_flag=tte_slot_{slot}"),
                )
            })
            .collect(),
    );
    let history = crate::history::compile(techs);
    out.effects.extend(history.effects);
    out.events.extend(history.events);
    // Goal getters were already generated above; only the separate MRU getter
    // family is added here, without duplicate scripted localisation names.
    out.getters.extend(
        history
            .getters
            .into_iter()
            .filter(|s| s.contains("name=GetTTERecentEntry")),
    );
    let mut recent = Vec::new();
    for slot in 1..=8 {
        out.loc(
            format!("tte_recent_entry_{slot}"),
            format!("{slot}. [Root.GetTTERecentEntry{slot}]"),
            format!("{slot}. [Root.GetTTERecentEntry{slot}]"),
        );
        recent.push(option(
            &format!("tte_recent_entry_{slot}"),
            &format!("tte_open_recent_{slot}=yes"),
            &format!("has_country_flag=tte_recent_slot_{slot}"),
        ));
    }
    out.loc(
        "tte_recent_clear_label",
        "清除最近查看记录",
        "Clear recently viewed history",
    );
    recent.push(option(
        "tte_recent_clear_label",
        &format!("tte_recent_clear=yes {}", jump("tteselect.9")),
        "",
    ));
    picker_page(out, 9, "tte_select_recent", recent);
    out.events.push(event(
        "tteselect.1",
        "tte_select_open",
        "tte_select_help",
        &choices,
        "tte_home_window",
        "tte_option",
        "tte_refresh=yes",
    ));
    Ok(())
}

pub fn localisation_files(rows: &Localisation) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for (lang, texts) in rows {
        let mut raw = format!("\u{feff}l_{lang}:\n");
        for (key, value) in texts {
            if !safe_localisation_key(key) {
                return Err(format!("Unsafe localisation key: {key:?}"));
            }
            raw.push_str(&format!(
                " {key}:0 {}\n",
                serde_json::to_string(value)
                    .map_err(|e| e.to_string())?
                    .replace("\\\\n", "\\n")
            ));
        }
        files.insert(
            format!("localisation/{lang}/tte_native_l_{lang}.yml"),
            raw.into_bytes(),
        );
    }
    Ok(files)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responsive_pixel_cards_fit_their_columns_at_band_boundaries() {
        for count in 6..=33 {
            for i in 0..count {
                let percent = (i + 1) * 100 / count - i * 100 / count;
                for viewport in [640, 960, 1280, 1600, 1920, 2560, 3840] {
                    let column = (viewport - 376) * percent / 100;
                    assert!(
                        4 + card_width(viewport, percent) <= column,
                        "{count} columns, viewport {viewport}"
                    );
                }
            }
        }
        let sizes = card_sizes(10, 70);
        let compact = sizes.split_whitespace().collect::<String>();
        assert!(compact.contains("min_width=1920max_width=2559size={x=146y=70}"));
        assert!(!sizes.contains('%'));
    }
    #[test]
    fn localisation_keys_cannot_contain_source_script_syntax() {
        let rows = BTreeMap::from([(
            "english".into(),
            BTreeMap::from([("x: bad {}".into(), "text".into())]),
        )]);
        assert!(localisation_files(&rows)
            .unwrap_err()
            .contains("Unsafe localisation key"));
    }
    #[test]
    fn sparse_tiers_do_not_generate_empty_intermediate_columns() {
        let columns = tier_columns([0, 1, 5, 20, 32]);
        assert_eq!(columns.len(), 8);
        assert!(!columns.contains_key(&19));
        assert_eq!(columns[&20], (75, 12));
        assert_eq!(columns[&32], (87, 13));
        let mut end = 0;
        for &(start, width) in columns.values() {
            assert_eq!(start, end);
            assert!(width > 0);
            end = start + width;
        }
        assert_eq!(end, 100);
    }
    #[test]
    fn routed_trunks_stay_on_boundaries_and_row_gaps() {
        for height in [48, 70] {
            let parent = (10, 52, 10);
            let child = (40, 310, 10);
            let p = route_edge(parent, child, height);
            assert_eq!(
                p,
                vec![
                    (20, 52 + height / 2),
                    (20, 52 + height + 6),
                    (40, 52 + height + 6),
                    (40, 310 + height / 2)
                ]
            );
            assert!(p.windows(2).all(|v| v[0].0 == v[1].0 || v[0].1 == v[1].1));
            assert_eq!(
                route_edge(parent, (10, 310, 10), height),
                vec![(20, 52 + height / 2), (20, 310 + height / 2)]
            );
            assert_eq!(
                route_edge(parent, (20, 310, 10), height),
                vec![(20, 52 + height / 2), (20, 310 + height / 2)]
            );
        }
    }
    #[test]
    fn aligned_and_zero_length_edges_do_not_emit_redundant_widgets() {
        let mut out = View {
            effects: vec![],
            buttons: vec![],
            events: vec![],
            getters: vec![],
            gui: vec![],
            gfx: vec![],
            assets: BTreeMap::new(),
            localisation: BTreeMap::new(),
        };
        let mut widgets = vec![];
        line(&mut out, "zero", (12, 40), (12, 40), &mut widgets);
        assert!(widgets.is_empty());
        assert!(out.assets.is_empty());
        line(&mut out, "chain", (12, 40), (75, 40), &mut widgets);
        assert_eq!(widgets.len(), 1);
        assert!(widgets[0].contains("iconType="));
        assert!(!widgets[0].contains("background="));
        assert!(widgets[0].contains("min_width=1920 max_width=2559 spriteType="));
        line(&mut out, "tall", (12, 40), (12, 5040), &mut widgets);
        assert_eq!(widgets.len(), 4);
        assert!(out.assets.keys().any(|p| p.ends_with("_v_2048.dds")));
        assert!(out.assets.keys().any(|p| p.ends_with("_v_904.dds")));
    }
    #[test]
    fn compact_labels_have_a_bounded_two_row_budget() {
        let t = compact_name("Multi-Dimensional Technology With A Very Long Name", 18, 2);
        assert!(t.ends_with("..."));
        assert_eq!(t.split("\\n").count(), 2);
        for row in t.split("\\n") {
            assert!(row.chars().count() <= 18);
        }
    }
    #[test]
    fn compact_captions_never_split_engine_references() {
        for s in [
            "A $very_long_source_name$",
            "[Root.GetLongTechnologyName]",
            "£physics£ Very Long Technology Name",
        ] {
            assert_eq!(compact_name(s, 8, 1), s);
        }
    }
    #[test]
    fn unicode_initials_are_native_table_data() {
        assert_eq!(initial("聚变能源", true), 'J');
        assert_eq!(initial("§HZero Point Power§!", false), 'Z');
        assert_eq!(initial("3D printing", false), '#');
    }
    #[test]
    fn dds_header_and_pixels_are_bounded() {
        let v = dds(20, 2, [1, 2, 3, 4]);
        assert_eq!(&v[..4], b"DDS ");
        assert_eq!(v.len(), 128 + 20 * 2 * 4);
        assert_eq!(&v[128..132], &[3, 2, 1, 4]);
    }
}

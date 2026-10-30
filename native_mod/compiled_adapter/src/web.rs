//! Local web atlas with a source-derived, read-only catalog.
//! No runtime generation/registration, save reads, writes via HTTP or remote bind.
use crate::{catalog, script::Result, sources::Sources};
use serde_json::{json, Value as Json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};

const HINTS: &str = include_str!("../assets/web-hints.json");
const ATLAS: &[u8] = include_bytes!("../../../dist/assets/tech-icons.a3d201959e2f.webp");

fn field<'a>(block: &'a Json, key: &str) -> &'a Json {
    block["items"]
        .as_array()
        .and_then(|es| es.iter().find(|e| e["key"] == key))
        .map(|e| &e["value"])
        .unwrap_or(&Json::Null)
}
fn atom(value: &Json) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value["items"][0].as_str())
        .or_else(|| value["items"][0]["value"].as_str())
}
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '§' => {
                chars.next();
            }
            '£' => {
                for c in chars.by_ref() {
                    if c == '£' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out.replace("\\n", "\n")
}
fn localise(key: &str, values: &BTreeMap<String, String>) -> String {
    expand(values.get(key).map(String::as_str).unwrap_or(key), values)
}
fn expand(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut current = text.to_owned();
    // A bounded display-only expansion, never a script evaluator.
    for _ in 0..12 {
        let mut out = String::new();
        let mut rest = current.as_str();
        while let Some(start) = rest.find('$') {
            out.push_str(&rest[..start]);
            let tail = &rest[start + 1..];
            let Some(end) = tail.find('$') else { break };
            out.push_str(
                values
                    .get(&tail[..end])
                    .map(String::as_str)
                    .unwrap_or(&rest[start..start + end + 2]),
            );
            rest = &tail[end + 1..];
            if out.len() > 65536 {
                return plain(text);
            }
        }
        out.push_str(rest);
        if out.len() > 65536 {
            return plain(text);
        }
        if out == current {
            break;
        }
        current = out;
    }
    plain(&current)
}

pub fn payload(report: &Json, sources: &Sources) -> Result<Vec<u8>> {
    let hints: Json = serde_json::from_str(HINTS).map_err(|e| e.to_string())?;
    let rows = report["technologies"]
        .as_object()
        .ok_or("Missing web catalog")?;
    let (en, _) = sources.localisation("english");
    let (zh, _) = sources.localisation("simp_chinese");
    let pair = |key: &str| {
        let english = localise(key, &en);
        json!({"en":english,"zh":if zh.contains_key(key){localise(key,&zh)}else{english}})
    };
    let mut technologies = Vec::new();
    for (id, row) in rows {
        let raw = &row["rawDefinitionFields"];
        let category = atom(field(raw, "category")).unwrap_or("other");
        let mut category_name = pair(category);
        if category_name["en"] == category {
            category_name = hints["categories"][category].clone();
            if category_name.is_null() {
                category_name =
                    json!({"en":category.replace('_'," "),"zh":category.replace('_'," ")});
            }
        }
        let layer = row["sourceLayer"].as_u64().unwrap_or(0) as usize;
        let source_name = report["sources"][layer]["name"]
            .as_str()
            .unwrap_or("Unknown source");
        let condition = row.get("originalCondition").unwrap_or(&row["condition"]);
        let description = pair(&format!("{id}_desc"));
        let mut effects = Vec::new();
        if let Some(es) = report["browseMetadata"][id]["effects"].as_array() {
            for effect in es.iter().filter_map(Json::as_str) {
                effects.push(json!({"key":effect,"en":expand(effect,&en),"zh":expand(effect,&zh),"value":""}));
            }
        }
        let mut unlocks = Vec::new();
        if let Some(items) = report["browseMetadata"][id]["unlocks"].as_array() {
            for u in items {
                let key = u["localizationKey"].as_str().unwrap_or("");
                let name = pair(key);
                unlocks
                    .push(json!({"key":u["key"],"kind":u["kind"],"en":name["en"],"zh":name["zh"]}));
            }
        }
        let area = row["area"].as_str().unwrap_or("physics");
        let icon = field(raw, "icon").as_str().unwrap_or(id);
        // This first probe reuses the existing atlas only when a vanilla icon
        // key matches. Unknown Mod textures get an explicit field fallback.
        let icon_index = if layer == 0 {
            hints["icons"][icon].as_u64()
        } else {
            None
        };
        let fallback = hints["fieldIcons"][area].as_u64().unwrap_or(0);
        let mut weight_text = atom(field(raw, "weight")).unwrap_or("");
        for _ in 0..64 {
            if !weight_text.starts_with('@') {
                break;
            }
            let Some(next) = row["scriptedVariables"][weight_text].as_str() else {
                break;
            };
            weight_text = next;
        }
        let weight = weight_text.parse::<f64>().ok();
        technologies.push(json!({"id":id,"name":pair(id),"description":description,
            "area":area,"tier":row["tier"],"cost":row["cost"],"weight":weight,
            "category":category,"categoryName":category_name,
            "prerequisites":catalog::ids(condition),"prerequisiteCondition":condition,
            "unknownPrerequisites":row.get("missingPrerequisites").cloned().unwrap_or(json!([])),
            "source":format!("{source_name} · {}",row["sourceRelativePath"].as_str().unwrap_or("")),
            "sourceLayer":layer,"iconIndex":icon_index.unwrap_or(fallback),"iconFallback":icon_index.is_none(),
            "flags":{"rare":field(raw,"is_rare")=="yes","dangerous":field(raw,"is_dangerous")=="yes",
              "repeatable":row["repeatable"],"start":field(raw,"start_tech")=="yes",
              "conditional":field(raw,"potential").is_object()||field(raw,"weight_modifier").is_object()},
            "effects":effects,"unlocks":unlocks,"dlcs":[],"dlcCondition":null}));
    }
    let layers: Vec<_> = report["sources"]
        .as_array()
        .ok_or("Missing web sources")?
        .iter()
        .enumerate()
        .map(|(i, s)| json!({"id":i,"name":s["name"],"count":s["effectiveTechnologies"]}))
        .collect();
    let mut issue_counts = BTreeMap::<String, usize>::new();
    for key in ["warnings", "browseDiagnostics"] {
        for item in report[key].as_array().into_iter().flatten() {
            *issue_counts
                .entry(item["issue"].as_str().unwrap_or("unknown").into())
                .or_default() += 1;
        }
    }
    let data = json!({"meta":{"game":{"name":"v1.3.0","raw":"v1.3.0"},
            "iconAtlas":hints["iconAtlas"],"localViewer":true},
        "sources":layers,"technologies":technologies,"dlcs":[],"searchPinyin":hints["searchPinyin"],
        "diagnostics":{"counts":issue_counts,"eligibility":"unknown / engine-controlled","liveSaveState":false}});
    serde_json::to_vec(&data).map_err(|e| e.to_string())
}

fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match path {
        "/" | "/index.html" => (
            "text/html; charset=utf-8",
            include_bytes!("../../../dist/index.html"),
        ),
        "/styles.css" => (
            "text/css; charset=utf-8",
            include_bytes!("../../../dist/styles.css"),
        ),
        "/app.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../../dist/app.js"),
        ),
        "/i18n.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../../dist/i18n.js"),
        ),
        "/keyboard-pan.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../../dist/keyboard-pan.js"),
        ),
        "/dlc-filter.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../../dist/dlc-filter.js"),
        ),
        "/viewport-renderer.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../../dist/viewport-renderer.js"),
        ),
        "/search.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../../dist/search.js"),
        ),
        "/assets/tech-icons.a3d201959e2f.webp" => ("image/webp", ATLAS),
        "/assets/field-physics.svg" => (
            "image/svg+xml",
            include_bytes!("../assets/web/field-physics.svg"),
        ),
        "/assets/field-society.svg" => (
            "image/svg+xml",
            include_bytes!("../assets/web/field-society.svg"),
        ),
        "/assets/field-engineering.svg" => (
            "image/svg+xml",
            include_bytes!("../assets/web/field-engineering.svg"),
        ),
        // No service worker in this ephemeral research server.
        _ => return None,
    })
}
fn request<'a>(raw: &'a str, authority: &str) -> std::result::Result<(&'a str, bool), u16> {
    let mut lines = raw.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err(400);
    }
    if !matches!(parts[0], "GET" | "HEAD") {
        return Err(405);
    }
    let mut hosts = Vec::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let (key, value) = line.split_once(':').ok_or(400u16)?;
        let value = value.trim();
        if key.eq_ignore_ascii_case("Host") {
            hosts.push(value);
        }
        if key.eq_ignore_ascii_case("Origin") && value != format!("http://{authority}") {
            return Err(403);
        }
        if key.eq_ignore_ascii_case("Sec-Fetch-Site") && value == "cross-site" {
            return Err(403);
        }
    }
    if hosts != [authority] {
        return Err(403);
    }
    let path = parts[1].split('?').next().unwrap_or("");
    if !path.starts_with('/') || path.contains('%') || path.contains('\\') || path.contains("..") {
        return Err(404);
    }
    Ok((path, parts[0] == "HEAD"))
}
fn connection(mut stream: TcpStream, authority: &str, data: &[u8]) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while buffer.len() < 8192 && !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let parsed = std::str::from_utf8(&buffer)
        .map_err(|_| 400u16)
        .and_then(|raw| {
            if buffer.len() >= 8192 {
                Err(431)
            } else {
                request(raw, authority)
            }
        });
    let (status, mime, body, head) = match parsed {
        Ok(("/data/techs.json", head)) => (200, "application/json; charset=utf-8", data, head),
        Ok((path, head)) => match asset(path) {
            Some((mime, body)) => (200, mime, body, head),
            None => (
                404,
                "text/plain; charset=utf-8",
                b"Not found".as_slice(),
                head,
            ),
        },
        Err(status) => (
            status,
            "text/plain; charset=utf-8",
            b"Request rejected".as_slice(),
            false,
        ),
    };
    write!(stream,"HTTP/1.1 {status} {}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\nContent-Security-Policy: default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'\r\n\r\n",
        if status==200 {"OK"}else{"Error"},body.len())?;
    if !head {
        stream.write_all(body)?;
    }
    Ok(())
}
fn open_browser(url: &str) -> Result<()> {
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                window: isize,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let open: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                0,
                open.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result <= 32 {
            return Err(format!("Default browser did not open ({result})"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let program = if cfg!(target_os = "macos") {
            "/usr/bin/open"
        } else {
            "/usr/bin/xdg-open"
        };
        std::process::Command::new(program)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn serve(data: Vec<u8>, launch_browser: bool, request_limit: Option<usize>) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let authority = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .to_string();
    println!("WEB ATLAS / 本地科技树: http://{authority}/");
    println!("Read-only source snapshot; no current-save status. Keep this window open; close it or Ctrl+C to stop. / 只读科技资料，不读取当前存档；保留此窗口，关闭窗口或 Ctrl+C 停止服务。");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    if launch_browser {
        if let Err(error) = open_browser(&format!("http://{authority}/")) {
            eprintln!(
                "{}; open the printed URL manually / 请手动打开上方地址",
                crate::safe(&error)
            );
        }
    }
    // Optional bounded serving is used by the shipped-launcher acceptance,
    // so cross-platform tests finish normally without killing process trees.
    for stream in listener
        .incoming()
        .take(request_limit.unwrap_or(usize::MAX))
    {
        let stream = stream.map_err(|e| e.to_string())?;
        // Bad/closed browser connections must not stop the viewer.
        let _ = connection(stream, &authority, &data);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_same_origin_read_routes() {
        let host = "127.0.0.1:12345";
        assert_eq!(
            request(
                "GET /app.js?v=7 HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
                host
            ),
            Ok(("/app.js", false))
        );
        for raw in [
            "POST / HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: evil.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:12345\r\nOrigin: https://evil.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:12345\r\nSec-Fetch-Site: cross-site\r\n\r\n",
            "GET /%2e%2e/secret HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
        ] {
            assert!(request(raw, host).is_err());
        }
        assert!(asset("/../Cargo.toml").is_none());
        assert!(asset("/report.json").is_none());
    }
    #[test]
    fn no_markup_execution_and_bounded_localisation() {
        let values = BTreeMap::from([
            ("name".into(), "§Y$other$§! £physics£".into()),
            ("other".into(), "Example".into()),
        ]);
        assert_eq!(localise("name", &values), "Example ");
        assert_eq!(localise("missing", &values), "missing");
    }
    #[test]
    fn unknown_mod_nodes_overrides_and_changed_prerequisites_are_collected() {
        use std::{
            fs,
            path::Path,
            time::{SystemTime, UNIX_EPOCH},
        };
        fn write(root: &Path, path: &str, text: &str) {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let root = std::env::temp_dir().join(format!(
            "tte-web-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let game = root.join("PRIVATE_ROOT_DO_NOT_SERVE");
        let source = root.join("PRIVATE_MOD_DO_NOT_SERVE");
        write(&game,"common/technology/a.txt","base={area=physics tier=0 cost=10 category={computing}} other={area=society tier=0 cost=5 category={statecraft}}");
        write(&source, "descriptor.mod", "name=\"Unlisted test source\"");
        write(&source,"common/technology/z.txt","base={area=physics tier=0 cost=77 category={computing}} newly_authored={area=engineering tier=7 cost=123 prerequisites={OR={base other} missing} category={new_category} levels=-1}");
        write(&source,"localisation/english/a_l_english.yml","l_english:\n newly_authored:0 \"A new Mod technology\"\n newly_authored_desc:0 \"A source description\"\n");
        write(
            &source,
            "localisation/simp_chinese/a_l_simp_chinese.yml",
            "l_simp_chinese:\n newly_authored:0 \"新模组科技\"\n",
        );
        let collect = || {
            let mut sources = Sources::load(
                &game,
                &[source.clone()],
                vec!["english".into(), "simp_chinese".into()],
                true,
            )
            .unwrap();
            let mut report = catalog::collect(&mut sources).unwrap();
            crate::metadata::enrich(&mut sources, &mut report).unwrap();
            let data = payload(&report, &sources).unwrap();
            sources.verify().unwrap();
            let raw = String::from_utf8(data.clone()).unwrap();
            assert!(!raw.contains("PRIVATE_ROOT_DO_NOT_SERVE"));
            assert!(!raw.contains("PRIVATE_MOD_DO_NOT_SERVE"));
            assert!(!raw.contains("rawDefinitionFields"));
            serde_json::from_slice::<Json>(&data).unwrap()
        };
        let first = collect();
        let rows = first["technologies"].as_array().unwrap();
        let new = rows.iter().find(|t| t["id"] == "newly_authored").unwrap();
        assert_eq!(new["name"]["zh"], "新模组科技");
        assert_eq!(new["name"]["en"], "A new Mod technology");
        assert_eq!(new["prerequisites"], json!(["base", "other", "missing"]));
        assert_eq!(new["unknownPrerequisites"], json!(["missing"]));
        assert_eq!(new["flags"]["repeatable"], true);
        assert_eq!(new["category"], "new_category");
        assert_eq!(
            rows.iter().find(|t| t["id"] == "base").unwrap()["category"],
            "computing"
        );
        assert_eq!(new["iconFallback"], true);
        assert_eq!(
            rows.iter().find(|t| t["id"] == "base").unwrap()["cost"],
            77.0
        );
        write(&source,"common/technology/z.txt","newly_authored={area=engineering tier=7 cost=123 prerequisites={other} category={new_category}}");
        let second = collect();
        let new = second["technologies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "newly_authored")
            .unwrap();
        assert_eq!(new["prerequisites"], json!(["other"]));
        assert_eq!(
            second["technologies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == "base")
                .unwrap()["cost"],
            10.0
        );
        fs::remove_dir_all(root).unwrap();
    }
}

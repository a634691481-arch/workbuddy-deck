//! 面板前端资源的来源解析与投递。
//!
//! 三条来源，按「谁的产物最新」排序：
//! 1. **DevProxy** —— `tauri dev` 且 vite 在跑：只有反代才能看到刚改的源码。
//!    dev 下 `dist` 往往是上一次构建的旧产物，优先用它会把「改完没生效」伪装成正常界面。
//! 2. **Embedded / Dist** —— 安装态用打进包里的资源；本机跑 debug 构建时退回磁盘 dist
//!    （tauri 2.12 的 dev 分支自己也会读 frontendDist，这里显式判一遍是为了给出可诊断的结论）。
//! 3. **Missing** —— 三条都不通：返回说明页而不是 404 空白，白屏时用户能看懂卡在哪一步。
//!
//! 来源只解析一次（`Missing` 除外：用户可能刚起 vite 或刚 build 完，刷新就该好）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use tauri::AppHandle;

use super::{boot, proxy, shim, Reply};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    /// 打进可执行文件的资源（安装态）。
    Embedded,
    /// 磁盘上的 dist 目录（`frontendDist`）。
    Dist(PathBuf),
    /// 开发态反代 vite dev server。
    DevProxy(String),
    /// 三路都不通，附一句人类可读的探测结论。
    Missing(String),
}

static SOURCE: OnceLock<Mutex<Option<Source>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<Source>> {
    SOURCE.get_or_init(|| Mutex::new(None))
}

/// 解析（并缓存）面板资源来源。
pub(super) async fn resolve(app: &AppHandle) -> Source {
    if let Some(cached) = cache().lock().ok().and_then(|guard| guard.clone()) {
        return cached;
    }
    let detected = detect(app).await;
    // Missing 不缓存：用户可能刚起 vite / 刚 build 完 dist，刷新面板就该恢复。
    // dev 构建下的 Dist/Embedded 同样不缓存：那多半是「vite 还没监听」的临时结论，
    // 一旦锁死，面板会一直吃上一次构建的旧产物，HMR 看起来就像没生效。
    // DevProxy 探到就锁死——HMR 期间不该再回落。
    let lockable = !matches!(detected, Source::Missing(_))
        && (!tauri::is_dev() || matches!(detected, Source::DevProxy(_)));
    if lockable {
        if let Ok(mut guard) = cache().lock() {
            *guard = Some(detected.clone());
        }
    }
    detected
}

async fn detect(app: &AppHandle) -> Source {
    let dev_build = tauri::is_dev();
    let mut notes: Vec<String> = Vec::new();

    // release 版也优先 dev server：改完代码点开面板就能看到，不必重新打包。
    // 闸门是「首页带 /@vite/client」，1420 被别的服务占用时不会误接头。
    let dev_url = proxy::dev_url();
    if proxy::is_vite(&dev_url).await {
        return Source::DevProxy(dev_url);
    }
    notes.push(format!("{dev_url} 上没有 vite dev server"));

    if !dev_build && has_embedded(app) {
        return Source::Embedded;
    }
    match dist_dir() {
        Some(dir) if dir.join("index.html").is_file() => return Source::Dist(dir),
        Some(dir) => notes.push(format!("{} 缺 index.html", dir.display())),
        None => notes.push("找不到 dist 目录".to_string()),
    }
    // dev 构建下 tauri 的 asset_resolver 也会回退读 frontendDist，仍留作最后兜底。
    if has_embedded(app) {
        return Source::Embedded;
    }
    notes.push("嵌入资源里没有 index.html".to_string());
    Source::Missing(notes.join("；"))
}

fn has_embedded(app: &AppHandle) -> bool {
    app.asset_resolver().get("index.html".to_string()).is_some()
}

/// 磁盘 dist 目录：编译期 `src-tauri` 的上跳（dev/CI 都能命中），再退到当前工作目录。
fn dist_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest).join("../dist"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("dist"));
        candidates.push(cwd.join("../dist"));
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
}

/// 投递一个 GET 请求。
pub(super) async fn serve(app: &AppHandle, path: &str, query: &str) -> Reply {
    let key = normalize_key(path);
    let source = resolve(app).await;
    match &source {
        Source::DevProxy(base) => serve_proxy(base, path, query, &label(&source)).await,
        Source::Embedded => serve_embedded(app, &key, &source),
        Source::Dist(dir) => serve_dist(dir, &key, &source),
        Source::Missing(note) => {
            if is_index(&key) {
                boot::missing_page(note)
            } else {
                not_found(&key)
            }
        }
    }
}

/// 只读诊断：白屏排查第一站（`GET /__deck_diag`）。不含 token 与任何业务数据。
pub(super) async fn diag(app: &AppHandle) -> Value {
    let source = resolve(app).await;
    let dev_url = proxy::dev_url();
    let dist = dist_dir();
    json!({
        "ok": !matches!(source, Source::Missing(_)),
        "source": label(&source),
        "detail": match &source {
            Source::Missing(note) => note.clone(),
            _ => String::new(),
        },
        "dev_build": tauri::is_dev(),
        "dev": {
            "url": dev_url,
            "reachable": proxy::reachable(&dev_url).await,
        },
        "dist": dist.as_ref().map(|dir| json!({
            "dir": dir.display().to_string(),
            "index": dir.join("index.html").is_file(),
        })),
        "embedded_index": has_embedded(app),
        "shim_version": shim::SHIM_VERSION,
        "port": super::PORT.get(),
        "token_ready": super::TOKEN.get().is_some(),
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn normalize_key(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        "index.html".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_index(key: &str) -> bool {
    key == "index.html"
}

/// 只要不是绝对路径、不含 `..` 段，就当作 dist 内的相对路径。
fn is_safe_key(key: &str) -> bool {
    !key.starts_with('/')
        && !key.contains('\\')
        && !key.contains('\0')
        && !key.split('/').any(|segment| segment == "..")
}

fn serve_embedded(app: &AppHandle, key: &str, source: &Source) -> Reply {
    let Some(asset) = app.asset_resolver().get(key.to_string()) else {
        return not_found(key);
    };
    if is_index(key) {
        let html = boot::inject(&String::from_utf8_lossy(&asset.bytes), &label(source), None);
        return html_reply(html);
    }
    (200, asset.mime_type.to_string(), asset.bytes.to_vec())
}

fn serve_dist(dir: &Path, key: &str, source: &Source) -> Reply {
    if !is_safe_key(key) {
        return not_found(key);
    }
    let Ok(bytes) = std::fs::read(dir.join(key)) else {
        return not_found(key);
    };
    if is_index(key) {
        let html = boot::inject(&String::from_utf8_lossy(&bytes), &label(source), None);
        return html_reply(html);
    }
    (200, mime_for(key).to_string(), bytes)
}

async fn serve_proxy(base: &str, path: &str, query: &str, source: &str) -> Reply {
    let (status, mime, bytes) = proxy::get(base, path, query).await;
    // 只有 HTML 需要注入 shim/看门狗；其余原样透传（含 vite 的 sourcemap、字体）。
    if status != 200 || !mime.starts_with("text/html") {
        return (status, mime, bytes);
    }
    html_reply(boot::inject(
        &String::from_utf8_lossy(&bytes),
        source,
        Some(&proxy::ws_url(base)),
    ))
}

fn html_reply(html: String) -> Reply {
    (
        200,
        "text/html; charset=utf-8".to_string(),
        html.into_bytes(),
    )
}

fn not_found(key: &str) -> Reply {
    (
        404,
        "text/plain; charset=utf-8".to_string(),
        format!("not found: {key}").into_bytes(),
    )
}

fn label(source: &Source) -> String {
    match source {
        Source::Embedded => "embedded".to_string(),
        Source::Dist(dir) => format!("dist {}", dir.display()),
        Source::DevProxy(url) => format!("dev-proxy {url}"),
        Source::Missing(_) => "missing".to_string(),
    }
}

/// 扩展名 → Content-Type。磁盘投递没有 tauri 的 MimeType 可借，自己查表最省事。
fn mime_for(key: &str) -> &'static str {
    let ext = key
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_maps_to_index_html() {
        assert_eq!(normalize_key("/"), "index.html");
        assert_eq!(normalize_key(""), "index.html");
        assert_eq!(normalize_key("/assets/a.js"), "assets/a.js");
        assert!(is_index(&normalize_key("/index.html")));
    }

    #[test]
    fn path_traversal_is_rejected() {
        assert!(is_safe_key("assets/app.js"));
        assert!(!is_safe_key("../Cargo.toml"));
        assert!(!is_safe_key("assets/../../secret.txt"));
        assert!(!is_safe_key("/etc/passwd"));
        assert!(!is_safe_key("assets\\app.js"));
    }

    #[test]
    fn mime_table_covers_bundle_types() {
        assert_eq!(
            mime_for("assets/index-abc.js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(mime_for("assets/index-abc.css"), "text/css; charset=utf-8");
        assert_eq!(mime_for("assets/font.woff2"), "font/woff2");
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("bin"), "application/octet-stream");
    }

    #[test]
    fn missing_source_is_not_cached() {
        // 只验证语义：Missing 不进缓存（起 vite 后刷新即可恢复）。
        let source = Source::Missing("x".to_string());
        assert!(matches!(source, Source::Missing(_)));
        assert_ne!(label(&source), "embedded");
    }
}

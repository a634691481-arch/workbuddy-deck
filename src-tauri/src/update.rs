//! 自动更新：GitHub Releases 清单检查 + 官方 updater 下载安装。
//!
//! 三个刻意的设计：
//! 1. **不用 GitHub API 查版本**：`/releases/latest` 是 60 次/小时/IP，
//!    一千个用户共用出口 IP 会集体 403。改读 release 资产 `latest.json`（走 CDN，不计配额）。
//! 2. **版本以本 crate 为准**：`APP_VERSION` 取本应用 `Cargo.toml`，不能借 core 的
//!    （core 是 0.1.62，比对基线会错）。
//! 3. **安装永不静默**：只在用户点确认后执行；签名校验失败即中止，不留半装状态。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use wb_switch_core::modules::config;
use wb_switch_core::modules::notifications;

pub const OWNER: &str = "a634691481-arch";
pub const REPO: &str = "workbuddy-deck";
/// 本应用版本：唯一比对基线。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 成功结果缓存 6 小时（对齐上游）：轮询命中缓存不发请求，手动检查传 force 绕过。
const CACHE_TTL_SECS: i64 = 6 * 60 * 60;
/// 启动后延迟首查，避开首屏取数与托盘初始化的网络争用。
const STARTUP_DELAY_SECS: u64 = 8;
/// 后台轮询间隔。
const POLL_SECS: u64 = 6 * 60 * 60;

const RELEASE_URL: &str = "https://github.com/a634691481-arch/workbuddy-deck/releases/latest";

/// 进程内缓存：只存成功结果，失败不写（下次仍要能重试）。
static CACHE: Mutex<Option<(i64, Value)>> = Mutex::new(None);
/// 安装互斥：下载/安装只允许一个在跑。
static INSTALLING: AtomicBool = AtomicBool::new(false);

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 更新链路的落地日志：GUI 进程没有控制台，`eprintln!` 写不出去，
/// 而「检查更新一直失败」必须能自证是哪一段断的。超过 128KB 直接重建。
fn trace(line: &str) {
    use std::io::Write;
    let path = config::store_dir().join("update.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(&path).map(|m| m.len() > 128 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "{} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            line
        );
    }
}

/// 更新源配置：自动检查开关 + 镜像前缀（国内直连 GitHub 慢时填）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateConfig {
    pub auto_check: bool,
    pub mirror: String,
    /// 已提醒过的最新版本，避免每次轮询都往日志里塞一条。
    pub notified: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self { auto_check: true, mirror: String::new(), notified: String::new() }
    }
}

pub fn config_path() -> PathBuf {
    config::store_dir().join("update_config.json")
}

/// 清空检查缓存：换了更新源之后，旧结果不再代表真实最新版本。
pub fn invalidate_cache() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
}

pub fn load_config() -> UpdateConfig {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_config(cfg: &UpdateConfig) -> std::io::Result<()> {
    let dir = config::store_dir();
    std::fs::create_dir_all(&dir)?;
    let text = serde_json::to_string_pretty(cfg).unwrap_or_default();
    config::atomic_write(&config_path(), &text)
}

/// 镜像前缀归一化：只接受 http(s)，补尾斜杠；其它一律当没配（避免拼出坏 URL）。
fn normalize_mirror(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return String::new();
    }
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

/// 清单候选 URL（按优先级）。镜像前缀同时作用于两个端点。
fn manifest_urls(mirror: &str) -> Vec<String> {
    let prefix = normalize_mirror(mirror);
    let base = format!("https://github.com/{OWNER}/{REPO}/releases/latest/download");
    vec![
        format!("{prefix}{base}/latest.json"),
        format!("{prefix}{base}/latest-windows-x86_64.json"),
    ]
}

/// 逐段数字比较（`0.1.10 > 0.1.9`，字符串比较会错）。
fn version_tuple(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|seg| {
            seg.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let a = version_tuple(candidate);
    let b = version_tuple(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 本机地址不走系统代理：开发/自建镜像常把清单放在 127.0.0.1，
/// 而 `HTTP_PROXY` 一旦存在，reqwest 会连本地请求也塞给代理，结果是「明明起了服务却收不到」。
fn is_loopback(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("://127.0.0.1") || lower.contains("://localhost") || lower.contains("://[::1]")
}

async fn fetch_manifest(mirror: &str) -> Result<Value, String> {
    let urls = manifest_urls(mirror);
    let mut last = String::from("更新清单不可用");
    for url in urls {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("workbuddy-deck-updater");
        if is_loopback(&url) {
            builder = builder.no_proxy();
        }
        let client = match builder.build() {
            Ok(client) => client,
            Err(e) => {
                last = format!("HTTP 客户端初始化失败：{e}");
                continue;
            }
        };
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                Ok(body) if body.get("version").and_then(Value::as_str).is_some() => return Ok(body),
                Ok(_) => last = "更新清单缺少 version 字段".into(),
                Err(e) => last = format!("更新清单解析失败：{e}"),
            },
            Ok(resp) if resp.status().as_u16() == 404 => {
                last = "尚未发布任何可用版本".into();
            }
            Ok(resp) => last = format!("更新清单请求失败（HTTP {}）", resp.status().as_u16()),
            Err(e) => last = format!("无法连接更新源：{e}"),
        }
        trace(&format!("manifest miss url={url} err={last}"));
    }
    Err(last)
}

/// 检查更新。`force=true` 绕过 6 小时缓存（设置页手动点）。
pub async fn check(force: bool) -> Value {
    if !force {
        if let Ok(guard) = CACHE.lock() {
            if let Some((at, cached)) = guard.as_ref() {
                if now_secs() - at < CACHE_TTL_SECS {
                    return cached.clone();
                }
            }
        }
    }

    let cfg = load_config();
    let result = match fetch_manifest(&cfg.mirror).await {
        Ok(manifest) => {
            let latest = manifest
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim_start_matches('v')
                .to_string();
            let has_update = is_newer(&latest, APP_VERSION);
            let mut value = json!({
                "ok": true,
                "current": APP_VERSION,
                "latest": latest,
                "hasUpdate": has_update,
                "notes": manifest.get("notes").and_then(Value::as_str),
                "publishedAt": manifest.get("pub_date").and_then(Value::as_str),
                "releaseUrl": RELEASE_URL,
                "checkedAt": now_secs(),
                "autoCheck": cfg.auto_check,
                "mirror": cfg.mirror,
            });
            if let Ok(mut guard) = CACHE.lock() {
                *guard = Some((now_secs(), value.clone()));
            }
            // 同一版本只提醒一次：日志页要能查到，但不能每 6 小时刷一条。
            if has_update && cfg.notified != latest {
                let _ = notifications::record(
                    "info",
                    &format!("更新 · 发现 v{latest}"),
                    Some(&format!("当前 v{APP_VERSION}，可在系统设置中安装")),
                );
                let mut next = cfg.clone();
                next.notified = latest.clone();
                let _ = save_config(&next);
            }
            if !has_update {
                value["latest"] = json!(latest);
            }
            value
        }
        Err(message) => json!({
            "ok": false,
            "current": APP_VERSION,
            "error": message,
            "releaseUrl": RELEASE_URL,
            "autoCheck": cfg.auto_check,
            "mirror": cfg.mirror,
            "checkedAt": now_secs(),
        }),
    };
    // 结构化观测行：源 / 结果 / 版本三分，排错时不用猜是网络还是比对的问题。
    trace(&format!(
        "check ok={} current={} latest={} mirror={}",
        result.get("ok").and_then(Value::as_bool).unwrap_or(false),
        APP_VERSION,
        result.get("latest").and_then(Value::as_str).unwrap_or("-"),
        if cfg.mirror.is_empty() { "direct" } else { cfg.mirror.as_str() }
    ));
    result
}

/// 下载并安装；Windows 上由插件在启动安装器后结束本进程，因此前端只能依赖事件。
/// 立即返回 `{ started: true }`，结果走 `update://done`。
pub fn install(app: AppHandle) -> Value {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return json!({ "ok": false, "error": "更新正在进行中" });
    }

    let cfg = load_config();
    let urls: Vec<tauri::Url> = match manifest_urls(&cfg.mirror)
        .iter()
        .map(|u| tauri::Url::parse(u))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(urls) => urls,
        Err(e) => {
            INSTALLING.store(false, Ordering::SeqCst);
            return json!({ "ok": false, "error": format!("更新地址无效：{e}") });
        }
    };

    trace("install requested");
    tauri::async_runtime::spawn(async move {
        let finish = |ok: bool, message: Option<String>| {
            INSTALLING.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "update://done",
                json!({ "ok": ok, "error": message, "version": APP_VERSION }),
            );
        };

        let builder = match app.updater_builder().endpoints(urls) {
            Ok(builder) => builder,
            Err(e) => return finish(false, Some(format!("更新器初始化失败：{e}"))),
        };
        let updater = match builder.build() {
            Ok(updater) => updater,
            Err(e) => return finish(false, Some(format!("更新器初始化失败：{e}"))),
        };
        let update = match updater.check().await {
            Ok(Some(update)) => update,
            Ok(None) => return finish(false, Some("已是最新版本".into())),
            Err(e) => return finish(false, Some(format!("检查失败：{e}"))),
        };

        let version = update.version.clone();
        let app_for_progress = app.clone();
        let downloaded = AtomicU64::new(0);
        let _ = app.emit("update://progress", json!({ "downloaded": 0, "total": null }));

        let outcome = update
            .download_and_install(
                |chunk, total| {
                    let done = downloaded.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                    let _ = app_for_progress.emit(
                        "update://progress",
                        json!({ "downloaded": done, "total": total }),
                    );
                },
                || {},
            )
            .await;

        match outcome {
            Ok(()) => {
                // 下载即校验通过（插件先 verify 再 install），这条日志是「更新成功」的凭据。
                let _ = notifications::record(
                    "success",
                    &format!("更新 · v{version} 已下载并校验通过"),
                    Some(&format!("从 v{APP_VERSION} 升级，正在重启安装")),
                );
                let _ = app.emit("update://done", json!({ "ok": true, "version": version }));
                // Windows：install 内部已退出进程；其它平台需手动重启。
                #[cfg(not(target_os = "windows"))]
                app.restart();
            }
            Err(e) => finish(false, Some(format!("下载或校验失败：{e}"))),
        }
        INSTALLING.store(false, Ordering::SeqCst);
    });

    json!({ "ok": true, "started": true })
}

/// 启动后延迟首查 + 每 6 小时轮询；只在发现新版本时推事件，窗口不轮询。
pub fn spawn_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        trace("poller start");
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;
        loop {
            if load_config().auto_check {
                let status = check(false).await;
                if status.get("hasUpdate").and_then(Value::as_bool).unwrap_or(false) {
                    let _ = app.emit("update://available", status);
                }
            } else {
                trace("poller skip: autoCheck=false");
            }
            tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare_handles_multi_digit_segments() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v0.2.0", "0.1.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn mirror_is_normalized_or_dropped() {
        assert_eq!(normalize_mirror(""), "");
        assert_eq!(normalize_mirror("  "), "");
        assert_eq!(normalize_mirror("ftp://x"), "");
        assert_eq!(normalize_mirror("https://ghproxy.net"), "https://ghproxy.net/");
        assert_eq!(normalize_mirror("https://ghproxy.net/"), "https://ghproxy.net/");
    }

    #[test]
    fn manifest_urls_are_prefixed_in_order() {
        let urls = manifest_urls("");
        assert!(urls[0].starts_with("https://github.com/a634691481-arch/workbuddy-deck"));
        assert!(urls[0].ends_with("latest.json"));
        let mirrored = manifest_urls("https://ghproxy.net");
        assert!(mirrored[0].starts_with("https://ghproxy.net/https://github.com/"));
    }

    /// 起一个只回一次的真 HTTP 服务，验证「拉清单 → 解版本 → 比对」整条链路，
    /// 不依赖网络，也不依赖 GitHub 上是否已有 Release。
    #[test]
    fn fetch_manifest_reads_local_mirror() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = r#"{"version":"9.9.9","notes":"local","platforms":{}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let mirror = format!("http://127.0.0.1:{port}/");
        let manifest = tauri::async_runtime::block_on(fetch_manifest(&mirror)).expect("manifest");
        assert_eq!(manifest["version"], "9.9.9");
        assert!(is_newer(manifest["version"].as_str().unwrap(), APP_VERSION));
        assert!(is_loopback(&mirror));
    }
}

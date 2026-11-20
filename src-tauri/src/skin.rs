//! WorkBuddy 换肤：CDP 注入样式，不改安装目录。
//!
//! 严格对齐 WorkDaddy 的 applyThemeByCdp（scripts/daemon.js）实现口径：
//! - **原生外观同步**：写 `html[data-theme]` / `body[data-vscode-theme-kind|name]` /
//!   `cb-dark`/`vscode-dark` 类 / localStorage `agent-ui-theme` + `workbuddy.appearance.*`，
//!   让 WorkBuddy 自身的 ThemeManager 跟随；否则官方组件仍是原主题的底色。
//! - **清官方特殊皮肤**：`data-skin` + adoptedStyleSheets 里含 `--wb-bg-primary` 的
//!   sheet 全部清空，防止官方皮肤盖在我们上面。
//! - **内置浅色/深色**：纯原生同步，不注入任何 CSS（对齐 WorkDaddy default/dark）。
//! - **自定义模式（mint/cyber/nebula）**：完整 `--vscode-*` + `--wb-*` 色板 +
//!   变量别名层 + 壁纸铺 `#root` + 容器透明化。
//! - **守护**：MutationObserver 拉回根属性，防止 React/账号外观把主题弹回。
//! - **快照还原**：应用前快照官方外观键到 `wbx.skin.native-snapshot`，remove 时还原。

use std::path::PathBuf;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use wb_switch_core::modules::cdp;

const SHELL: &str = include_str!("../assets/skin/shell.css");
const PALETTE_TPL: &str = include_str!("../assets/skin/palette.css");

/// 壁纸本地目录：项目文件夹 wallpapers\（随项目一起打包），项目不存在时回落 ~/.wb-switch/wallpapers
fn wallpaper_dir() -> PathBuf {
    let project = PathBuf::from(r"C:\Users\63469\Desktop\workbuddy-task-assistant\wallpapers");
    if project.parent().map(|p| p.is_dir()).unwrap_or(false) {
        return project;
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".wb-switch").join("wallpapers")
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "image/jpeg",
    }
}

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// 本地文件读成 data URL（上传入口 / 缓存命中共用）。
fn encode_data_url(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    Some(format!(
        "data:{};base64,{}",
        mime_from_path(path),
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// 下载到本地缓存并转 data URL；缓存命中直接读本地文件（离线秒出）。
async fn fetch_cached_data_url(
    client: &reqwest::Client,
    url: &str,
    cache: PathBuf,
    mime: &'static str,
    max_bytes: usize,
) -> Option<String> {
    if cache.exists() {
        let cache2 = cache.clone();
        if let Ok(Some(data)) = tokio::task::spawn_blocking(move || encode_data_url(&cache2)).await
        {
            return Some(data);
        }
    }
    if !url.starts_with("https://") {
        return None;
    }
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return None;
    }
    let data = bytes.to_vec();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
    let cache2 = cache.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(dir) = cache2.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&cache2, &data);
    })
    .await;
    Some(format!("data:{mime};base64,{}", encoded))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinConfig {
    /// light | dark | mint | cyber | nebula
    #[serde(default = "default_mode")]
    pub mode: String,
    /// 壁纸本地文件路径（~/.wb-switch/wallpapers 下的文件名或绝对路径）；空表示不铺图
    #[serde(default)]
    pub wallpaper: String,
    /// 输入框毛玻璃模糊度（px）
    #[serde(default = "default_blur")]
    pub blur: u32,
    /// 壁纸压暗蒙版（0–1）
    #[serde(default = "default_mask")]
    pub mask: f32,
}

fn default_mode() -> String {
    "dark".to_string()
}

fn default_blur() -> u32 {
    18
}

fn default_mask() -> f32 {
    0.35
}

impl Default for SkinConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            wallpaper: String::new(),
            blur: default_blur(),
            mask: default_mask(),
        }
    }
}

struct Palette {
    label: &'static str,
    bg: &'static str,
    bg_alt: &'static str,
    panel: &'static str,
    text: &'static str,
    muted: &'static str,
    accent: &'static str,
    line: &'static str,
    hover: &'static str,
    select: &'static str,
    scroll: &'static str,
    /// 弹层实底 / tooltip 底 / hover 实底 / 禁用文字
    popover: &'static str,
    tooltip: &'static str,
    hover_solid: &'static str,
    disabled: &'static str,
    /// 毛玻璃模式：按钮底透明、次级面板透明（WorkDaddy nebula 口径）
    glass: bool,
    blur_default: u32,
    /// dark / light：决定原生外观同步方向与 darkOnly 别名层
    scheme: &'static str,
}

/// 两套模式。light/dark 为内置原生主题（不注入 CSS，对齐 WorkDaddy default/dark）。
const PALETTES: &[(&str, Palette)] = &[
    (
        "light",
        Palette {
            label: "默认浅色",
            bg: "#f6f7f9",
            bg_alt: "#eceef2",
            panel: "rgba(255, 255, 255, 0.86)",
            text: "#1a1d21",
            muted: "#6b7280",
            accent: "#2f6fed",
            line: "rgba(20, 24, 32, 0.12)",
            hover: "rgba(47, 111, 237, 0.10)",
            select: "rgba(47, 111, 237, 0.18)",
            scroll: "rgba(20, 24, 32, 0.20)",
            popover: "#ffffff",
            tooltip: "rgba(20, 24, 32, 0.92)",
            hover_solid: "#eceef2",
            disabled: "rgba(26, 29, 33, 0.45)",
            glass: false,
            blur_default: 0,
            scheme: "light",
        },
    ),
    (
        "dark",
        Palette {
            label: "深色",
            bg: "#0b0d12",
            bg_alt: "#12151c",
            panel: "rgba(16, 19, 26, 0.88)",
            text: "#e6e8ee",
            muted: "#8b93a7",
            accent: "#6b8afd",
            line: "rgba(255, 255, 255, 0.10)",
            hover: "rgba(107, 138, 253, 0.14)",
            select: "rgba(107, 138, 253, 0.24)",
            scroll: "rgba(255, 255, 255, 0.18)",
            popover: "#12151c",
            tooltip: "rgba(11, 13, 18, 0.95)",
            hover_solid: "#1d2330",
            disabled: "rgba(230, 232, 238, 0.35)",
            glass: false,
            blur_default: 12,
            scheme: "dark",
        },
    ),
];

fn palette(id: &str) -> &'static Palette {
    PALETTES
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, p)| p)
        .unwrap_or_else(|| &PALETTES[1].1)
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".wb-switch").join("skin.json"))
}

/// 读上次的配置；文件不存在就返回默认值。
pub fn load_config() -> SkinConfig {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_config(cfg: &SkinConfig) {
    let Some(path) = config_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}

/// light / dark 是内置原生模式：不注入色板（对齐 WorkDaddy default/dark）。
fn is_builtin(mode: &str) -> bool {
    matches!(mode, "light" | "dark")
}

fn render_palette(cfg: &SkinConfig) -> String {
    let p = palette(&cfg.mode);
    let dark = p.scheme == "dark";
    let dark_lead = if dark {
        "html[data-theme=\"dark\"] "
    } else {
        ""
    };
    let (btn_bg, btn_fg, bg_secondary) = if p.glass {
        (
            "transparent !important".to_string(),
            "var(--wb-color-text-primary)".to_string(),
            "transparent !important".to_string(),
        )
    } else {
        (
            p.accent.to_string(),
            "#ffffff".to_string(),
            p.panel.to_string(),
        )
    };
    let mut css = PALETTE_TPL
        .replace("__WBX_BG__", p.bg)
        .replace("__WBX_BG_ALT__", p.bg_alt)
        .replace("__WBX_PANEL__", p.panel)
        .replace("__WBX_TEXT__", p.text)
        .replace("__WBX_MUTED__", p.muted)
        .replace("__WBX_ACCENT__", p.accent)
        .replace("__WBX_LINE__", p.line)
        .replace("__WBX_HOVER__", p.hover)
        .replace("__WBX_SELECT__", p.select)
        .replace("__WBX_SCROLL__", p.scroll)
        .replace("__WBX_POPOVER__", p.popover)
        .replace("__WBX_TOOLTIP__", p.tooltip)
        .replace("__WBX_HOVER_SOLID__", p.hover_solid)
        .replace("__WBX_DISABLED__", p.disabled)
        .replace("__WBX_BTN_BG__", &btn_bg)
        .replace("__WBX_BTN_FG__", &btn_fg)
        .replace("__WBX_BG_SECONDARY__", &bg_secondary)
        .replace("__WBX_SCHEME__", p.scheme);

    // darkOnly 变量别名层（theme-vars.js 口径）：markdown 表格/代码块深色适配
    if dark {
        css.push_str(&format!(
            "{dark_lead}body[data-vscode-theme-name] .cb-markdown{{--cb-markdown-table-cell-bg:var(--wb-bg-primary);--cb-markdown-table-header-bg:var(--wb-bg-secondary);--cb-markdown-table-border-color:transparent !important;--cb-markdown-border-color:transparent !important;--cb-markdown-code-block-bg:var(--wb-bg-tertiary);--cb-markdown-code-block-header-bg:var(--wb-bg-secondary);}}"
        ));
    }
    // 输入框上方渐变兜底（浅色下官方变量回退白色）
    css.push_str(
        "body[data-vscode-theme-name] [class*=\"input-area-container\"]::before{--cb-colleagues-dashboard-bg:var(--wb-bg-primary);}",
    );

    // 毛玻璃模式：按钮/次级面板透出背景（body 层 + 主题容器作用域双层，同 WorkDaddy）
    if p.glass {
        let glass = "--wb-button-primary-bg:transparent !important;--wb-bg-secondary:transparent !important;--wb-button-primary-fg:var(--wb-color-text-primary);";
        css.push_str(&format!("body[data-vscode-theme-name]{{{glass}}}"));
        css.push_str(&format!(
            "body[data-vscode-theme-name] [data-theme],body[data-vscode-theme-name] .vscode-dark,body[data-vscode-theme-name] .dark,body[data-vscode-theme-name] .cb-dark,body[data-vscode-theme-name] .cr-theme{{{glass}}}"
        ));
    }
    css
}

/// 解析配置里的壁纸 → data URL。
///
/// 配置里存的是**本地文件**（路径或 ~/.wb-switch/wallpapers 下的文件名），
/// 注入时才临时读盘转 data URL —— 目录里的文件随时可以整包拷走。
/// 兼容旧配置里直接存的 data URL。
/// 当前壁纸的绝对路径；不存在或不是文件返回 None。
fn wallpaper_path(cfg: &SkinConfig) -> Option<PathBuf> {
    let raw = cfg.wallpaper.trim();
    if raw.is_empty() || raw.starts_with("data:") {
        return None;
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        wallpaper_dir().join(path)
    };
    path.is_file().then_some(path)
}

fn mime_of(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        _ => "image/jpeg",
    }
}

/// 壁纸引用走本地 HTTP，而不是 base64 内联。
///
/// 内联 16MB 的图会变成 22MB 的 `<style>`，WebKit 解析直接卡死——表现为「切了没效果」。
/// WorkDaddy 同样是 `/api/theme-video` 这类字节流接口，CSS 里只留 URL。
fn resolve_wallpaper(cfg: &SkinConfig) -> Option<String> {
    let path = wallpaper_path(cfg)?;
    let base = crate::deck_http::base_url()?;
    let version = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|delta| delta.as_millis())
        .unwrap_or(0);
    Some(format!("{base}/skin/wallpaper?v={version}"))
}

/// `/skin/wallpaper` 的响应：直接吐当前壁纸字节，切换后按 `v` 参数破缓存。
pub fn wallpaper_reply() -> (u16, String, Vec<u8>) {
    let cfg = load_config();
    let Some(path) = wallpaper_path(&cfg) else {
        return (
            404,
            "text/plain; charset=utf-8".into(),
            b"no wallpaper".to_vec(),
        );
    };
    let mime = mime_of(&path).to_string();
    match std::fs::read(&path) {
        Ok(bytes) => (200, mime, bytes),
        Err(error) => (
            500,
            "text/plain; charset=utf-8".into(),
            format!("read wallpaper failed: {error}").into_bytes(),
        ),
    }
}

fn render_css(cfg: &SkinConfig, wall_url: Option<&str>) -> String {
    let mut palette_css = String::new();
    if !is_builtin(&cfg.mode) {
        palette_css = render_palette(cfg);
    }

    // 壁纸块整段按有无壁纸生成：没图时不写 #root 规则，避免把官方背景清空
    let wall_block = if let Some(url) = wall_url {
        let url = url.replace('"', "%22");
        let mask = cfg.mask;
        format!(
            "#root {{ background: linear-gradient(rgba(0,0,0,{mask}), rgba(0,0,0,{mask})), \
             linear-gradient(90deg, color-mix(in srgb, var(--wb-bg-primary, #0a0a0a) 40%, transparent) 0 18%, transparent 42%), \
             linear-gradient(180deg, transparent 0 58%, color-mix(in srgb, var(--wb-bg-primary, #0a0a0a) 50%, transparent) 100%), \
             url(\"{url}\") right center / cover no-repeat fixed !important; }}"
        )
    } else {
        String::new()
    };

    SHELL
        .replace("__WBX_PALETTE__", &palette_css)
        .replace("__WBX_WALL__", &wall_block)
        .replace("__WBX_BLUR__", &cfg.blur.to_string())
}

/// 当前配置 + 可选模式清单。
pub fn state() -> Value {
    let cfg = load_config();
    json!({
        "ok": true,
        "config": cfg,
        "modes": PALETTES.iter().map(|(id, p)| json!({
            "id": id,
            "label": p.label,
            "accent": p.accent,
            "blur": p.blur_default,
        })).collect::<Vec<_>>(),
    })
}

async fn page() -> Result<std::sync::Arc<cdp::CdpSession>, String> {
    if let Some(session) = cdp::workbuddy_page(false).await {
        return Ok(session);
    }
    // 用户点「应用主题」是一次明确动作：客户端没开调试端口时，替他带端口重启一次。
    // 只试一次（进程内），失败后不再反复折腾用户的客户端。
    static BOOTSTRAP_TRIED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if BOOTSTRAP_TRIED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("未连上 WorkBuddy 的调试端口，请先在设置页点「带调试端口重启」".to_string());
    }
    let variant = wb_switch_core::modules::variant::WbVariant::Cn;
    let restarted = tokio::task::spawn_blocking(move || {
        wb_switch_core::modules::process::restart_workbuddy_with_cdp(variant, 9222, 15)
    })
    .await
    .map_err(|error| format!("重启 WorkBuddy 的任务失败: {error}"))?;
    if let Err(error) = restarted {
        return Err(format!(
            "WorkBuddy 没有开调试端口，带端口重启失败：{error}；可在设置页手动点「带调试端口重启」"
        ));
    }
    // 冷启动 + 渲染进程就绪通常 10–20s，给 24 次 × 500ms 的窗口。
    for _ in 0..24 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if let Some(session) = cdp::workbuddy_page(false).await {
            return Ok(session);
        }
    }
    Err("WorkBuddy 已带调试端口重启，但 12 秒内没连上页面，请稍后重试应用主题".to_string())
}

/// 应用脚本：原生外观同步 + 清官方皮肤 + 注入色板/壁纸 + 守护。
/// 占位符：__CSS__（注入的 CSS，可为空串）、__MODE__（模式 id）、__DARK__（true/false）。
const APPLY_JS: &str = r#"(() => {
  const css = __CSS__;
  const modeId = __MODE__;
  const wantDark = __DARK__;
  const h = document.documentElement, b = document.body;
  if (!h || !b) return { pending: true };
  const SNAP = 'wbx.skin.native-snapshot';
  const mode = wantDark ? 'dark' : 'light';
  const kind = wantDark ? 'vscode-dark' : 'vscode-light';
  const name = wantDark ? 'IDE Night' : 'IDE Light';
  const builtin = { kind: 'theme', resourceKey: mode, nameZh: wantDark ? '深色' : '浅色', nameEn: wantDark ? 'Dark' : 'Light', vipLevel: 'free', updatedAt: 0, series: 'base', appearance: mode };

  // 1. 快照官方外观键（一次），remove 时还原
  try {
    if (!localStorage.getItem(SNAP)) {
      const keys = [];
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k && (k.indexOf('workbuddy.appearance.') === 0 || k === 'agent-ui-theme')) keys.push([k, localStorage.getItem(k)]);
      }
      localStorage.setItem(SNAP, JSON.stringify({ keys, at: Date.now() }));
    }
  } catch (e) {}

  // 2. 清官方特殊皮肤（SkinManager 的 adoptedStyleSheets + data-skin 标记）
  try {
    h.removeAttribute('data-skin');
    document.querySelectorAll('style[data-skin-sheet]').forEach((s) => { s.textContent = ''; });
    let cssState = null;
    try { cssState = JSON.parse(localStorage.getItem('workbuddy.appearance.lastApplied.css') || 'null'); } catch (e) {}
    const officialCss = String((cssState || {}).css || '');
    if (document.adoptedStyleSheets && document.adoptedStyleSheets.length) {
      const kept = [];
      for (const sheet of document.adoptedStyleSheets) {
        let text = '';
        try { text = Array.from(sheet.cssRules || []).map((r) => r.cssText).join('\n'); } catch (e) {}
        const looksSkin = text.indexOf('--wb-bg-primary') !== -1 || text.indexOf('--cb-bg-primary') !== -1 || text.indexOf('--cb-color') !== -1;
        if (text !== officialCss && !looksSkin) kept.push(sheet);
        else if (typeof sheet.replaceSync === 'function') { sheet.replaceSync(''); kept.push(sheet); }
      }
      if (kept.length !== document.adoptedStyleSheets.length) document.adoptedStyleSheets = kept;
    }
    localStorage.removeItem('workbuddy.appearance.lastApplied.css');
  } catch (e) {}

  // 3. 原生外观全链路同步（ThemeManager 恢复链路），防止被官方弹回
  try {
    localStorage.setItem('agent-ui-theme', JSON.stringify({ theme: mode, followSystem: false, vsCodeThemeName: name, vsCodeThemeKind: kind }));
    localStorage.setItem('workbuddy.appearance.lastApplied', JSON.stringify(builtin));
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const k = localStorage.key(i);
      if (!k) continue;
      if (k.indexOf('workbuddy.appearance.mode::') === 0) localStorage.setItem(k, mode);
      else if (k.indexOf('workbuddy.appearance.lastApplied::') === 0) localStorage.setItem(k, JSON.stringify(builtin));
      else if (k.indexOf('workbuddy.appearance.state::') === 0) {
        try {
          const st = JSON.parse(localStorage.getItem(k) || '{}');
          st.currentTheme = mode;
          if (st.pendingSync && st.pendingSync.theme) st.pendingSync.theme = mode;
          localStorage.setItem(k, JSON.stringify(st));
        } catch (e) {}
      }
    }
  } catch (e) {}

  // 4. 根属性 + 类切换（html 与 body 全量，同 WorkDaddy keepSelectedTheme）
  h.setAttribute('data-theme', mode);
  h.style.colorScheme = mode;
  b.setAttribute('data-vscode-theme-kind', kind);
  b.setAttribute('data-vscode-theme-name', name);
  const wanted = wantDark ? ['cb-dark', 'dark', 'vscode-dark'] : ['cb-light', 'light', 'vscode-light'];
  for (const el of [h, b]) {
    for (const c of ['cb-dark', 'cb-light', 'dark', 'light', 'vscode-dark', 'vscode-light']) {
      el.classList.toggle(c, wanted.indexOf(c) !== -1);
    }
  }
  h.setAttribute('data-wbs-theme', css ? '1' : '0');
  h.setAttribute('data-wbs-theme-id', modeId);

  // 5. 注入 / 移除样式表（内置浅色/深色 css 为空串，纯原生同步）
  let el = document.getElementById('wbs-theme-style');
  if (!css) {
    if (el) el.remove();
  } else {
    if (!el) {
      el = document.createElement('style');
      el.id = 'wbs-theme-style';
      (document.head || h).appendChild(el);
    }
    if (el.textContent !== css) el.textContent = css;
  }

  // 6. 守护：React / 账号外观写入会把根属性弹回，观察并强制拉回
  const keep = () => {
    if (h.getAttribute('data-theme') !== mode) h.setAttribute('data-theme', mode);
    if (b.getAttribute('data-vscode-theme-kind') !== kind) b.setAttribute('data-vscode-theme-kind', kind);
    if (b.getAttribute('data-vscode-theme-name') !== name) b.setAttribute('data-vscode-theme-name', name);
  };
  keep();
  try { if (window.__wbxThemeGuard) window.__wbxThemeGuard.disconnect(); } catch (e) {}
  if (typeof MutationObserver !== 'undefined') {
    const g = new MutationObserver(keep);
    g.observe(h, { attributes: true, attributeFilter: ['data-theme'] });
    g.observe(b, { attributes: true, attributeFilter: ['data-vscode-theme-kind', 'data-vscode-theme-name'] });
    window.__wbxThemeGuard = g;
  }
  return { ok: true };
})()"#;

/// 恢复脚本：删样式/守护/标记，把快照的官方外观键写回去并按快照重同步根属性。
const REMOVE_JS: &str = r#"(() => {
  const h = document.documentElement, b = document.body;
  const st = document.getElementById('wbs-theme-style');
  if (st) st.remove();
  try { if (window.__wbxThemeGuard) { window.__wbxThemeGuard.disconnect(); delete window.__wbxThemeGuard; } } catch (e) {}
  h.removeAttribute('data-wbs-theme');
  h.removeAttribute('data-wbs-theme-id');
  try {
    const snap = JSON.parse(localStorage.getItem('wbx.skin.native-snapshot') || 'null');
    if (snap && Array.isArray(snap.keys)) {
      for (const kv of snap.keys) {
        try { if (kv[1] === null) localStorage.removeItem(kv[0]); else localStorage.setItem(kv[0], kv[1]); } catch (e) {}
      }
    }
    localStorage.removeItem('wbx.skin.native-snapshot');
  } catch (e) {}
  let mode = 'light';
  try {
    const t = JSON.parse(localStorage.getItem('agent-ui-theme') || 'null');
    if (t && t.theme === 'dark') mode = 'dark';
  } catch (e) {}
  const wantDark = mode === 'dark';
  const kind = wantDark ? 'vscode-dark' : 'vscode-light';
  const name = wantDark ? 'IDE Night' : 'IDE Light';
  h.setAttribute('data-theme', mode);
  h.style.colorScheme = mode;
  if (b) {
    b.setAttribute('data-vscode-theme-kind', kind);
    b.setAttribute('data-vscode-theme-name', name);
    const wanted = wantDark ? ['cb-dark', 'dark', 'vscode-dark'] : ['cb-light', 'light', 'vscode-light'];
    for (const c of ['cb-dark', 'cb-light', 'dark', 'light', 'vscode-dark', 'vscode-light']) {
      h.classList.toggle(c, wanted.indexOf(c) !== -1);
      b.classList.toggle(c, wanted.indexOf(c) !== -1);
    }
  }
  return true;
})()"#;

/// 应用配置：重写 `<style id="wbs-theme-style">` + 原生外观同步，立即生效。
pub async fn apply(cfg: SkinConfig) -> Result<Value, String> {
    // 注入时把本地壁纸文件读成 data URL（配置里只存路径，方便整目录打包）
    let wall_url = resolve_wallpaper(&cfg);
    let has_wall = wall_url.is_some();
    let css = render_css(&cfg, wall_url.as_deref());
    let payload = APPLY_JS
        .replace(
            "__CSS__",
            &serde_json::to_string(&css).map_err(|e| e.to_string())?,
        )
        .replace(
            "__MODE__",
            &serde_json::to_string(&cfg.mode).map_err(|e| e.to_string())?,
        )
        .replace(
            "__DARK__",
            if palette(&cfg.mode).scheme == "dark" {
                "true"
            } else {
                "false"
            },
        );

    let session = page().await?;
    session
        .evaluate(&payload)
        .await
        .map_err(|e| format!("注入失败: {e}"))?;
    save_config(&cfg);

    Ok(json!({
        "ok": true,
        "mode": cfg.mode,
        "label": palette(&cfg.mode).label,
        "hasWallpaper": has_wall,
    }))
}

/// 恢复官方外观：删注入样式 + 还原快照的官方外观状态。
pub async fn remove() -> Result<Value, String> {
    let session = page().await?;
    session
        .evaluate(REMOVE_JS)
        .await
        .map_err(|e| format!("恢复失败: {e}"))?;
    Ok(json!({ "ok": true }))
}

/// wallhaven 壁纸列表（SFW / 横向 / 1080p+）。
///
/// 参数对齐官方文档：`sorting=toplist` + `topRange=1M` 取最近一个月热门，
/// `page` 翻页。缩略图在后端下载到本地缓存（~/.wb-switch/wallpapers）再转
/// data URL —— 渲染进程 CSP 拦外域图，且本地缓存让二次打开秒出。
pub async fn wallpapers(page: u32) -> Result<Value, String> {
    let url = format!(
        "https://wallhaven.cc/api/v1/search?sorting=toplist&topRange=1M&categories=111&purity=100&atleast=1920x1080&ratios=16x9&page={}",
        page.max(1)
    );
    let client = reqwest::Client::new();
    let body = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("壁纸接口失败: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let parsed: Value = serde_json::from_str(&body).map_err(|e| format!("解析失败: {e}"))?;
    let rows = parsed
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "没有取到壁纸".to_string())?;

    // 每行：缩略图本地缓存文件 ~/.wb-switch/wallpapers/{id}_t.jpg
    let mut items: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.get("id").cloned().unwrap_or(Value::Null),
                "url": row.get("path").cloned().unwrap_or(Value::Null),
                "thumb": Value::Null,
                "resolution": row.get("resolution").cloned().unwrap_or(Value::Null),
                "colors": row.get("colors").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();

    let handles: Vec<_> = rows
        .iter()
        .map(|row| {
            let client = client.clone();
            let id = row
                .get("id")
                .map(|v| v.to_string().trim_matches('"').to_string())
                .unwrap_or_default();
            let cache = wallpaper_dir().join(format!("{id}_t.jpg"));
            let thumb = row
                .get("thumbs")
                .and_then(|t| t.get("small"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            tokio::task::spawn(async move {
                fetch_cached_data_url(&client, &thumb, cache, "image/jpeg", 1024 * 1024).await
            })
        })
        .collect();
    for (i, handle) in handles.into_iter().enumerate() {
        if let Ok(Some(data)) = handle.await {
            items[i]["thumb"] = Value::String(data);
        }
    }

    Ok(json!({
        "ok": true,
        "page": page.max(1),
        "lastPage": parsed.get("meta").and_then(|m| m.get("last_page")).cloned().unwrap_or(Value::Null),
        "items": items,
    }))
}

/// 选中壁纸：下载原图到本地缓存目录，返回**本地路径**（配置里存路径）+ 预览 data URL。
pub async fn wallpaper_data(url: &str) -> Result<Value, String> {
    // 用远端文件名做缓存键：wallhaven 的 path 形如 https://w.../cc/wallhaven-xxxx.jpg
    let seg = url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty() && s.contains('.'))
        .ok_or_else(|| "壁纸地址不对".to_string())?
        .to_string();
    let cache = wallpaper_dir().join(&seg);
    let mime = mime_from_path(&cache);
    let client = reqwest::Client::new();
    let data = fetch_cached_data_url(&client, url, cache.clone(), mime, MAX_IMAGE_BYTES)
        .await
        .ok_or_else(|| "壁纸下载失败（太大或不可达）".to_string())?;
    Ok(json!({
        "ok": true,
        "path": cache.to_string_lossy(),
        "dataUrl": data,
    }))
}

/// 本地图片上传：对话框选图 → 拷进壁纸目录（upload-<时间戳>.<ext>）→ 返回路径。
pub async fn wallpaper_upload(app: tauri::AppHandle) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_file())
            .await
            .map_err(|e| format!("文件对话框失败: {e}"))?;
    let src = picked
        .and_then(|p| p.as_path().map(|x| x.to_path_buf()))
        .ok_or_else(|| "未选择图片".to_string())?;
    let dest = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp"))
            .unwrap_or_else(|| "jpg".to_string());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dir = wallpaper_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建壁纸目录失败: {e}"))?;
        let dest = dir.join(format!("upload-{ts}.{ext}"));
        std::fs::copy(&src, &dest).map_err(|e| format!("拷贝壁纸失败: {e}"))?;
        Ok(dest)
    })
    .await
    .map_err(|e| format!("图片处理失败: {e}"))??;
    let dest2 = dest.clone();
    let data = tokio::task::spawn_blocking(move || encode_data_url(&dest2))
        .await
        .map_err(|e| format!("图片读取失败: {e}"))?
        .ok_or_else(|| "图片读取失败（超过 20MB 或不是图片）".to_string())?;
    Ok(json!({
        "ok": true,
        "path": dest.to_string_lossy(),
        "dataUrl": data,
    }))
}

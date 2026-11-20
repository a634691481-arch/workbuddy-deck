//! 面板 index.html 的注入层：本地桥 shim、开发态 HMR 重写、白屏兜底错误卡。
//!
//! 为什么注入放在 Rust 侧而不是前端：白屏意味着前端 JS 根本没跑起来
//! （资源 404 / bundle 顶层抛错），前端自己的兜底救不了这两种情况。
//!
//! 注入顺序只有一种正确写法：**shim 先、boot 后**，两者都是普通 script，
//! 在 `</head>` 里同步执行，早于任何 `type="module"`（defer 语义）的 bundle。

use super::Reply;

/// 注入点：`</head>`。dist 与 vite dev 的 index.html 都带它。
const HEAD_END: &str = "</head>";
/// `</head>` 缺失时的第二选择：插到 `<body` 之前（普通 script 仍先于 module 执行）。
const BODY_OPEN: &str = "<body";

/// 把 shim 与看门狗注入到 HTML。
///
/// `source` 是资源来源的人类可读标签（写进错误卡与诊断，便于用户直接看出走的哪条路）。
/// `dev_ws` 非空时（开发态反代）额外装一个 WebSocket 改写，让 HMR 连到 vite 而不是桥。
pub(super) fn inject(html: &str, source: &str, dev_ws: Option<&str>) -> String {
    let tags = format!(
        r#"<script src="/__deck_shim.js"></script><script>{}</script>"#,
        boot_script(source, dev_ws)
    );
    if let Some(pos) = html.find(HEAD_END) {
        let mut out = String::with_capacity(html.len() + tags.len());
        out.push_str(&html[..pos]);
        out.push_str(&tags);
        out.push_str(&html[pos..]);
        return out;
    }
    if let Some(pos) = html.find(BODY_OPEN) {
        let mut out = String::with_capacity(html.len() + tags.len());
        out.push_str(&html[..pos]);
        out.push_str(&tags);
        out.push_str(&html[pos..]);
        return out;
    }
    format!("{tags}{html}")
}

fn boot_script(source: &str, dev_ws: Option<&str>) -> String {
    BOOT_JS
        .replace("__WB_SOURCE__", &js_literal(source))
        .replace(
            "__WB_DEV_WS__",
            &dev_ws.map(js_literal).unwrap_or_else(|| "''".to_string()),
        )
}

fn js_literal(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 三条资源路都不通时的说明页：要让用户一眼看出该做什么（起 vite 还是重新打包）。
pub(super) fn missing_page(note: &str) -> Reply {
    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>WorkBuddy Deck · 面板没有资源</title></head>
<body style="margin:0;background:#0b0b0d;color:#e9e9ec;font:13px/1.7 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;padding:28px">
<div style="color:#ff5d5d;font-weight:600;letter-spacing:.08em">DECK 面板没有可用的前端资源</div>
<p style="max-width:860px">三条路都试过了：安装包内嵌资源、磁盘 dist 构建产物、vite devUrl 反代。</p>
<pre style="background:#141417;border:1px solid rgba(255,255,255,.12);padding:12px;white-space:pre-wrap;max-width:860px">{}</pre>
<p style="max-width:860px">开发态：先 <code>npm run dev</code>（vite 监听 1420），或 <code>npm run build</code> 生成 dist。<br>
安装态：确认构建时 <code>frontendDist</code> 指向的产物已经打进包。<br>
完整诊断：<a style="color:#7fd1ff" href="/__deck_diag">/__deck_diag</a></p>
</body></html>"#,
        html_escape(note)
    );
    (
        200,
        "text/html; charset=utf-8".to_string(),
        html.into_bytes(),
    )
}

/// 兜底脚本：开发态 HMR 改写 + 白屏错误卡。占位符由 [`boot_script`] 填。
const BOOT_JS: &str = r###"
(function () {
  var DEV_WS = __WB_DEV_WS__;
  var SOURCE = __WB_SOURCE__;
  var loadErrors = [];
  var shown = false;

  function mounted() {
    var root = document.getElementById('app');
    return !!(root && root.children.length > 0);
  }

  // 开发态：vite HMR 的 WebSocket 默认连当前 origin（也就是本桥，桥不转发 ws），
  // 这里改写到 vite dev server。连不上只是没有热更新，不影响渲染。
  if (DEV_WS) {
    try {
      var Native = window.WebSocket;
      var fix = function (url) {
        try {
          var u = new URL(String(url), location.href);
          if (u.protocol !== 'ws:' && u.protocol !== 'wss:') return url;
          if (u.hostname !== location.hostname || u.port !== location.port) return url;
          return DEV_WS + u.pathname + u.search;
        } catch (e) {
          return url;
        }
      };
      var Patched = function (url, protocols) {
        return protocols === undefined ? new Native(fix(url)) : new Native(fix(url), protocols);
      };
      Patched.prototype = Native.prototype;
      Patched.CONNECTING = 0; Patched.OPEN = 1; Patched.CLOSING = 2; Patched.CLOSED = 3;
      window.WebSocket = Patched;
    } catch (e) {}
  }

  function show(reason) {
    if (shown || mounted()) return;
    shown = true;
    var layer = document.createElement('div');
    layer.setAttribute('data-deck-boot-error', '1');
    layer.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#0b0b0d;color:#e9e9ec;' +
      'font:13px/1.7 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;padding:28px;box-sizing:border-box;' +
      'display:flex;flex-direction:column;gap:14px;';
    var title = document.createElement('div');
    title.textContent = 'DECK 面板加载失败';
    title.style.cssText = 'color:#ff5d5d;font-weight:600;letter-spacing:.08em;';
    var body = document.createElement('div');
    body.textContent = String(reason || '未知原因');
    body.style.cssText = 'white-space:pre-wrap;max-width:860px;';
    var meta = document.createElement('div');
    meta.style.cssText = 'opacity:.7;max-width:860px;';
    meta.textContent = '资源来源 ' + SOURCE + ' · 诊断 ' + location.origin + '/__deck_diag';
    var row = document.createElement('div');
    row.style.cssText = 'display:flex;gap:8px;flex-wrap:wrap;';
    var mkButton = function (label, run) {
      var b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.style.cssText = 'padding:7px 14px;background:#17171a;color:#e9e9ec;border:1px solid rgba(255,255,255,.18);' +
        'font:inherit;cursor:pointer;';
      b.onmouseenter = function () { b.style.borderColor = 'rgba(255,255,255,.45)'; };
      b.onmouseleave = function () { b.style.borderColor = 'rgba(255,255,255,.18)'; };
      b.onclick = run;
      return b;
    };
    row.appendChild(mkButton('重试', function () { location.reload(); }));
    row.appendChild(mkButton('打开诊断', function () { location.href = '/__deck_diag'; }));
    layer.appendChild(title);
    layer.appendChild(body);
    layer.appendChild(meta);
    layer.appendChild(row);
    (document.body || document.documentElement).appendChild(layer);
  }
  window.__WB_DECK_BOOT_FAIL = show;

  // 资源级失败先记账，不立刻遮屏：favicon / 可选样式 404 不代表面板不可用。
  // 只有脚本错误（可能是 bundle 顶层异常）与最终看门狗超时才弹卡。
  window.addEventListener('error', function (e) {
    var target = e && e.target;
    if (target && (target.tagName === 'SCRIPT' || target.tagName === 'LINK')) {
      loadErrors.push(String(target.src || target.href || target.tagName));
      return;
    }
    if (e && e.message) show('页面脚本错误: ' + e.message);
  }, true);

  window.addEventListener('unhandledrejection', function (e) {
    var reason = e && e.reason;
    show('未处理的 Promise 拒绝: ' + (reason && reason.message ? reason.message : String(reason)));
  });

  // 挂载看门狗：8 秒还没内容就是白屏（vite dev 首屏冷启动最慢也在 3s 内）。
  var ticks = 0;
  var timer = setInterval(function () {
    if (mounted()) {
      clearInterval(timer);
      return;
    }
    ticks += 1;
    if (ticks < 8) return;
    clearInterval(timer);
    show('8 秒内没有渲染出界面（bundle 未执行或挂载失败）' +
      (loadErrors.length ? '\n失败资源:\n' + loadErrors.join('\n') : ''));
  }, 1000);
})();
"###;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_puts_shim_before_boot_script() {
        let html = "<html><head><title>t</title></head><body><div id=\"app\"></div></body></html>";
        let out = inject(html, "dist /x", None);
        let shim_at = out.find("/__deck_shim.js").expect("shim 未注入");
        let boot_at = out.find("__WB_DECK_BOOT_FAIL").unwrap_or(0);
        assert!(shim_at < boot_at, "shim 必须早于 boot 脚本");
        // 仍在 head 内、body 之前：module 脚本是 defer，普通 script 一定先跑。
        assert!(out.find("/__deck_shim.js").unwrap() < out.find("<body").unwrap());
        assert!(out.contains("\"dist /x\""), "来源标签没写进脚本");
    }

    #[test]
    fn inject_falls_back_to_body_when_head_missing() {
        let out = inject(
            "<body><div id=\"app\"></div></body>",
            "embedded",
            Some("ws://localhost:1420"),
        );
        assert!(out.contains("/__deck_shim.js"));
        assert!(out.contains("\"ws://localhost:1420\""));
        assert!(out.starts_with("<script src=\"/__deck_shim.js\""));
    }

    #[test]
    fn missing_page_escapes_note() {
        let (status, mime, body) = missing_page("<script>alert(1)</script>");
        assert_eq!(status, 200);
        assert!(mime.starts_with("text/html"));
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("&lt;script&gt;"));
        assert!(!text.contains("<script>alert(1)</script>"));
    }
}

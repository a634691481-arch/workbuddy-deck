//! 面板 iframe 里的「伪 Tauri 宿主」：把 `@tauri-apps/api` 的 invoke 转成本地 HTTP，
//! 事件转成 EventSource，窗口类命令一律当空操作（内嵌面板没有真实窗口）。
//!
//! 注入时机由 [`super::boot`] 保证：这段脚本必须在 bundle 之前执行，
//! 否则 `@tauri-apps/api` 初始化时读不到 `window.__TAURI_INTERNALS__`，面板里所有 invoke 直接炸。
//!
//! 双保险之一：`__WB_DECK_EMBED__` 是前端 `isEmbedded()` 的唯一判据
//! （`src/lib/tauri-env.ts`），少了它前端会把面板当成独立窗口去做窗口操作。

/// 面板与宿主的协议版本：与 shim 语义绑定的事件/IPC 形状变化时递增。
/// 只用于给前端和诊断页一个可比对的字符串，不参与逻辑分支。
pub(super) const SHIM_VERSION: u32 = 2;

pub(super) const SHIM: &str = r#"
(function () {
  if (window.__WB_DECK_EMBED__) return;
  window.__WB_DECK_EMBED__ = true;
  document.documentElement.setAttribute('data-deck-embed', '1');

  var token = (new URLSearchParams(location.search)).get('__deck_t') || '';
  var callbacks = new Map();
  var nextCallback = 1;
  var listeners = new Map();
  var nextEventId = 1;

  function call(cmd, args) {
    return fetch('/__ipc?t=' + encodeURIComponent(token), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ cmd: cmd, args: args || {} })
    }).then(function (resp) {
      if (!resp.ok) throw new Error('IPC HTTP ' + resp.status);
      return resp.json();
    }).then(function (body) {
      if (body && body.__error != null) throw body.__error;
      return body ? body.__ok : null;
    });
  }

  function transformCallback(callback, once) {
    var id = nextCallback++;
    callbacks.set(id, { fn: callback, once: !!once });
    return id;
  }

  window.__TAURI_INTERNALS__ = {
    invoke: function (cmd, args) {
      if (cmd === 'plugin:event|listen') {
        var name = (args && args.event) || '';
        var id = nextEventId++;
        var rows = listeners.get(name) || [];
        rows.push({ id: id, handler: args && args.handler });
        listeners.set(name, rows);
        return Promise.resolve(id);
      }
      if (cmd === 'plugin:event|unlisten') {
        var nm = (args && args.event) || '';
        var keep = (listeners.get(nm) || []).filter(function (row) {
          return row.id !== (args && args.eventId);
        });
        listeners.set(nm, keep);
        return Promise.resolve(null);
      }
      // 内嵌面板没有真实窗口：窗口类命令一律返回安全默认值，别让界面报错。
      if (cmd.indexOf('plugin:window|') === 0) {
        if (cmd === 'plugin:window|is_maximized' || cmd === 'plugin:window|is_fullscreen') return Promise.resolve(false);
        if (cmd === 'plugin:window|scale_factor') return Promise.resolve(1);
        if (cmd.indexOf('position') > 0) return Promise.resolve({ x: 0, y: 0 });
        if (cmd.indexOf('size') > 0) return Promise.resolve({ width: window.innerWidth, height: window.innerHeight });
        return Promise.resolve(null);
      }
      // 文件选择需要真实宿主窗口，这里明确拒绝，让前端走自己的降级文案。
      return call(cmd, args);
    },
    transformCallback: transformCallback,
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { label: 'main', windowLabel: 'main' }
    },
    plugins: {}
  };

  try {
    var source = new EventSource('/__events?t=' + encodeURIComponent(token));
    source.onmessage = function (message) {
      var data;
      try { data = JSON.parse(message.data); } catch (e) { return; }
      var rows = listeners.get(data.event) || [];
      rows.slice().forEach(function (row) {
        var entry = callbacks.get(row.handler);
        if (!entry) return;
        try { entry.fn({ event: data.event, id: row.id, payload: data.payload }); } catch (e) {}
        if (entry.once) callbacks.delete(row.handler);
      });
    };
  } catch (e) {}
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// shim 必须定义 internals、标记嵌入态、拦窗口命令、走 EventSource，缺一不可。
    #[test]
    fn shim_covers_invoke_events_and_windows() {
        assert!(SHIM.contains("window.__TAURI_INTERNALS__"));
        assert!(SHIM.contains("window.__WB_DECK_EMBED__ = true"));
        assert!(SHIM.contains("plugin:event|listen"));
        assert!(SHIM.contains("EventSource"));
        assert!(SHIM.contains("plugin:window|"));
        assert!(SHIM.contains("__deck_t"));
        assert!(SHIM_VERSION >= 2);
    }
}

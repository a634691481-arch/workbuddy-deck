// 验证内嵌面板全链路：图标注入 → 点击 → iframe 加载 → 面板内 IPC 可用
const http = require('http');

function getJson(path) {
  return new Promise((res, rej) => {
    http.get('http://127.0.0.1:9222' + path, (r) => {
      let b = '';
      r.on('data', (d) => (b += d));
      r.on('end', () => res(JSON.parse(b)));
    }).on('error', rej);
  });
}

(async () => {
  const list = await getJson('/json/list');
  const page = list.find((x) => x.type === 'page' && /WorkBuddy/.test(x.title || ''));
  if (!page) { console.log('NO_PAGE'); process.exit(1); }

  const ws = new WebSocket(page.webSocketDebuggerUrl);
  let id = 0;
  const waiters = new Map();
  const send = (method, params) => new Promise((res) => {
    const mid = ++id;
    waiters.set(mid, res);
    ws.send(JSON.stringify({ id: mid, method, params }));
  });
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && waiters.has(d.id)) { waiters.get(d.id)(d); waiters.delete(d.id); }
  };
  await new Promise((r) => (ws.onopen = r));

  const evalJs = async (expression, awaitPromise = false) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise, returnByValue: true });
    if (r.result && r.result.exceptionDetails) return { __error: r.result.exceptionDetails.text };
    return r.result && r.result.result ? r.result.result.value : undefined;
  };

  console.log('icon:', await evalJs('!!document.getElementById("wb-deck-icon")'));

  // 点击图标（真实 click 事件）
  await evalJs('document.getElementById("wb-deck-icon") && document.getElementById("wb-deck-icon").click()');
  await new Promise((r) => setTimeout(r, 2500));

  const state = await evalJs(`(() => {
    const mask = document.getElementById('__wbDeckMask');
    const frame = document.getElementById('wb-deck-frame');
    return JSON.stringify({
      mask: !!mask,
      display: mask ? mask.style.display : null,
      frameSrc: frame ? frame.getAttribute('src') : null,
    });
  })()`);
  console.log('panel:', state);

  // 面板内文档标题（跨域拿不到 → 用 CDP 的 frame tree 判断加载成功）
  const tree = await send('Page.getFrameTree', {});
  const children = (tree.result && tree.result.frameTree && tree.result.frameTree.childFrames) || [];
  console.log('frames:', children.map((f) => f.frame.url).join(' | ') || '(none)');

  ws.close();
  process.exit(0);
})().catch((e) => { console.log('FATAL', e.message); process.exit(1); });

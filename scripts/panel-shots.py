import base64, json, os, subprocess, time, urllib.request, websocket, sys

CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
OUT = r"C:\Users\63469\Desktop\workbuddy-task-assistant\docs\screenshots"
PORT = 9333
PROFILE = os.path.join(os.environ.get("TEMP", r"C:\Windows\Temp"), "wb-shot-profile")


def panel_url():
    targets = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json/list", timeout=5))
    page = [t for t in targets if t.get("type") == "page"][0]
    ws = websocket.create_connection(page["webSocketDebuggerUrl"], timeout=10, suppress_origin=True)
    ws.send(json.dumps({"id": 1, "method": "Runtime.evaluate",
                        "params": {"expression": "window.__wbDeckUrl||''", "returnByValue": True}}))
    r = json.loads(ws.recv())
    ws.close()
    return r["result"]["result"]["value"]


class Cdp:
    def __init__(self, ws_url):
        self.ws = websocket.create_connection(ws_url, timeout=30, suppress_origin=True)
        self.i = 0

    def send(self, method, params=None):
        self.i += 1
        self.ws.send(json.dumps({"id": self.i, "method": method, "params": params or {}}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.i:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})


def main():
    url = panel_url()
    print("panel:", url)
    proc = subprocess.Popen([
        CHROME, "--headless=new", f"--remote-debugging-port={PORT}",
        f"--user-data-dir={PROFILE}", "--no-first-run", "--disable-gpu",
        "--hide-scrollbars", "--force-device-scale-factor=2", "about:blank",
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(40):
        try:
            targets = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/list", timeout=2))
            if any(t.get("type") == "page" for t in targets):
                break
        except Exception:
            pass
        time.sleep(0.4)
    page = [t for t in targets if t.get("type") == "page"][0]
    c = Cdp(page["webSocketDebuggerUrl"])
    c.send("Page.enable")
    c.send("Emulation.setDeviceMetricsOverride",
           {"width": 1440, "height": 900, "deviceScaleFactor": 2, "mobile": False})

    sections = ["accounts", "tasks", "credits", "tokens", "logs", "settings"]
    names = ["01-accounts", "02-tasks", "03-credits", "04-tokens", "05-logs", "06-settings"]

    for lang in ("zh", "en"):
        c.send("Page.navigate", {"url": url})
        time.sleep(2)
        c.send("Runtime.evaluate", {"expression":
            f"localStorage.setItem('wb-ta/locale', '{lang}')"})
        for sec, name in zip(sections, names):
            c.send("Page.navigate", {"url": f"{url}#{sec}"})
            time.sleep(3.2)
            # 强制重载同一 hash 视图时用 go 导航不可靠，改用 hashchange
            c.send("Runtime.evaluate", {"expression":
                f"location.hash='{sec}'; window.dispatchEvent(new HashChangeEvent('hashchange'))"})
            time.sleep(1.2)
            shot = c.send("Page.captureScreenshot", {"format": "png", "captureBeyondViewport": False})
            suffix = "" if lang == "zh" else "-en"
            path = os.path.join(OUT, f"{name}{suffix}.png")
            with open(path, "wb") as f:
                f.write(base64.b64decode(shot["data"]))
            print("saved", path)
    proc.terminate()


if __name__ == "__main__":
    main()

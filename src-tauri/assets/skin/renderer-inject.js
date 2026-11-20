((cssText, artDataUrls, theme, skinVersion) => {
  const STATE_KEY = "__WORKBUDDY_DREAM_SKIN_STATE__";
  const STYLE_ID = "workbuddy-dream-skin-style";
  const CHROME_ID = "workbuddy-dream-skin-chrome";
  window.__WORKBUDDY_DREAM_SKIN_DISABLED__ = false;

  const previous = window[STATE_KEY];
  previous?.observer?.disconnect();
  if (previous?.timer) clearInterval(previous.timer);
  if (previous?.scheduler?.timeout) clearTimeout(previous.scheduler.timeout);

  const createArtUrl = (artDataUrl) => {
    if (!artDataUrl) return null;
    const comma = artDataUrl.indexOf(",");
    const mime = artDataUrl.slice(5, artDataUrl.indexOf(";")) || "image/png";
    const binary = atob(artDataUrl.slice(comma + 1));
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return URL.createObjectURL(new Blob([bytes], { type: mime }));
  };
  const artUrlCache = new Map();
  const artUrls = Object.fromEntries(Object.entries(artDataUrls || {}).map(([role, value]) => {
    if (value && !artUrlCache.has(value)) artUrlCache.set(value, createArtUrl(value));
    return [role, value ? artUrlCache.get(value) : null];
  }));
  for (const oldUrl of new Set(Object.values(previous?.artUrls || {}))) if (oldUrl) URL.revokeObjectURL(oldUrl);

  const colorWithAlpha = (hex, alpha) => {
    const match = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex || "");
    if (!match) return `rgba(7, 25, 30, ${alpha})`;
    return `rgba(${parseInt(match[1], 16)}, ${parseInt(match[2], 16)}, ${parseInt(match[3], 16)}, ${alpha})`;
  };

  const positionValue = (position, edge, fallback) => {
    const match = new RegExp(`(?:^|\\s)${edge}\\s+([0-9]+(?:\\.[0-9]+)?px)`, "i").exec(position || "");
    return match?.[1] || fallback;
  };
  const decorationWidth = (size) => /^\s*([0-9]+(?:\.[0-9]+)?px)/i.exec(size || "")?.[1] || "154px";
  const decorationArtUrl = theme.decoration.source === "hero-full" ? artUrls.hero : artUrls.character;

  const setThemeVariables = (root) => {
    const values = {
      "--wbds-art": artUrls.background ? `url("${artUrls.background}")` : "none",
      "--wbds-background-art": artUrls.background ? `url("${artUrls.background}")` : "none",
      "--wbds-hero-art": artUrls.hero ? `url("${artUrls.hero}")` : "none",
      "--wbds-character-art": decorationArtUrl ? `url("${decorationArtUrl}")` : "none",
      "--wbds-background-position": theme.layout.backgroundPosition,
      "--wbds-background-size": theme.layout.backgroundSize,
      "--wbds-hero-position": theme.layout.heroPosition,
      "--wbds-hero-size": theme.layout.heroSize,
      "--wbds-character-position": theme.layout.characterPosition,
      "--wbds-character-size": theme.layout.characterSize,
      "--wbds-character-opacity": String(theme.effects.characterOpacity),
      "--wbds-decoration-right": positionValue(theme.layout.characterPosition, "right", "14px"),
      "--wbds-decoration-top": positionValue(theme.layout.characterPosition, "top", "170px"),
      "--wbds-decoration-width": decorationWidth(theme.layout.characterSize),
      "--wbds-panel-weight": `${Math.round(theme.effects.panelOpacity * 100)}%`,
      "--wbds-background-overlay-strong": colorWithAlpha(theme.colors.background, Math.min(1, theme.effects.backgroundOverlay + .1)),
      "--wbds-background-overlay": colorWithAlpha(theme.colors.background, theme.effects.backgroundOverlay),
      "--wbds-hero-overlay-strong": colorWithAlpha(theme.colors.background, Math.min(1, theme.effects.heroOverlay + .12)),
      "--wbds-hero-overlay": colorWithAlpha(theme.colors.background, theme.effects.heroOverlay),
      "--wbds-background": theme.colors.background,
      "--wbds-panel": theme.colors.panel,
      "--wbds-panel-alt": theme.colors.panelAlt,
      "--wbds-accent": theme.colors.accent,
      "--wbds-accent-alt": theme.colors.accentAlt,
      "--wbds-secondary": theme.colors.secondary,
      "--wbds-highlight": theme.colors.highlight,
      "--wbds-text": theme.colors.text,
      "--wbds-muted": theme.colors.muted,
      "--wbds-line": theme.colors.line,
    };
    for (const [key, value] of Object.entries(values)) root.style.setProperty(key, value);
  };

  const findWorkspace = () => document.querySelector(
    ".claw-workspace, .colleague-chat-page__shell, .teams-content-wrapper, .colleagues-chat-float__main, main"
  );

  const ensure = () => {
    if (window.__WORKBUDDY_DREAM_SKIN_DISABLED__) return;
    const root = document.documentElement;
    if (!root || !document.body) return;
    root.classList.add("workbuddy-dream-skin");
    const drawnShape = (theme.decoration.shape || "polaroid") !== "polaroid";
    root.classList.toggle("wbds-has-character", Boolean(decorationArtUrl) || drawnShape);
    root.dataset.workbuddyDreamSkinStyle = theme.style;
    root.dataset.workbuddyDreamSkinAppearance = theme.appearance;
    root.dataset.workbuddyDreamSkinDecorationMode = theme.decoration.mode;
    root.dataset.workbuddyDreamSkinDecorationVariant = theme.decoration.variant;
    root.dataset.workbuddyDreamSkinDecorationShape = theme.decoration.shape || "polaroid";
    root.dataset.workbuddyDreamSkinDecorationAnchor = theme.layout.decorationAnchor || "top-right";
    root.dataset.workbuddyDreamSkinVersion = skinVersion;
    document.body.classList.add("workbuddy-dream-skin-body");
    setThemeVariables(root);

    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      (document.head || root).appendChild(style);
    }
    if (style.dataset.version !== skinVersion || style.textContent !== cssText) {
      style.textContent = cssText;
      style.dataset.version = skinVersion;
    }

    let chrome = document.getElementById(CHROME_ID);
    if (!chrome) {
      chrome = document.createElement("div");
      chrome.id = CHROME_ID;
      chrome.setAttribute("aria-hidden", "true");
      chrome.innerHTML = `
        <div class="wbds-grain"></div>
        <div class="wbds-character">
          <div class="wbds-charm-orbit"><i></i><i></i><i></i></div>
          <div class="wbds-character-card"><img alt=""><svg class="wbds-stamp-face" viewBox="0 0 200 200" aria-hidden="true"><defs><path id="wbds-stamp-arc-top" d="M 30,105 A 70,70 0 0 1 170,105"/><path id="wbds-stamp-arc-bottom" d="M 34,96 A 66,66 0 0 0 166,96"/></defs><circle class="wbds-stamp-ring-outer" cx="100" cy="100" r="94"/><circle class="wbds-stamp-ring-inner" cx="100" cy="100" r="82"/><g class="wbds-stamp-emblem" transform="translate(100 108)"><path d="M0,-30 C-18,-28 -28,-14 -28,0 C-28,14 -18,22 -6,24 C-4,16 0,10 0,10 C0,10 4,16 6,24 C18,22 28,14 28,0 C28,-14 18,-28 0,-30 Z"/><path d="M0,10 L0,26" stroke-linecap="round"/></g><text class="wbds-stamp-arc-top" dy="0"><textPath href="#wbds-stamp-arc-top" startOffset="50%" text-anchor="middle">SUNLIT CAMPUS</textPath></text><text class="wbds-stamp-arc-bottom" dy="0"><textPath href="#wbds-stamp-arc-bottom" startOffset="50%" text-anchor="middle">EST · NOTE 01</textPath></text></svg></div>
          <div class="wbds-sparkles"><i></i><i></i><i></i><i></i></div>
        </div>
        <div class="wbds-sonar"><i></i><i></i><i></i><b></b></div>
        <div class="wbds-brand"><span class="wbds-pulse"></span><span><strong></strong><small></small></span></div>
        <div class="wbds-status"><span></span><b></b></div>
        <div class="wbds-coordinate">31.2304 N<br>121.4737 E</div>`;
      document.body.appendChild(chrome);
    }
    if (!chrome.querySelector(".wbds-character")) {
      const character = document.createElement("div");
      character.className = "wbds-character";
      chrome.querySelector(".wbds-grain")?.after(character);
    }
    const character = chrome.querySelector(".wbds-character");
    if (!character.querySelector(".wbds-character-card")) {
      character.innerHTML = '<div class="wbds-charm-orbit"><i></i><i></i><i></i></div><div class="wbds-character-card"><img alt=""><svg class="wbds-stamp-face" viewBox="0 0 200 200" aria-hidden="true"><defs><path id="wbds-stamp-arc-top" d="M 30,105 A 70,70 0 0 1 170,105"/><path id="wbds-stamp-arc-bottom" d="M 34,96 A 66,66 0 0 0 166,96"/></defs><circle class="wbds-stamp-ring-outer" cx="100" cy="100" r="94"/><circle class="wbds-stamp-ring-inner" cx="100" cy="100" r="82"/><g class="wbds-stamp-emblem" transform="translate(100 108)"><path d="M0,-30 C-18,-28 -28,-14 -28,0 C-28,14 -18,22 -6,24 C-4,16 0,10 0,10 C0,10 4,16 6,24 C18,22 28,14 28,0 C28,-14 18,-28 0,-30 Z"/><path d="M0,10 L0,26" stroke-linecap="round"/></g><text class="wbds-stamp-arc-top" dy="0"><textPath href="#wbds-stamp-arc-top" startOffset="50%" text-anchor="middle">SUNLIT CAMPUS</textPath></text><text class="wbds-stamp-arc-bottom" dy="0"><textPath href="#wbds-stamp-arc-bottom" startOffset="50%" text-anchor="middle">EST · NOTE 01</textPath></text></svg></div><div class="wbds-sparkles"><i></i><i></i><i></i><i></i></div>';
    }
    if (!character.querySelector(".wbds-charm-orbit")) {
      const orbit = document.createElement("div");
      orbit.className = "wbds-charm-orbit";
      orbit.innerHTML = "<i></i><i></i><i></i>";
      character.prepend(orbit);
    }
    if (!character.querySelector(".wbds-sparkles")) {
      const sparkles = document.createElement("div");
      sparkles.className = "wbds-sparkles";
      sparkles.innerHTML = "<i></i><i></i><i></i><i></i>";
      character.append(sparkles);
    }
    const characterCard = character.querySelector(".wbds-character-card");
    if (characterCard && !characterCard.querySelector(".wbds-stamp-face")) {
      characterCard.insertAdjacentHTML("beforeend", '<svg class="wbds-stamp-face" viewBox="0 0 200 200" aria-hidden="true"><defs><path id="wbds-stamp-arc-top" d="M 30,105 A 70,70 0 0 1 170,105"/><path id="wbds-stamp-arc-bottom" d="M 34,96 A 66,66 0 0 0 166,96"/></defs><circle class="wbds-stamp-ring-outer" cx="100" cy="100" r="94"/><circle class="wbds-stamp-ring-inner" cx="100" cy="100" r="82"/><g class="wbds-stamp-emblem" transform="translate(100 108)"><path d="M0,-30 C-18,-28 -28,-14 -28,0 C-28,14 -18,22 -6,24 C-4,16 0,10 0,10 C0,10 4,16 6,24 C18,22 28,14 28,0 C28,-14 18,-28 0,-30 Z"/><path d="M0,10 L0,26" stroke-linecap="round"/></g><text class="wbds-stamp-arc-top" dy="0"><textPath href="#wbds-stamp-arc-top" startOffset="50%" text-anchor="middle">SUNLIT CAMPUS</textPath></text><text class="wbds-stamp-arc-bottom" dy="0"><textPath href="#wbds-stamp-arc-bottom" startOffset="50%" text-anchor="middle">EST · NOTE 01</textPath></text></svg>');
    }
    if (characterCard && !characterCard.querySelector(".wbds-sticker-face")) {
      characterCard.insertAdjacentHTML("beforeend", '<svg class="wbds-sticker-face" viewBox="0 0 200 160" aria-hidden="true"><g class="wbds-sticker-rose" transform="translate(100 62)"><ellipse class="wbds-sticker-leaf" cx="-32" cy="12" rx="18" ry="7" transform="rotate(-24 -32 12)"/><ellipse class="wbds-sticker-leaf" cx="32" cy="12" rx="18" ry="7" transform="rotate(24 32 12)"/><circle class="wbds-sticker-petal wbds-sticker-petal-side" cx="-18" cy="4" r="14"/><circle class="wbds-sticker-petal wbds-sticker-petal-side" cx="18" cy="4" r="14"/><circle class="wbds-sticker-petal wbds-sticker-petal-side" cx="0" cy="-14" r="14"/><circle class="wbds-sticker-petal" cx="0" cy="0" r="18"/><circle class="wbds-sticker-petal-core" cx="0" cy="0" r="8"/><circle class="wbds-sticker-petal-highlight" cx="-3" cy="-3" r="3"/></g><text class="wbds-sticker-label" x="100" y="118" text-anchor="middle">Dream Notes</text><text class="wbds-sticker-sub" x="100" y="140" text-anchor="middle">♡ pink diary · 01</text><g class="wbds-sticker-corner"><circle cx="14" cy="14" r="2.5"/><circle cx="186" cy="14" r="2.5"/><circle cx="14" cy="146" r="2.5"/><circle cx="186" cy="146" r="2.5"/></g></svg>');
    }
    if (characterCard && !characterCard.querySelector(".wbds-leaf-face")) {
      characterCard.insertAdjacentHTML("beforeend", '<svg class="wbds-leaf-face" viewBox="0 0 160 200" aria-hidden="true"><g class="wbds-leaf-fern" transform="translate(80 92)"><path d="M0,-52 Q0,-24 0,42" class="wbds-leaf-stem"/><ellipse cx="-11" cy="-38" rx="9" ry="3.5" transform="rotate(-32 -11 -38)"/><ellipse cx="11" cy="-38" rx="9" ry="3.5" transform="rotate(32 11 -38)"/><ellipse cx="-14" cy="-22" rx="12" ry="4.5" transform="rotate(-32 -14 -22)"/><ellipse cx="14" cy="-22" rx="12" ry="4.5" transform="rotate(32 14 -22)"/><ellipse cx="-16" cy="-6" rx="14" ry="5" transform="rotate(-32 -16 -6)"/><ellipse cx="16" cy="-6" rx="14" ry="5" transform="rotate(32 16 -6)"/><ellipse cx="-15" cy="12" rx="12" ry="4.5" transform="rotate(-32 -15 12)"/><ellipse cx="15" cy="12" rx="12" ry="4.5" transform="rotate(32 15 12)"/><ellipse cx="-12" cy="28" rx="9" ry="3.5" transform="rotate(-32 -12 28)"/><ellipse cx="12" cy="28" rx="9" ry="3.5" transform="rotate(32 12 28)"/></g><text class="wbds-leaf-latin" x="80" y="162" text-anchor="middle">Folium Menthae</text><text class="wbds-leaf-catalog" x="80" y="180" text-anchor="middle">SPECIMEN · No. 03</text><g class="wbds-leaf-corner"><circle cx="20" cy="20" r="9"/><text x="20" y="23" text-anchor="middle">26</text></g></svg>');
    }
    if (characterCard && !characterCard.querySelector(".wbds-heart-face")) {
      characterCard.insertAdjacentHTML("beforeend", '<svg class="wbds-heart-face" viewBox="0 0 200 180" aria-hidden="true" preserveAspectRatio="none"><path class="wbds-heart-outline" d="M 100,158 C 100,158 28,110 28,58 C 28,25 60,12 100,52 C 140,12 172,25 172,58 C 172,110 100,158 100,158 Z"/><g class="wbds-heart-ribbon" transform="translate(100 8)"><path d="M -18,0 L -6,10 L 0,4 L 6,10 L 18,0 L 12,-6 L 0,0 L -12,-6 Z"/></g></svg>');
    }
    if (characterCard && !characterCard.querySelector(".wbds-porthole-face")) {
      characterCard.insertAdjacentHTML("beforeend", '<svg class="wbds-porthole-face" viewBox="0 0 200 200" aria-hidden="true"><g class="wbds-porthole-rivets"><circle cx="100" cy="10" r="3"/><circle cx="164" cy="36" r="3"/><circle cx="190" cy="100" r="3"/><circle cx="164" cy="164" r="3"/><circle cx="100" cy="190" r="3"/><circle cx="36" cy="164" r="3"/><circle cx="10" cy="100" r="3"/><circle cx="36" cy="36" r="3"/></g><path class="wbds-porthole-glare" d="M 60,30 Q 40,60 45,100 Q 30,80 25,60 Z"/></svg>');
    }
    const characterImage = character.querySelector("img");
    if (characterImage && characterImage.src !== (decorationArtUrl || "")) characterImage.src = decorationArtUrl || "";
    chrome.querySelector(".wbds-brand strong").textContent = theme.name;
    chrome.querySelector(".wbds-brand small").textContent = theme.eyebrow;
    chrome.querySelector(".wbds-status span").textContent = theme.status;
    chrome.querySelector(".wbds-status b").textContent = theme.tagline;

    const workspace = findWorkspace();
    if (workspace) {
      const box = workspace.getBoundingClientRect();
      chrome.style.setProperty("--wbds-shell-left", `${Math.max(0, Math.round(box.left))}px`);
      chrome.style.setProperty("--wbds-shell-top", `${Math.max(0, Math.round(box.top))}px`);
      chrome.style.setProperty("--wbds-shell-width", `${Math.max(0, Math.round(box.width))}px`);
      chrome.style.setProperty("--wbds-shell-height", `${Math.max(0, Math.round(box.height))}px`);
    }
  };

  const cleanup = () => {
    window.__WORKBUDDY_DREAM_SKIN_DISABLED__ = true;
    const root = document.documentElement;
    root?.classList.remove("workbuddy-dream-skin");
    root?.classList.remove("wbds-has-character");
    if (root) {
      delete root.dataset.workbuddyDreamSkinVersion;
      delete root.dataset.workbuddyDreamSkinStyle;
      delete root.dataset.workbuddyDreamSkinAppearance;
      delete root.dataset.workbuddyDreamSkinDecorationMode;
      delete root.dataset.workbuddyDreamSkinDecorationVariant;
      [...root.style].filter((name) => name.startsWith("--wbds-")).forEach((name) => root.style.removeProperty(name));
    }
    document.body?.classList.remove("workbuddy-dream-skin-body");
    document.getElementById(STYLE_ID)?.remove();
    document.getElementById(CHROME_ID)?.remove();
    const state = window[STATE_KEY];
    state?.observer?.disconnect();
    if (state?.timer) clearInterval(state.timer);
    if (state?.scheduler?.timeout) clearTimeout(state.scheduler.timeout);
    for (const artUrl of new Set(Object.values(state?.artUrls || {}))) if (artUrl) URL.revokeObjectURL(artUrl);
    delete window[STATE_KEY];
    return true;
  };

  const scheduler = { timeout: null };
  const scheduleEnsure = () => {
    if (scheduler.timeout) clearTimeout(scheduler.timeout);
    scheduler.timeout = setTimeout(() => {
      scheduler.timeout = null;
      ensure();
    }, 180);
  };
  const observer = new MutationObserver(scheduleEnsure);
  observer.observe(document.documentElement, { childList: true, subtree: true });
  const timer = setInterval(ensure, 5000);
  window[STATE_KEY] = { ensure, cleanup, observer, timer, scheduler, artUrls, version: skinVersion, themeId: theme.id };
  ensure();
  return { installed: true, version: skinVersion, themeId: theme.id };
})(__WORKBUDDY_SKIN_CSS_JSON__, __WORKBUDDY_SKIN_ART_JSON__, __WORKBUDDY_SKIN_THEME_JSON__, __WORKBUDDY_SKIN_VERSION_JSON__)

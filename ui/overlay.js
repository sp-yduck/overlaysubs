const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let cfg = { ...OVERLAY_DEFAULTS };
let uiLang = "ja";
// Set while the appearance settings are open, so changes can be checked with translation stopped.
let preview = false;
const caps = new Map(); // id -> {caption, at}
const esc = (s) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
/** False for text made only of punctuation and spaces, such as "…" or "". */
const hasWords = (s) => /[\p{L}\p{N}]/u.test(s || "");
const t = (k) => (I18N[uiLang] || I18N.ja)[k] || k;

function apply(settings) {
  cfg = { ...OVERLAY_DEFAULTS, ...settings.overlay };
  uiLang = settings.ui_lang || detectUiLang(navigator.languages);
  const r = document.documentElement.style;
  r.setProperty("--size", cfg.fontSize + "px");
  r.setProperty("--font", cfg.font);
  r.setProperty("--bg", cfg.bg);
  document.getElementById("credit").hidden = !cfg.showCredit;
  invoke("set_click_through", { on: !!cfg.clickThrough }).catch(() => {});
  draw();
}

const captionHtml = (c) =>
  `<div class="cap ${c.stage}">${cfg.showSource ? `<span class="src">${esc(c.source)}</span>` : ""}<span class="tr">${esc(c.translation)}</span></div>`;

/** Two sample captions so the "lines" setting is previewed too (real captions take priority). */
function previewHtml() {
  return [1, 2]
    .map((n) => ({ stage: "final", source: t(`preview_src_${n}`), translation: t(`preview_tr_${n}`) }))
    .slice(-cfg.lines)
    .map(captionHtml)
    .join("");
}

function draw() {
  const now = Date.now();
  for (const [id, c] of caps) if (c.caption.stage === "final" && now - c.at > cfg.linger * 1000) caps.delete(id);
  // A caption with no readable translation yet is left out rather than drawn as "…" over the previous lines.
  const shown = [...caps.values()].filter(({ caption: c }) => hasWords(c.translation)).slice(-cfg.lines);
  document.getElementById("lines").innerHTML = shown.length
    ? shown.map(({ caption: c }) => captionHtml(c)).join("")
    : preview
      ? previewHtml()
      : "";
}

/**
 * Keeps the previous translation while the new one would only take text off the screen:
 * one without words (empty or just "…"), or a finalized translation still streaming in
 * that is shorter than the draft. Otherwise the new caption replaces it.
 */
function merge(prev, next) {
  if (!prev || !hasWords(prev.translation)) return next;
  const keep = !hasWords(next.translation) || (next.stage === "streaming" && next.translation.length < prev.translation.length);
  return keep ? { ...next, translation: prev.translation } : next;
}

listen("caption", (e) => {
  const prev = caps.get(e.payload.id)?.caption;
  caps.set(e.payload.id, { caption: merge(prev, e.payload), at: Date.now() });
  draw();
});
listen("settings", (e) => apply(e.payload));
// Hidden windows have their timers throttled, so drop captions as the overlay disappears
// rather than when it is shown again (that could discard a caption emitted in between).
listen("overlay-hidden", () => { caps.clear(); draw(); });
// The settings window turns the preview on while its appearance page is open and translation is stopped.
listen("overlay-preview", (e) => {
  preview = !!e.payload;
  draw();
});
setInterval(draw, 500);
invoke("snapshot").then((s) => apply(s.settings));

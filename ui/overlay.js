const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let cfg = { ...OVERLAY_DEFAULTS };
let uiLang = "ja";
// Set while the appearance settings are open, so changes can be checked with translation stopped.
let preview = false;
const caps = new Map(); // id -> {caption, at}
const esc = (s) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
const t = (k) => (I18N[uiLang] || I18N.ja)[k] || k;

function apply(settings) {
  cfg = { ...OVERLAY_DEFAULTS, ...settings.overlay };
  uiLang = settings.ui_lang || uiLang;
  const r = document.documentElement.style;
  r.setProperty("--size", cfg.fontSize + "px");
  r.setProperty("--font", cfg.font);
  r.setProperty("--bg", cfg.bg);
  document.getElementById("credit").hidden = !cfg.showCredit;
  invoke("set_click_through", { on: !!cfg.clickThrough }).catch(() => {});
  draw();
}

const captionHtml = (c) =>
  `<div class="cap ${c.stage}">${cfg.showSource ? `<span class="src">${esc(c.source)}</span>` : ""}<span class="tr">${esc(c.translation || "…")}</span></div>`;

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
  const shown = [...caps.values()].slice(-cfg.lines);
  document.getElementById("lines").innerHTML = shown.length
    ? shown.map(({ caption: c }) => captionHtml(c)).join("")
    : preview
      ? previewHtml()
      : "";
}

listen("caption", (e) => {
  caps.set(e.payload.id, { caption: e.payload, at: Date.now() });
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

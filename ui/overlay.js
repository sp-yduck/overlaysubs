const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let cfg = { ...OVERLAY_DEFAULTS };
const caps = new Map(); // id -> {caption, at}
const esc = (s) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

function apply(settings) {
  cfg = { ...OVERLAY_DEFAULTS, ...settings.overlay };
  const r = document.documentElement.style;
  r.setProperty("--size", cfg.fontSize + "px");
  r.setProperty("--font", cfg.font);
  r.setProperty("--bg", cfg.bg);
  document.getElementById("credit").hidden = !cfg.showCredit;
  invoke("set_click_through", { on: !!cfg.clickThrough }).catch(() => {});
  draw();
}

function draw() {
  const now = Date.now();
  for (const [id, c] of caps) if (c.caption.stage === "final" && now - c.at > cfg.linger * 1000) caps.delete(id);
  const shown = [...caps.values()].slice(-cfg.lines);
  document.getElementById("lines").innerHTML = shown
    .map(({ caption: c }) => `<div class="cap ${c.stage}">${cfg.showSource ? `<span class="src">${esc(c.source)}</span>` : ""}<span class="tr">${esc(c.translation || "…")}</span></div>`)
    .join("");
}

listen("caption", (e) => {
  caps.set(e.payload.id, { caption: e.payload, at: Date.now() });
  draw();
});
listen("settings", (e) => apply(e.payload));
// Hidden windows have their timers throttled, so drop captions as the overlay disappears
// rather than when it is shown again (that could discard a caption emitted in between).
listen("overlay-hidden", () => { caps.clear(); draw(); });
setInterval(draw, 500);
invoke("snapshot").then((s) => apply(s.settings));

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let cfg = { ...OVERLAY_DEFAULTS };
let uiLang = "en";
// Set while the appearance settings are open, so changes can be checked with translation stopped.
let preview = false;
const caps = new Map(); // id -> caption
const esc = (s) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
/** False for text made only of punctuation and spaces, such as "…" or "". */
const hasWords = (s) => /[\p{L}\p{N}]/u.test(s || "");
const t = (k) => (I18N[uiLang] || I18N.en)[k] || k;
const linesEl = document.getElementById("lines");
// Wheel delta treated as one mouse-wheel notch (Chromium and WebView2 send about 100 px per notch).
const WHEEL_NOTCH = 100;
// How much of a caption line one notch moves. Half a line lets the reader stop between lines
// instead of jumping a whole large caption per notch.
const WHEEL_LINES = 0.5;
// Whether the newest caption stays in view. It turns false when the reader scrolls up.
let follow = true;
// Captions kept on screen while the reader is scrolled up, so new ones do not push out the lines being read.
let frozen = null;
let shownIds = [];
let lastHtml = "";
// When the subtitles were last emitted or updated, used to clear the overlay after a pause.
let lastUpdate = 0;
// The wheel moves by fractions of a pixel, but scrollTop rounds every assignment down; keep the
// exact target so slow trackpad deltas are not lost, and drop it when something else moves the view.
let wheelTarget = null;

function apply(settings) {
  // A font change reflows the lines even when their text does not change, so remember the line
  // being read before the styles change and put it back afterwards.
  const anchor = follow ? null : visibleAnchor();
  cfg = { ...OVERLAY_DEFAULTS, ...settings.overlay };
  uiLang = settings.ui_lang || detectUiLang(navigator.languages);
  document.documentElement.lang = uiLang || "en";
  showGrip();
  const r = document.documentElement.style;
  r.setProperty("--size", cfg.fontSize + "px");
  r.setProperty("--font", cfg.font);
  r.setProperty("--bg", cfg.bg);
  invoke("set_click_through", { on: !!cfg.clickThrough }).catch(() => {});
  draw();
  restoreAnchor(anchor);
}

const captionHtml = (c) =>
  `<div class="cap ${c.stage}"${c.id == null ? "" : ` data-id="${c.id}"`}>${cfg.showSource ? `<span class="src">${esc(c.source)}</span>` : ""}<span class="tr">${esc(c.translation)}</span></div>`;

/** Two sample captions so the "lines" setting is previewed too (real captions take priority). */
function previewHtml() {
  return [1, 2]
    .map((n) => ({ stage: "final", source: t(`preview_src_${n}`), translation: t(`preview_tr_${n}`) }))
    .slice(-cfg.lines)
    .map(captionHtml)
    .join("");
}

/** Captions with a readable translation, in arrival order. */
const readable = () => [...caps.values()].filter((c) => hasWords(c.translation));

function draw() {
  const now = Date.now();
  // Captions do not expire one by one: that removed older lines before the reader could scroll
  // back to them. Instead clear the whole stack once the subtitles have not been updated for the
  // configured time (a pause in speech).
  if (caps.size && now - lastUpdate > cfg.linger * 1000) {
    caps.clear();
    frozen = null;
    follow = true;
  }
  // Only the newest captions can be shown or scrolled back to, so a long session does not need to
  // keep all of them; the frozen lines stay while they are on screen.
  const limit = Math.max(4 * cfg.lines, 40);
  for (const id of caps.keys()) {
    if (caps.size <= limit) break;
    if (!frozen?.includes(id)) caps.delete(id);
  }

  const entries = readable();
  const shown = frozen
    ? frozen.map((id) => caps.get(id)).filter((c) => c && hasWords(c.translation))
    : entries.slice(-cfg.lines);
  const html = shown.length ? shown.map((c) => captionHtml(c)).join("") : preview ? previewHtml() : "";
  if (html !== lastHtml) {
    // Replacing the HTML resets the scroll position, so keep the reader where they were.
    const anchor = follow ? null : visibleAnchor();
    const top = linesEl.scrollTop;
    linesEl.innerHTML = html;
    lastHtml = html;
    if (follow) linesEl.scrollTop = linesEl.scrollHeight;
    else if (!restoreAnchor(anchor)) linesEl.scrollTop = top;
  } else if (follow) {
    // The same captions at a larger font size can be taller than the overlay again.
    linesEl.scrollTop = linesEl.scrollHeight;
  }
  shownIds = shown.map((c) => c.id).filter((id) => id != null);
  // The frozen lines no longer overflow (the font shrank), so there is nothing left to scroll;
  // go back to the newest captions.
  if (frozen && linesEl.scrollHeight <= linesEl.clientHeight + 1) unfreeze();
}

/** Back to the live view: draw the newest captions again. */
function unfreeze() {
  frozen = null;
  follow = true;
  draw();
}

/** The first caption in view and its distance from the top of the overlay, so a re-render can keep it there. */
function visibleAnchor() {
  const view = linesEl.getBoundingClientRect();
  for (const el of linesEl.children) {
    const rect = el.getBoundingClientRect();
    if (rect.bottom <= view.top) continue;
    return el.dataset.id ? { id: el.dataset.id, at: rect.top - view.top } : null;
  }
  return null;
}

/** Puts the anchored caption back at the same distance from the top of the overlay. */
function restoreAnchor(anchor) {
  const el = anchor && linesEl.querySelector(`[data-id="${CSS.escape(anchor.id)}"]`);
  if (!el) return false;
  const view = linesEl.getBoundingClientRect();
  linesEl.scrollTop += el.getBoundingClientRect().top - view.top - anchor.at;
  return true;
}

/** Average height of one caption line in the current view, so a notch can move half a line of it. */
function captionLineHeight() {
  const caps = linesEl.children;
  const gap = parseFloat(getComputedStyle(linesEl).rowGap) || 0;
  if (!caps.length) return cfg.fontSize * 1.3 + gap;
  let total = 0;
  for (const el of caps) total += el.offsetHeight;
  return total / caps.length + gap;
}

/** Keeps the live view in step with where the view is: frozen while scrolled up, live at the bottom. */
function syncFollow() {
  if (linesEl.scrollHeight - linesEl.scrollTop - linesEl.clientHeight > 0.5) {
    if (follow) {
      follow = false;
      if (!preview && !frozen && shownIds.length) frozen = [...shownIds];
    }
  } else if (!follow || frozen) {
    unfreeze();
  }
}

/**
 * Vertical scrolling for lines taller than the overlay. One wheel notch moves half a caption line,
 * so large captions do not jump a whole screen at a time; the scrollbar on the right shows the
 * position and can set it too.
 */
window.addEventListener(
  "wheel",
  (e) => {
    const max = linesEl.scrollHeight - linesEl.clientHeight;
    if (max <= 0) return;
    e.preventDefault();
    const delta = e.deltaMode === 1 ? e.deltaY * WHEEL_NOTCH : e.deltaY;
    const step = (delta / WHEEL_NOTCH) * WHEEL_LINES * captionLineHeight();
    wheelTarget = Math.max(0, Math.min(max, (wheelTarget ?? linesEl.scrollTop) + step));
    // Reach the ends exactly even though scrollTop itself is rounded down.
    if (max > 0.5) {
      if (wheelTarget > max - 0.5) wheelTarget = max;
      else if (wheelTarget < 0.5) wheelTarget = 0;
    }
    linesEl.scrollTop = wheelTarget;
    syncFollow();
  },
  { passive: false },
);

// The scrollbar can also move the view (drag, track click or keyboard); keep the state in step with it.
linesEl.addEventListener("scroll", () => {
  // A scroll from elsewhere (scrollbar, re-render, anchor restore) wins over the pending wheel target.
  if (wheelTarget != null && Math.abs(linesEl.scrollTop - wheelTarget) > 1) wheelTarget = null;
  syncFollow();
});

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
  lastUpdate = Date.now();
  caps.set(e.payload.id, merge(caps.get(e.payload.id), e.payload));
  draw();
});
listen("settings", (e) => apply(e.payload));
// Hidden windows have their timers throttled, so drop captions as the overlay disappears
// rather than when it is shown again (that could discard a caption emitted in between).
listen("overlay-hidden", () => {
  caps.clear();
  lastUpdate = 0;
  follow = true;
  frozen = null;
  wheelTarget = null;
  draw();
});
// The settings window turns the preview on while its appearance page is open and translation is stopped.
listen("overlay-preview", (e) => {
  preview = !!e.payload;
  follow = true;
  frozen = null;
  wheelTarget = null;
  showGrip();
  draw();
});

/** The hint is the grip's own text: a child element would not be part of the drag region. */
function showGrip() {
  document.body.classList.toggle("preview", preview);
  document.getElementById("grip").textContent = preview ? `⠿ ${t("drag_hint")}` : "";
}
setInterval(draw, 500);
invoke("snapshot").then((s) => apply(s.settings));

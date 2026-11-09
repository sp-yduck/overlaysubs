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
// Wheel movement that steps one caption line. A mouse notch is usually 100 or 120px, while a
// trackpad sends many small deltas, so they accumulate until this much has been scrolled.
const WHEEL_STEP = 40;
// Whether the newest caption stays in view. It turns false when the reader scrolls up.
let follow = true;
// Captions kept on screen while the reader is scrolled up, so new ones do not push out the lines being read.
let frozen = null;
let shownIds = [];
let lastHtml = "";
let wheelDelta = 0;
// When the subtitles were last emitted or updated, used to clear the overlay after a pause.
let lastUpdate = 0;

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

/**
 * Scroll positions that put each caption's bottom at the bottom of the overlay, plus the very top.
 * Stepping between them moves a whole line, so a wheel notch never leaves a caption cut in half.
 */
function lineStops() {
  const max = linesEl.scrollHeight - linesEl.clientHeight;
  if (max <= 0) return [];
  const pad = parseFloat(getComputedStyle(linesEl).paddingBottom) || 0;
  const view = linesEl.getBoundingClientRect();
  const stops = [0, max];
  for (const el of linesEl.children) {
    const rect = el.getBoundingClientRect();
    stops.push(Math.min(max, Math.max(0, Math.round(linesEl.scrollTop + rect.bottom - view.bottom + pad))));
  }
  return [...new Set(stops)].sort((a, b) => a - b);
}

/** Index of the stop closest to the current scroll position. */
function nearestStop(stops) {
  let best = 0;
  for (let i = 1; i < stops.length; i++)
    if (Math.abs(stops[i] - linesEl.scrollTop) < Math.abs(stops[best] - linesEl.scrollTop)) best = i;
  return best;
}

/**
 * Vertical scrolling for lines taller than the overlay: one caption line per wheel notch. The
 * newest captions stay at the bottom by default; scrolling up freezes the lines being read and
 * scrolling back down to the bottom resumes the live view.
 */
window.addEventListener(
  "wheel",
  (e) => {
    const stops = lineStops();
    if (stops.length < 2) return;
    e.preventDefault();
    const delta = e.deltaMode === 1 ? e.deltaY * 40 : e.deltaY;
    if (Math.sign(delta) !== Math.sign(wheelDelta)) wheelDelta = 0;
    wheelDelta += delta;
    if (Math.abs(wheelDelta) < WHEEL_STEP) return;
    const dir = wheelDelta > 0 ? 1 : -1;
    wheelDelta = 0;
    const next = nearestStop(stops) + dir;
    if (next < 0 || next >= stops.length) return;
    linesEl.scrollTop = stops[next];
    follow = next === stops.length - 1;
    if (follow) unfreeze();
    else if (!preview && !frozen && shownIds.length) {
      // Freeze the lines being read, so new captions do not push them out.
      frozen = [...shownIds];
    }
  },
  { passive: false },
);

/**
 * The scrollbar can also move the view (drag, track click or keyboard). Keep the live/frozen
 * state in step with where it was left, so new captions do not yank the view back to the bottom.
 */
linesEl.addEventListener("scroll", () => {
  if (linesEl.scrollHeight - linesEl.scrollTop - linesEl.clientHeight > 1) {
    if (follow) {
      follow = false;
      if (!preview && !frozen && shownIds.length) frozen = [...shownIds];
    }
  } else if (!follow || frozen) unfreeze();
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
  draw();
});
// The settings window turns the preview on while its appearance page is open and translation is stopped.
listen("overlay-preview", (e) => {
  preview = !!e.payload;
  follow = true;
  frozen = null;
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

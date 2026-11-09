const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { save: saveDialog, open: openDialog } = window.__TAURI__.dialog;

let snap, page, progress = {}, benchMsg = "", hist = [], termsLang, agreed = false;
let updateInfo = null, updateChecked = false;
const busy = new Set();
const $ = (s) => document.querySelector(s);
const t = (k) => (I18N[snap?.settings.ui_lang] || I18N.en)[k] || k;
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
/** Backend errors may be codes such as E_MACOS_PERMISSION or E_MACOS_AUDIO|detail; localize known codes and keep unknown ones verbatim. */
function errText(e) {
  const m = /^E_([A-Z0-9_]+)(?:\|([\s\S]*))?$/.exec(String(e ?? ""));
  if (!m) return String(e);
  const key = "err_" + m[1].toLowerCase();
  const msg = t(key);
  if (msg === key) return String(e);
  return m[2] ? `${msg} (${m[2]})` : msg;
}
const gb = (b) => (b < 1e8 ? (b / 1e6).toFixed(1) + " MB" : (b / 1e9).toFixed(2) + " GB");
const model = (id) => snap.registry.models.find((m) => m.id === id);

const get = (o, path) => path.split(".").reduce((a, k) => a?.[k], o);
function set(o, path, v) {
  const ks = path.split(".");
  const last = ks.pop();
  ks.reduce((a, k) => (a[k] ??= {}), o)[last] = v;
}

function toast(msg, kind = "ok") {
  const el = document.createElement("div");
  el.className = `toast ${kind}`;
  el.textContent = msg;
  $("#toasts").append(el);
  setTimeout(() => el.remove(), kind === "error" ? 7000 : 1800);
}

async function refresh() {
  // A live appearance drag may not have reached settings.json yet; save it first so the snapshot
  // below does not revert the controls (for example when a hotkey starts translation mid-drag).
  await settleAppearance();
  snap = await invoke("snapshot");
  snap.settings.overlay = overlayConfig(snap.settings.overlay);
  if (!snap.settings.ui_lang) {
    snap.settings.ui_lang = detectUiLang(navigator.languages);
    // A failed save must not keep the first screen blank; the detected language still applies this session.
    try { await invoke("save_settings", { settings: snap.settings }); }
    catch (e) { toast(errText(e), "error"); }
  }
  if (snap.terms_accepted) checkUpdate();
  render();
}

/** Checks GitHub once per app session; offline or API errors stay quiet. */
async function checkUpdate() {
  if (updateChecked) return;
  updateChecked = true;
  try {
    const u = await invoke("check_update");
    if (u) { updateInfo = u; showUpdateBanner(); }
  } catch { /* offline / API error: stay quiet */ }
}

async function persist() {
  try { await invoke("save_settings", { settings: snap.settings }); toast(t("saved")); }
  catch (e) { toast(errText(e), "error"); }
  await refresh();
}

// --- small builders ---
function opts(pairs, cur) {
  return pairs.map(([v, l]) => `<option value="${esc(v)}" ${String(v) === String(cur ?? "") ? "selected" : ""}>${esc(l)}</option>`).join("");
}
const langOpts = (cur, extra = [], langs = LANGS) => opts([...extra, ...Object.entries(langs)], cur);
function field(label, path, kind = "text", extra = "") {
  const v = get(snap.settings, path);
  if (kind === "check") return `<label class="chk"><input type="checkbox" data-k="${path}" data-t="check" ${v ? "checked" : ""}> ${esc(label)}</label>`;
  return `<label class="f"><span>${esc(label)}</span><input type="${kind}" data-k="${path}" data-t="${kind}" value="${esc(v)}" ${extra}></label>`;
}
function select(label, path, options, type = "str") {
  return `<label class="f"><span>${esc(label)}</span><select data-k="${path}" data-t="${type}">${options}</select></label>`;
}
/** A slider whose value is shown next to it. `fmt` picks the readout: "pct" (0-1 -> %), "px", or plain. */
function rangeField(label, path, min, max, step, fmt = "") {
  const v = get(snap.settings, path);
  const id = `ov-${path.replace(/\W/g, "-")}`;
  const shown = esc(fmtValue(fmt, v));
  return `<label class="f"><span>${esc(label)}</span><span class="ctl"><input id="${id}" type="range" data-k="${path}" data-t="range" data-live="1" data-fmt="${fmt}" min="${min}" max="${max}" step="${step}" value="${esc(v)}" aria-valuetext="${shown}"><output for="${id}" aria-hidden="true">${shown}</output></span></label>`;
}
/** A color picker with the hex value next to it. */
function colorField(label, path) {
  const v = get(snap.settings, path);
  const id = `ov-${path.replace(/\W/g, "-")}`;
  return `<label class="f"><span>${esc(label)}</span><span class="ctl"><input id="${id}" type="color" data-k="${path}" data-t="color" data-live="1" value="${esc(v)}"><code>${esc(v)}</code></span></label>`;
}
function fmtValue(fmt, v) {
  if (fmt === "pct") return `${Math.round(v * 100)}%`;
  if (fmt === "px") return `${v}px`;
  return String(v);
}
/** Every action button goes through here so it shows a spinner and is disabled while its action runs. */
function btn(act, label, { cls = "", disabled = false, id = "" } = {}) {
  const key = id ? `${act}:${id}` : act;
  const b = busy.has(key);
  return `<button data-act="${act}" ${id ? `data-id="${esc(id)}"` : ""} class="${cls}${b ? " busy" : ""}" ${disabled || b ? "disabled" : ""}>${esc(label)}</button>`;
}
const card = (title, body, extra = "") => `<div class="card" ${extra}>${title ? `<h3>${esc(title)}</h3>` : ""}${body}</div>`;

// --- setup state ---
function setupState() {
  const sel = snap.settings.selection;
  const missing = sel ? [sel.vad, sel.asr, sel.translator].filter((id) => !snap.downloaded.includes(id)) : [];
  return { sel, missing, ready: !!sel && missing.length === 0 };
}

// --- pages ---
function termsView() {
  return `<div class="terms">
    <h1>OverlaySubs <span class="badge">Built with PLaMo</span></h1>
    <h2>${t("terms_title")}</h2>
    <p>${t("terms_intro")}</p>
    <p class="muted">${t("about_plamo_license")}</p>
    <div class="seg">${btn("terms-en", "English", { cls: termsLang === "en" ? "on" : "" })}${btn("terms-ja", "日本語", { cls: termsLang === "ja" ? "on" : "" })}</div>
    <pre class="license" id="terms"></pre>
    <label class="chk"><input type="checkbox" id="agree" ${agreed ? "checked" : ""}> ${t("terms_agree")}</label>
    ${btn("accept", t("accept"), { cls: "primary big", disabled: !agreed })}
  </div>`;
}

/** In-flight progress markup. Download events patch it in place instead of re-rendering. */
const progressView = (p) =>
  `<progress data-prog="${esc(p.id)}" max="${p.total}" value="${p.done}"></progress><span class="muted" data-pct="${esc(p.id)}">${Math.floor((100 * p.done) / p.total)}%</span>`;

function modelRow(kind, key) {
  const s = snap.settings.selection;
  const list = snap.registry.models.filter((m) => m.kind === kind);
  const cur = model(s[key]);
  const have = snap.downloaded.includes(cur.id);
  const p = progress[cur.id];
  const status = have
    ? `<span class="ok">✓ ${t("downloaded")}</span> ${btn("delete", t("delete"), { cls: "small", id: cur.id })}`
    : p
      ? progressView(p)
      : `<span class="muted">${t("missing")} · ${gb(cur.size)}</span>`;
  return `<div class="mrow">
    ${select(t(kind), `selection.${key}`, opts(list.map((m) => [m.id, `${m.label} · ${gb(m.size)}${snap.downloaded.includes(m.id) ? " ✓" : ""}`]), s[key]))}
    <div class="mstatus" data-mstatus="${esc(cur.id)}">${status}</div></div>`;
}

function modelsPage() {
  const { sel, missing, ready } = setupState();
  const s = snap.settings;
  const total = missing.reduce((a, id) => a + model(id).size, 0);
  const cached = snap.hardware;
  const hw = cached?.hardware;
  const checked = cached?.detected_at > 0 ? `${t("last_checked")} ${new Date(cached.detected_at * 1000).toLocaleDateString(s.ui_lang || "en")} · ` : "";
  const step = (n, done, locked, title, body) =>
    `<div class="step ${done ? "done" : ""} ${locked ? "locked" : ""}"><div class="num">${done ? "✓" : n}</div><div class="body"><h3>${esc(title)}</h3>${body}</div></div>`;
  return `<h2>${t("models")}</h2><p class="muted">${t("models_intro")}</p>
  ${step(1, !!sel, false, t("step_detect"), `<p class="muted">${t("step_detect_desc")}</p>
    <div class="row">${btn("detect", sel ? t("redetect") : t("detect"), { cls: sel ? "" : "primary" })}
    ${s.tier ? `<span class="badge">${t("tier")}: ${esc(s.tier)}</span>` : ""}</div>
    ${hw ? `<p class="muted small">${esc(checked + hw.cpu_name)} · ${hw.cpu_threads} threads · RAM ${(hw.ram_mb / 1024).toFixed(0)} GB · GPU ${esc(hw.gpu ? `${hw.gpu.name} (${(hw.gpu.vram_mb / 1024).toFixed(1)} GB${hw.gpu.unified ? ", unified" : ""})` : "none")}</p>` : ""}`)}
  ${step(2, ready, !sel, t("step_download"), sel
    ? `<p class="muted">${t("step_download_desc")}</p>
      ${modelRow("asr", "asr")}${modelRow("translator", "translator")}${modelRow("vad", "vad")}
      <div class="row">${btn("download", busy.has("download") ? t("downloading") : `${t("download")} (${gb(total)})`, { cls: ready ? "" : "primary", disabled: ready })}</div>`
    : `<p class="muted">${t("step_locked")}</p>`)}
  ${step(3, !!benchMsg, !ready, t("step_bench"), `<p class="muted">${t("step_bench_desc")}</p>
    <div class="row">${btn("bench", busy.has("bench") ? t("benchmarking") : t("benchmark"), { disabled: !ready })}${benchMsg ? `<span>${esc(benchMsg)}</span>` : ""}</div>`)}
  <details class="card"><summary>${t("advanced")}</summary>
    ${field(t("gpu_layers"), "gpu_layers", "number", 'min="0" max="999"')}
    ${field(t("whisper_gpu"), "whisper_gpu", "check")}
    <div class="f"><span>${t("models_dir")}</span><div class="row"><code>${esc(snap.models_dir)}</code>${btn("dir", t("change"), { cls: "small" })}</div></div>
  </details>`;
}

function homePage() {
  const s = snap.settings;
  const { ready } = setupState();
  // macOS captures system audio from every app through ScreenCaptureKit, so there is nothing to pick.
  const macSystem = s.source.kind === "system" && snap.platform === "macos";
  const devs = s.source.kind === "system" ? snap.devices.outputs : snap.devices.inputs;
  const glossary = (s.session.glossary || []).map(([a, b]) => `${a}=${b}`).join("\n");
  return `<h2>${t("home")}</h2>
  ${ready ? "" : `<div class="banner"><span>${t("need_setup_long")}</span>${btn("goto-models", t("goto_models"), { cls: "primary" })}</div>`}
  <div class="grid2">
    ${card(t("input"), `${select(t("source"), "source.kind", opts([["system", t("system")], ["mic", t("mic")]], s.source.kind))}
      ${macSystem ? `<p class="muted small">${t("system_audio_note")}</p>` : select(t("device"), "source.device", opts([["", t("default_dev")], ...devs.map((d) => [d, d])], s.source.device), "nullable")}
      ${snap.running ? `<p class="muted small">${t("input_restart_note")}</p>` : ""}`)}
    ${card(t("languages"), `${select(t("src_lang"), "session.source_lang", langOpts(s.session.source_lang, [["", t("auto")]]), "nullable")}
      ${select(t("tgt_lang"), "session.target_lang", langOpts(s.session.target_lang, [], TARGET_LANGS))}
      ${select(t("alt_lang"), "session.alt_target_lang", langOpts(s.session.alt_target_lang, [], TARGET_LANGS))}`)}
  </div>
  <details class="card"><summary>${t("tuning")}</summary>
    ${select(t("form"), "session.form", opts([["", t("form_default")], ["plain", t("plain")], ["polite", t("polite")]], s.session.form), "nullable")}
    ${field(t("context"), "session.use_context", "check")}
    ${field(t("topic"), "session.topic", "text", `placeholder="${esc(t("topic_ph"))}"`)}
    <label class="f top"><span>${t("glossary")}</span><textarea data-k="session.glossary" data-t="glossary" placeholder="source=target">${esc(glossary)}</textarea></label>
  </details>
  ${card(t("recent"), `<div class="hist" data-hist="6"></div>`)}`;
}

function overlayPage() {
  const s = snap.settings;
  return `<h2>${t("overlay")}</h2>
  <p class="muted small">${t("overlay_preview_note")}</p>
  <div class="row">${btn("overlay-reset", t("reset_position"))}${btn("overlay-reset-appearance", t("reset_appearance"))}</div>
  <div class="grid2">
    ${card(t("group_overlay"), `${colorField(t("overlay_bg_color"), "overlay.bgColor")}
      ${rangeField(t("overlay_bg_opacity"), "overlay.bgOpacity", 0, 1, 0.01, "pct")}`)}
    ${card(t("group_caption"), `${colorField(t("cap_bg_color"), "overlay.capBgColor")}
      ${rangeField(t("cap_bg_opacity"), "overlay.capOpacity", 0, 1, 0.01, "pct")}`)}
    ${card(t("group_text"), `${rangeField(t("font_size"), "overlay.fontSize", 12, 96, 1, "px")}
      ${colorField(t("text_color"), "overlay.color")}
      ${field(t("font"), "overlay.font")}
      ${select(t("font_weight"), "overlay.fontWeight", opts([[400, t("weight_normal")], [500, t("weight_medium")], [600, t("weight_semibold")], [700, t("weight_bold")]], s.overlay.fontWeight), "number")}
      ${rangeField(t("line_height"), "overlay.lineHeight", 0.8, 2.2, 0.05)}`)}
    ${card(t("group_source"), `${field(t("show_source"), "overlay.showSource", "check")}
      ${rangeField(t("src_size"), "overlay.srcSize", 8, 48, 1, "px")}
      ${colorField(t("src_color"), "overlay.srcColor")}`)}
  </div>
  ${card(t("group_layout"), `${rangeField(t("lines"), "overlay.lines", 1, 50, 1)}
    ${select(t("align"), "overlay.align", opts([["center", t("align_center")], ["left", t("align_left")], ["right", t("align_right")]], s.overlay.align))}
    ${select(t("valign"), "overlay.vAlign", opts([["bottom", t("valign_bottom")], ["center", t("valign_center")], ["top", t("valign_top")]], s.overlay.vAlign))}
    ${field(t("linger"), "overlay.linger", "number", 'min="1" max="60"')}
    ${field(t("click_through"), "overlay.clickThrough", "check")}`)}
  ${card("", field(t("hotkey"), "hotkey"))}`;
}

/** The sidebar switch mirrors the overlay window; patch it in place instead of a full render. */
function drawOverlayToggle() {
  const el = $("#overlay-toggle");
  if (!el || !snap) return;
  const on = !!snap.overlay_visible;
  el.classList.toggle("on", on);
  el.setAttribute("aria-checked", String(on));
  el.title = on ? t("hide_overlay") : t("show_overlay");
}

function historyPage() {
  return `<h2>${t("history")}</h2>
  <div class="row">${btn("export-txt", t("export_txt"))}${btn("export-srt", t("export_srt"))}</div>
  <p class="muted small">${t("output_note")}</p>
  ${card("", `<div class="hist" data-hist="200"></div>`)}`;
}

function aboutPage() {
  return `<h2>${t("about")}</h2>
  ${card("", `<p>OverlaySubs · MIT License. Translation: <b>Built with PLaMo</b> (PLaMo 3 Translate, Preferred Networks). Speech: whisper.cpp. VAD: Silero.</p>
    <p class="muted">${t("about_plamo_license")}</p>`)}
  ${card(t("terms_title"), `<div class="seg">${btn("terms-en", "English", { cls: termsLang === "en" ? "on" : "" })}${btn("terms-ja", "日本語", { cls: termsLang === "ja" ? "on" : "" })}</div>
    <pre class="license" id="terms"></pre>`)}`;
}

function updateBanner() {
  if (!updateInfo) return "";
  const msg = t("update_available").replace("{latest}", esc(updateInfo.latest)).replace("{current}", esc(updateInfo.current));
  return `<div class="update-banner" role="status" aria-live="polite">
    <div class="msg"><b>${esc(t("update_title"))}</b> ${msg}</div>
    <div class="row">${btn("open-release", t("update_open"), { cls: "primary small" })}${btn("dismiss-update", t("update_later"), { cls: "small" })}</div>
  </div>`;
}

/** Insert the banner in place: the check can resolve seconds after startup, so a full render would steal focus. */
function showUpdateBanner() {
  const host = $("#page");
  if (host && !$(".update-banner")) host.insertAdjacentHTML("afterbegin", updateBanner());
}

const PAGES = { home: homePage, models: modelsPage, overlay: overlayPage, history: historyPage, about: aboutPage };

function render() {
  const s = snap.settings;
  document.documentElement.lang = s.ui_lang || "en";
  // The terms exist only in Japanese and English.
  termsLang ??= s.ui_lang === "ja" ? "ja" : "en";
  if (!snap.terms_accepted) {
    $("#app").className = "solo";
    $("#app").innerHTML = termsView();
    return loadTerms();
  }
  const { ready } = setupState();
  page ??= ready ? "home" : "models";
  const prev = $("#page");
  const keep = prev?.dataset.page === page ? prev.scrollTop : 0;
  $("#app").className = "";
  $("#app").innerHTML = `
  <aside>
    <div class="brand">OverlaySubs</div>
    <nav>${Object.keys(PAGES).map((p) => `<button data-act="nav" data-id="${p}" class="${p === page ? "on" : ""}">${t("nav_" + p)}${p === "models" && !ready ? ' <span class="dot warn"></span>' : ""}</button>`).join("")}</nav>
    <div class="spacer"></div>
    <button id="overlay-toggle" class="ovtoggle${snap.overlay_visible ? " on" : ""}" role="switch" aria-checked="${snap.overlay_visible ? "true" : "false"}" title="${esc(snap.overlay_visible ? t("hide_overlay") : t("show_overlay"))}"><span class="lbl">${t("overlay_toggle")}</span><span class="track"><span class="knob"></span></span></button>
    ${btn("toggle", snap.running ? t("stop") : t("start"), { cls: `start ${snap.running ? "running" : "primary"}`, disabled: !ready })}
    <p class="hint">${snap.running ? `<span class="dot live"></span>${t("live")}` : ready ? `${t("hotkey")}: <code>${esc(s.hotkey)}</code>` : t("need_setup")}</p>
    <select data-k="ui_lang" data-t="str" aria-label="${t("ui_lang")}">${opts([["en", "English"], ["ja", "日本語"], ["zh-hant", "繁體中文"], ["zh-hans", "简体中文"]], s.ui_lang)}</select>
  </aside>
  <section id="page" data-page="${page}">${updateBanner()}${PAGES[page]()}</section>`;
  $("#page").scrollTop = keep;
  drawHistory();
  if (page === "about") loadTerms();
  syncPreview();
}

/**
 * Keep a sample subtitle on the overlay while this page is open and translation is stopped,
 * so font size, font, background opacity and the other appearance settings can be checked live.
 * Real subtitles win: the overlay ignores the preview once a caption arrives.
 */
let previewOn = false;
function syncPreview() {
  const on = !!snap && snap.terms_accepted && page === "overlay" && !snap.running;
  if (on === previewOn) return;
  previewOn = on;
  invoke("set_overlay_preview", { on }).catch(() => {});
}

async function loadTerms() {
  const el = $("#terms");
  if (el) el.textContent = await invoke("terms_text", { lang: termsLang });
}

function drawHistory() {
  for (const el of document.querySelectorAll("[data-hist]")) {
    const rows = hist.slice(-Number(el.dataset.hist)).reverse();
    el.innerHTML = rows.length ? rows.map((e) => `<div><div class="src">${esc(e.source)}</div>${esc(e.translation)}</div>`).join("") : `<p class="muted">${t("no_history")}</p>`;
  }
}
async function loadHistory() {
  hist = await invoke("history");
  drawHistory();
}

// --- events ---
/**
 * Appearance sliders and color pickers emit an `input` event for every step of a drag. A full
 * refresh there would drop the control and a toast for every step would bury the screen, so they
 * are saved quietly at most once per interval; the overlay applies them live through the
 * `settings` event save_settings emits. The final value is flushed on `change`.
 */
const APPEARANCE_SAVE_MS = 80;
let appearanceTimer = null;
let appearanceSavedAt = 0;
let appearanceErrorShown = false;
// Saves are chained so two invokes cannot land out of order and leave an older snapshot on disk.
let appearanceSave = Promise.resolve();
function saveAppearance() {
  if (!snap) return;
  appearanceSavedAt = Date.now();
  appearanceSave = appearanceSave
    .catch(() => {})
    .then(() => invoke("save_settings", { settings: snap.settings }))
    .then(
      () => { appearanceErrorShown = false; },
      (e) => {
        // One toast per failure streak: a drag would otherwise bury the screen in errors.
        if (appearanceErrorShown) return;
        appearanceErrorShown = true;
        toast(errText(e), "error");
      },
    );
}
function saveAppearanceSoon() {
  if (appearanceTimer) return;
  appearanceTimer = setTimeout(() => { appearanceTimer = null; saveAppearance(); }, Math.max(0, APPEARANCE_SAVE_MS - (Date.now() - appearanceSavedAt)));
}
function flushAppearance() {
  if (appearanceTimer) { clearTimeout(appearanceTimer); appearanceTimer = null; }
  if (snap) saveAppearance();
}
/** Saves and waits for a pending appearance change, so a snapshot cannot overwrite a drag in progress. */
async function settleAppearance() {
  if (appearanceTimer) { clearTimeout(appearanceTimer); appearanceTimer = null; saveAppearance(); }
  await appearanceSave;
}
/** Applies one appearance control to the in-memory settings and updates its readout. */
function applyAppearance(el) {
  if (!snap) return;
  const v = el.dataset.t === "range" ? Number(el.value) : el.value;
  set(snap.settings, el.dataset.k, v);
  const shown = fmtValue(el.dataset.fmt, v);
  const out = el.closest(".ctl")?.querySelector("output, code");
  if (out) out.textContent = shown;
  if (el.type === "range") el.setAttribute("aria-valuetext", shown);
  saveAppearanceSoon();
}
document.addEventListener("input", (ev) => {
  const el = ev.target;
  if (el.dataset.live) applyAppearance(el);
});
document.addEventListener("change", (ev) => {
  const el = ev.target;
  if (el.id === "agree") return ((agreed = el.checked), ($('[data-act="accept"]').disabled = !agreed));
  const k = el.dataset.k;
  if (!k) return;
  if (el.dataset.live) { applyAppearance(el); flushAppearance(); return; }
  const type = el.dataset.t;
  let v = el.value;
  if (type === "check") v = el.checked;
  else if (type === "number") {
    v = Number(v);
    if (el.value === "" || Number.isNaN(v)) return toast(t("invalid_number"), "error");
  } else if (type === "nullable") v = v || null;
  else if (type === "glossary") v = v.split("\n").map((l) => l.split("=").map((x) => x.trim())).filter((p) => p.length === 2 && p[0] && p[1]);
  if (k === "source.kind") snap.settings.source = { kind: v, device: null };
  else set(snap.settings, k, v);
  // Number inputs accept typed values outside their min/max; normalize before saving so the file
  // holds the same values the overlay clamps to.
  if (k.startsWith("overlay.")) snap.settings.overlay = overlayConfig(snap.settings.overlay);
  persist();
});

async function act(name, id, el) {
  if (name === "nav") { page = id; return render(); }
  if (name === "goto-models") { page = "models"; return render(); }
  if (name === "terms-ja" || name === "terms-en") { termsLang = name.slice(6); return render(); }
  if (name === "accept") {
    await invoke("accept_terms");
    return refresh();
  }
  if (name === "detect") {
    const d = await invoke("detect");
    snap.settings.tier = d.tier;
    snap.settings.selection = d.selection;
    await invoke("save_settings", { settings: snap.settings });
    toast(`${t("detected")}: ${t("tier")} ${d.tier}`);
  }
  if (name === "download") {
    const s = snap.settings.selection;
    await invoke("download", { ids: [s.vad, s.asr, s.translator].filter((id) => !snap.downloaded.includes(id)) });
    progress = {};
    toast(t("download_done"));
  }
  if (name === "overlay-reset") {
    await invoke("reset_overlay_position");
    toast(t("position_reset"));
  }
  if (name === "overlay-reset-appearance") {
    // Click-through is an interaction setting, not a look, so resetting the appearance keeps it.
    const { clickThrough } = snap.settings.overlay;
    snap.settings.overlay = { ...OVERLAY_DEFAULTS, clickThrough };
    await invoke("save_settings", { settings: snap.settings });
    toast(t("appearance_reset"));
  }
  if (name === "bench") {
    const b = await invoke("benchmark");
    benchMsg = `${t("rtf")}: ${b.rtf.toFixed(2)} ${b.rtf < 1 ? "✓" : ""}`;
    if (b.downgraded_to) {
      benchMsg += ` · ${t("downgrade")} → ${b.downgraded_to.tier}`;
      snap.settings.tier = b.downgraded_to.tier;
      snap.settings.selection = b.downgraded_to.selection;
      await invoke("save_settings", { settings: snap.settings });
    }
    toast(t("bench_done"));
  }
  if (name === "toggle") await invoke(snap.running ? "stop" : "start");
  if (name === "delete") { await invoke("delete_model", { id }); toast(t("deleted")); }
  if (name === "dir") {
    const d = await openDialog({ directory: true });
    if (d) { snap.settings.models_dir = d; await invoke("save_settings", { settings: snap.settings }); toast(t("saved")); }
  }
  if (name === "export-txt" || name === "export-srt") {
    const format = name.slice(7);
    const path = await saveDialog({ defaultPath: `overlaysubs.${format}`, filters: [{ name: format, extensions: [format] }] });
    if (path) { await invoke("export_history", { path, format, withSource: snap.settings.overlay.showSource }); toast(t("exported")); toast(t("output_note"), "warn"); }
  }
  if (name === "open-release") return invoke("open_url", { url: updateInfo.url }).catch((e) => toast(errText(e), "error"));
  if (name === "dismiss-update") { updateInfo = null; return render(); }
  await refresh();
}

document.addEventListener("click", async (ev) => {
  const toggle = ev.target.closest("#overlay-toggle");
  if (!toggle || !snap) return;
  const show = !snap.overlay_visible;
  snap.overlay_visible = show;
  drawOverlayToggle();
  try { await invoke("show_overlay", { show }); }
  catch (e) { snap.overlay_visible = !show; drawOverlayToggle(); toast(errText(e), "error"); }
});

document.addEventListener("click", async (ev) => {
  const link = ev.target.closest("a[data-external]");
  if (link) {
    ev.preventDefault();
    invoke("open_url", { url: link.href }).catch((e) => toast(errText(e), "error"));
    return;
  }
  const el = ev.target.closest("[data-act]");
  if (!el || el.disabled) return;
  const { act: name, id } = el.dataset;
  const key = id && name !== "nav" ? `${name}:${id}` : name;
  if (busy.has(key)) return;
  busy.add(key);
  if (snap) render();
  try { await act(name, id, el); }
  catch (e) { toast(errText(e), "error"); }
  finally { busy.delete(key); if (snap) render(); }
});

listen("download", (e) => {
  const p = e.payload;
  // The last chunk is emitted before the checksum and the rename, so a late event can be delivered after a
  // refresh already lists the model as downloaded. Keep that row's "Downloaded" state instead of replacing
  // it with a progress bar that would stay until the next render.
  if (snap?.downloaded.includes(p.id)) return;
  progress[p.id] = p;
  // Progress is shown only on the models page. Re-rendering other pages on every chunk replaces the nav
  // buttons between mousedown and mouseup, so clicks are lost until the download ends.
  if (page !== "models") return;
  // Patch the status cell of the row that shows this model. The cell is keyed by model id, so a selection
  // change during the download removes it and leaves nothing to update; switching back re-renders from the
  // recorded progress. Never render() here: rebuilding the page between mousedown and mouseup swallows the
  // click, closes an open <select>, and resets the <details> sections.
  const cell = document.querySelector(`[data-mstatus="${CSS.escape(p.id)}"]`);
  if (!cell) return;
  const bar = cell.querySelector(`[data-prog="${CSS.escape(p.id)}"]`);
  if (!bar) { cell.innerHTML = progressView(p); return; }
  bar.value = p.done;
  const pct = cell.querySelector(`[data-pct="${CSS.escape(p.id)}"]`);
  if (pct) pct.textContent = `${Math.floor((100 * p.done) / p.total)}%`;
});
listen("running", refresh);
listen("overlay-visible", (e) => {
  if (!snap) return;
  snap.overlay_visible = e.payload;
  drawOverlayToggle();
});
listen("error", (e) => toast(errText(e.payload), "error"));
listen("caption", (e) => { if (e.payload.stage === "final") loadHistory(); });
refresh().then(loadHistory);

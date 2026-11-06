const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { save: saveDialog, open: openDialog } = window.__TAURI__.dialog;

let snap, detected, progress = {}, error = "", benchMsg = "", licenseLang = "ja";
const $ = (s) => document.querySelector(s);
const t = (k) => (I18N[snap?.settings.ui_lang] || I18N.ja)[k] || k;
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const gb = (b) => (b / 1e9).toFixed(2) + " GB";

const get = (o, path) => path.split(".").reduce((a, k) => a?.[k], o);
function set(o, path, v) {
  const ks = path.split(".");
  const last = ks.pop();
  ks.reduce((a, k) => (a[k] ??= {}), o)[last] = v;
}

async function refresh() {
  snap = await invoke("snapshot");
  snap.settings.overlay = { ...OVERLAY_DEFAULTS, ...snap.settings.overlay };
  render();
}

async function persist() {
  try { await invoke("save_settings", { settings: snap.settings }); error = ""; }
  catch (e) { error = String(e); }
  await refresh();
}

function opts(pairs, cur) {
  return pairs.map(([v, l]) => `<option value="${esc(v)}" ${String(v) === String(cur ?? "") ? "selected" : ""}>${esc(l)}</option>`).join("");
}
const langOpts = (cur, extra = []) => opts([...extra, ...Object.entries(LANGS)], cur);

function field(label, path, kind = "text", extra = "") {
  const v = get(snap.settings, path);
  if (kind === "check") return `<label><input type="checkbox" data-k="${path}" data-t="check" ${v ? "checked" : ""}> ${esc(label)}</label>`;
  return `<label><span>${esc(label)}</span><input type="${kind}" data-k="${path}" data-t="${kind}" value="${esc(v)}" ${extra}></label>`;
}
function select(label, path, options, type = "str") {
  return `<label><span>${esc(label)}</span><select data-k="${path}" data-t="${type}">${options}</select></label>`;
}

function licenseView() {
  return `<section><h2>${t("license_title")}</h2>
    <p>${t("license_intro")}</p>
    <p><b>${t("license_corp")}</b></p>
    <p>${t("license_output")}</p>
    <div class="row"><button data-act="lic-ja">日本語</button><button data-act="lic-en">English</button></div>
    <pre class="license" id="lic"></pre>
    <label><input type="checkbox" id="agree"> ${t("license_agree")}</label>
    <button class="primary" data-act="accept">${t("accept")}</button></section>`;
}

function modelRow(kind) {
  const s = snap.settings.selection || {};
  const key = kind === "asr" ? "asr" : kind === "translator" ? "translator" : "vad";
  const list = snap.registry.models.filter((m) => m.kind === kind);
  const cur = list.find((m) => m.id === s[key]);
  const have = cur && snap.downloaded.includes(cur.id);
  const p = cur && progress[cur.id];
  return `${select(t(kind), `selection.${key}`, opts(list.map((m) => [m.id, `${m.label} · ${gb(m.size)}${snap.downloaded.includes(m.id) ? " ✓" : ""}`]), s[key]))}
    <div class="row muted">${cur ? (have ? t("downloaded") : t("missing")) : ""}
      ${have ? `<button data-act="delete" data-id="${cur.id}">${t("delete")}</button>` : ""}
      ${p && !have ? `<progress max="${p.total}" value="${p.done}"></progress>` : ""}</div>`;
}

function render() {
  const s = snap.settings, o = s.overlay;
  if (!s.license_accepted) {
    $("#app").innerHTML = `<h1>floatsubs <span class="badge">Built with PLaMo</span></h1>${licenseView()}`;
    loadLicense();
    return;
  }
  const sel = s.selection;
  const missing = sel ? [sel.asr, sel.translator, sel.vad].filter((id) => !snap.downloaded.includes(id)) : [];
  const srcKind = s.source.kind;
  const devs = srcKind === "system" ? snap.devices.outputs : snap.devices.inputs;
  const glossary = (s.session.glossary || []).map(([a, b]) => `${a}=${b}`).join("\n");
  $("#app").innerHTML = `
  <h1>floatsubs <span class="badge">Built with PLaMo</span>
    <select data-k="ui_lang" data-t="str" style="margin-left:auto">${opts([["ja", "日本語"], ["en", "English"]], s.ui_lang)}</select></h1>
  ${error ? `<p class="error">${esc(error)}</p>` : ""}
  <section class="row">
    <button class="primary big" data-act="toggle" ${!sel || missing.length ? "disabled" : ""}>${snap.running ? t("stop") : t("start")}</button>
    <span class="muted">${t("hotkey")}: <code>${esc(s.hotkey)}</code></span>
  </section>
  <section><h2>${t("setup")}</h2>
    <div class="row"><button data-act="detect">${t("detect")}</button>
      ${sel ? `<button data-act="download" ${missing.length ? "" : "disabled"}>${t("download")} (${gb(missing.reduce((a, id) => a + snap.registry.models.find((m) => m.id === id).size, 0))})</button>
      <button data-act="bench" ${missing.length ? "disabled" : ""}>${t("benchmark")}</button>` : ""}
      ${s.tier ? `<span class="badge">${t("tier")}: ${s.tier}</span>` : ""}</div>
    ${detected ? `<p class="muted">${esc(detected.hardware.cpu_name)} · ${detected.hardware.cpu_threads} threads · RAM ${(detected.hardware.ram_mb / 1024).toFixed(0)} GB · GPU ${esc(detected.hardware.gpu ? `${detected.hardware.gpu.name} (${(detected.hardware.gpu.vram_mb / 1024).toFixed(1)} GB${detected.hardware.gpu.unified ? ", unified" : ""})` : "none")}</p>` : ""}
    ${benchMsg ? `<p class="muted">${esc(benchMsg)}</p>` : ""}
  </section>
  <section><h2>${t("input")}</h2>
    ${select(t("input"), "source.kind", opts([["system", t("system")], ["mic", t("mic")]], srcKind))}
    ${select(t("device"), "source.device", opts([["", t("default_dev")], ...devs.map((d) => [d, d])], s.source.device), "nullable")}
  </section>
  <section><h2>${t("languages")}</h2>
    ${select(t("src_lang"), "session.source_lang", langOpts(s.session.source_lang, [["", t("auto")]]), "nullable")}
    ${select(t("tgt_lang"), "session.target_lang", langOpts(s.session.target_lang))}
    ${select(t("alt_lang"), "session.alt_target_lang", langOpts(s.session.alt_target_lang))}
    ${select(t("form"), "session.form", opts([["", t("form_default")], ["plain", t("plain")], ["polite", t("polite")]], s.session.form), "nullable")}
    ${field(t("context"), "session.use_context", "check")}
    <label style="display:block"><span>${t("glossary")}</span><textarea data-k="session.glossary" data-t="glossary">${esc(glossary)}</textarea></label>
  </section>
  <section><h2>${t("overlay")}</h2>
    ${field(t("show_source"), "overlay.showSource", "check")}
    ${field(t("click_through"), "overlay.clickThrough", "check")}
    ${field(t("font_size"), "overlay.fontSize", "number", 'min="12" max="96"')}
    ${field(t("font"), "overlay.font")}
    ${field(t("bg"), "overlay.bg", "number", 'min="0" max="1" step="0.05"')}
    ${field(t("lines"), "overlay.lines", "number", 'min="1" max="6"')}
    ${field(t("linger"), "overlay.linger", "number", 'min="1" max="60"')}
  </section>
  <section><h2>${t("models")}</h2>
    ${sel ? modelRow("asr") + modelRow("translator") + modelRow("vad") : `<p class="muted">${t("detect")} →</p>`}
    <div class="row muted">${t("models_dir")}: <code>${esc(snap.models_dir)}</code><button data-act="dir">${t("change")}</button></div>
  </section>
  <section><h2>${t("perf")}</h2>
    ${field(t("gpu_layers"), "gpu_layers", "number", 'min="0" max="999"')}
    ${field(t("whisper_gpu"), "whisper_gpu", "check")}
    ${field(t("hotkey"), "hotkey")}
  </section>
  <section><h2>${t("history")}</h2>
    <div class="row"><button data-act="export-txt">${t("export_txt")}</button><button data-act="export-srt">${t("export_srt")}</button></div>
    <p class="muted">${t("license_output")}</p>
    <div class="hist" id="hist"></div>
  </section>
  <section><h2>${t("about")}</h2>
    <p>floatsubs · MIT License. Translation: <b>Built with PLaMo</b> (PLaMo 3 Translate, Preferred Networks). Speech: whisper.cpp. VAD: Silero.</p>
    <p class="muted">${t("license_corp")}</p>
    <div class="row"><button data-act="lic-ja">${t("view_license")} (日本語)</button><button data-act="lic-en">${t("view_license")} (English)</button></div>
    <pre class="license" id="lic" hidden></pre>
  </section>`;
  loadHistory();
}

async function loadLicense(show) {
  const el = $("#lic");
  el.textContent = await invoke("license_text", { lang: licenseLang });
  if (show) el.hidden = false;
}

async function loadHistory() {
  const h = await invoke("history");
  const el = $("#hist");
  if (el) el.innerHTML = h.slice(-50).reverse().map((e) => `<div><div class="src">${esc(e.source)}</div>${esc(e.translation)}</div>`).join("");
}

document.addEventListener("change", (ev) => {
  const el = ev.target, k = el.dataset.k;
  if (!k) return;
  const type = el.dataset.t;
  let v = el.value;
  if (type === "check") v = el.checked;
  else if (type === "number") v = Number(v);
  else if (type === "nullable") v = v || null;
  else if (type === "glossary") v = v.split("\n").map((l) => l.split("=").map((x) => x.trim())).filter((p) => p.length === 2 && p[0] && p[1]);
  if (k === "source.kind") snap.settings.source = { kind: v, device: null };
  else set(snap.settings, k, v);
  persist();
});

document.addEventListener("click", async (ev) => {
  const act = ev.target.dataset?.act;
  if (!act) return;
  try {
    error = "";
    if (act === "lic-ja" || act === "lic-en") { licenseLang = act.slice(4); return loadLicense(true); }
    if (act === "accept") {
      if (!$("#agree").checked) return;
      snap.settings.license_accepted = true;
      return persist();
    }
    if (act === "detect") {
      detected = await invoke("detect");
      snap.settings.tier = detected.tier;
      snap.settings.selection = detected.selection;
      return persist();
    }
    if (act === "download") {
      const s = snap.settings.selection;
      render();
      await invoke("download", { ids: [s.vad, s.asr, s.translator].filter((id) => !snap.downloaded.includes(id)) });
    }
    if (act === "bench") {
      benchMsg = "…"; render();
      const b = await invoke("benchmark");
      benchMsg = `${t("rtf")}: ${b.rtf.toFixed(2)}`;
      if (b.downgraded_to) {
        benchMsg += ` · ${t("downgrade")} → ${b.downgraded_to.tier}`;
        snap.settings.tier = b.downgraded_to.tier;
        snap.settings.selection = b.downgraded_to.selection;
        return persist();
      }
    }
    if (act === "toggle") await invoke(snap.running ? "stop" : "start");
    if (act === "delete") await invoke("delete_model", { id: ev.target.dataset.id });
    if (act === "dir") {
      const d = await openDialog({ directory: true });
      if (d) { snap.settings.models_dir = d; return persist(); }
    }
    if (act === "export-txt" || act === "export-srt") {
      const format = act.slice(7);
      const path = await saveDialog({ defaultPath: `floatsubs.${format}`, filters: [{ name: format, extensions: [format] }] });
      if (path) await invoke("export_history", { path, format, withSource: snap.settings.overlay.showSource });
    }
  } catch (e) { error = String(e); }
  refresh();
});

listen("download", (e) => {
  progress[e.payload.id] = e.payload;
  render();
});
listen("running", refresh);
listen("error", (e) => { error = String(e.payload); render(); });
listen("caption", (e) => { if (e.payload.stage === "final") loadHistory(); });
refresh();

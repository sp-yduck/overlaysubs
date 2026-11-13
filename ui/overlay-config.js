// Overlay appearance settings, shared by the settings window (app.js) and the overlay (overlay.js).
// Rust keeps `Settings.overlay` opaque, so the shape of this object belongs to the UI. The defaults
// here are one source of truth: saved settings are merged over them, which also fills in options
// added by a later version.

/** The appearance the overlay ships with: the same look as versions before these options existed. */
const OVERLAY_DEFAULTS = {
  // The faint tint that shows where the subtitle area is, so the overlay can be found without
  // hovering over it. Set the opacity to 0 for a completely invisible overlay background.
  bgColor: "#808080",
  bgOpacity: 0.12,
  // Caption box: the rounded plate behind each subtitle line.
  capBgColor: "#000000",
  capOpacity: 0.55,
  // Translated text.
  fontSize: 28,
  color: "#ffffff",
  font: "system-ui, sans-serif",
  fontWeight: 400,
  lineHeight: 1.3,
  // Original text, shown above the translation.
  showSource: true,
  srcSize: 17,
  srcColor: "#cfd6e2",
  // Layout and behavior.
  lines: 10,
  linger: 10,
  align: "center",
  vAlign: "bottom",
  clickThrough: false,
};

/** Ranges enforced on load, so a hand-edited settings.json cannot push the overlay out of shape.
 *  They match the control ranges on the Subtitles page, so the readout and the slider agree. */
const OVERLAY_LIMITS = {
  bgOpacity: [0, 1],
  capOpacity: [0, 1],
  fontSize: [12, 96],
  lineHeight: [1, 2.2],
  srcSize: [8, 48],
  lines: [1, 50],
  linger: [1, 60],
};
const OVERLAY_WEIGHTS = [400, 500, 600, 700];
const OVERLAY_ALIGNS = ["left", "center", "right"];
const OVERLAY_VALIGNS = ["top", "center", "bottom"];
const OVERLAY_COLORS = ["bgColor", "capBgColor", "color", "srcColor"];

/** A stored number, a numeric string, or NaN. `Number(null)` and `Number("")` would wrongly be 0. */
const numeric = (v) => (typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN);
const clamp = (v, [min, max], fallback) => (Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback);
const bool = (v, fallback) => (typeof v === "boolean" ? v : fallback);

/**
 * Merges saved settings over the defaults and normalizes them. Old versions stored the caption
 * background opacity as `bg`, and sized the original text at 0.6em of the translation; both are
 * carried over so an upgraded app looks the same.
 */
function overlayConfig(saved) {
  const cfg = { ...OVERLAY_DEFAULTS, ...(saved || {}) };
  if (saved && saved.bg != null && saved.capOpacity == null) cfg.capOpacity = numeric(saved.bg);
  // Options that have since been removed (or renamed) do not linger in settings.json.
  for (const key of ["bg", "capRadius", "capMaxWidth", "outline", "outlineColor", "partialOpacity"]) delete cfg[key];
  for (const [key, range] of Object.entries(OVERLAY_LIMITS)) cfg[key] = clamp(numeric(cfg[key]), range, OVERLAY_DEFAULTS[key]);
  // Before srcSize existed the original text was 0.6em of the translation size, so keep that
  // proportion when an older settings file did not save one.
  if (saved && saved.srcSize == null) cfg.srcSize = clamp(Math.round(cfg.fontSize * 0.6), OVERLAY_LIMITS.srcSize, OVERLAY_DEFAULTS.srcSize);
  for (const key of OVERLAY_COLORS) if (!/^#[0-9a-f]{6}$/i.test(cfg[key])) cfg[key] = OVERLAY_DEFAULTS[key];
  cfg.fontWeight = numeric(cfg.fontWeight);
  if (!OVERLAY_WEIGHTS.includes(cfg.fontWeight)) cfg.fontWeight = OVERLAY_DEFAULTS.fontWeight;
  if (!OVERLAY_ALIGNS.includes(cfg.align)) cfg.align = OVERLAY_DEFAULTS.align;
  if (!OVERLAY_VALIGNS.includes(cfg.vAlign)) cfg.vAlign = OVERLAY_DEFAULTS.vAlign;
  cfg.showSource = bool(cfg.showSource, OVERLAY_DEFAULTS.showSource);
  cfg.clickThrough = bool(cfg.clickThrough, OVERLAY_DEFAULTS.clickThrough);
  return cfg;
}

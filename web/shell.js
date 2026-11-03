// Fallback text is checked against browser.ftl by tools/web/test_contracts.py.
const messages = {
  source: "Source",
  notices: "Third-party notices",
  dismiss: "Dismiss save notice",
  saves: "Saved games stay in this browser. Clearing site data removes them.",
  viewportTitle: "This window is too small for Megawatt Mogul",
  viewportDimensions: "Window: { $width } × { $height } CSS pixels. Screen: { $screenWidth } × { $screenHeight } CSS pixels.",
  viewportMinimum: "Minimum game area: { $minWidth } × { $minHeight } CSS pixels. Required window with the current notice: { $minWidth } × { $windowHeight } CSS pixels.",
  viewportHelp: "Enlarge your window, use full screen, or switch to a larger display. The game will appear when there is enough room. An active game is paused when the window is too small.",
  loading: "Loading Megawatt Mogul…",
  reload: "Reload",
  failed: "A new version may be available. Reload to retry.",
};
document.getElementById("status").textContent = messages.loading;
const reload = document.getElementById("reload");
reload.textContent = messages.reload;
reload.onclick = () => { const url = new URL(location.href); url.searchParams.set("reload", Date.now()); location.replace(url); };
function failed() {
  document.getElementById("status").textContent = messages.failed;
  document.getElementById("status").hidden = false;
  reload.hidden = false;
}
window.addEventListener("error", event => { if (mmOwnFailure(event)) failed(); }, true);
window.addEventListener("unhandledrejection", failed);
new MutationObserver(() => { if (document.getElementById("status").dataset.failed) failed(); })
  .observe(document.getElementById("status"), {attributes:true,attributeFilter:["data-failed"]});

document.getElementById("source").textContent = messages.source;
document.getElementById("notices").textContent = messages.notices;
const notice = document.getElementById("save-notice");
const dismiss = document.getElementById("dismiss-notice");
const dismissalKey = "megawatt-mogul.save-notice-dismissed";
document.getElementById("preview").textContent = messages.saves;
dismiss.textContent = messages.dismiss;
try { notice.hidden = localStorage.getItem(dismissalKey) === "true"; } catch { /* Storage may be blocked. */ }
dismiss.onclick = () => {
  notice.hidden = true;
  try { localStorage.setItem(dismissalKey, "true"); } catch { /* Dismiss for this page anyway. */ }
  sizeHint();
  document.getElementById("game").focus();
};

// Keep these content-space limits aligned with game::window::MIN_WINDOW_SIZE.
const minWidth = 1280, minHeight = 640;
const surface = document.getElementById("surface");
function format(message, values) {
  return message.replace(/\{ \$(\w+) \}/g, (_, key) => values[key]);
}
function sizeHint() {
  const supported = surface.clientWidth >= minWidth && surface.clientHeight >= minHeight;
  const values = {width:innerWidth, height:innerHeight, screenWidth:screen.width, screenHeight:screen.height,
    minWidth, minHeight, windowHeight:minHeight + Math.ceil(notice.getBoundingClientRect().height)};
  document.getElementById("viewport-title").textContent = messages.viewportTitle;
  document.getElementById("viewport-dimensions").textContent = format(messages.viewportDimensions, values);
  document.getElementById("viewport-minimum").textContent = format(messages.viewportMinimum, values);
  document.getElementById("viewport-help").textContent = messages.viewportHelp;
  document.getElementById("viewport-warning").hidden = supported;
  document.body.dataset.viewportSupported = String(supported);
  window.dispatchEvent(new Event("mm-viewport-change"));
}
window.addEventListener("resize", sizeHint);
new ResizeObserver(sizeHint).observe(surface);
sizeHint();

if (window.mmBootFailed) failed();

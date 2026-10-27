// Fallback text is checked against browser.ftl by tools/web/test_contracts.py.
const messages = {
  source: "Source",
  notices: "Third-party notices",
  small: "Use a larger window for the full game interface.",
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
window.addEventListener("error", failed, true);
window.addEventListener("unhandledrejection", failed);
new MutationObserver(() => { if (document.getElementById("status").dataset.failed) failed(); })
  .observe(document.getElementById("status"), {attributes:true,attributeFilter:["data-failed"]});

document.getElementById("source").textContent = messages.source;
document.getElementById("notices").textContent = messages.notices;
function sizeHint() { document.getElementById("small").textContent = innerWidth < 1280 || innerHeight < 670 ? messages.small : ""; }
window.addEventListener("resize",sizeHint); sizeHint();

if (window.mmBootFailed) failed();

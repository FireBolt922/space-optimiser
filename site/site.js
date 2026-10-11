// Two small enhancements; the page works without them.
// 1. Follow the visitor's light or dark preference (dark is the default).
// 2. Point the main button at the installer for the visitor's system and
//    mark that row. Phones and unknown systems keep "Choose your download".
(function () {
  if (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches) {
    document.documentElement.setAttribute("data-theme", "light");
  }

  function detectSystem() {
    var ua = navigator.userAgent || "";
    if (/Android|iPhone|iPad|iPod/i.test(ua)) return null;
    var platform =
      (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || ua;
    if (/Mac/i.test(platform)) return "macos";
    if (/Win/i.test(platform)) return "windows";
    if (/Linux/i.test(platform)) return "ubuntu";
    return null;
  }

  document.addEventListener("DOMContentLoaded", function () {
    var system = detectSystem();
    if (!system) return;
    var button = document.getElementById("primary-download");
    var href = button && button.getAttribute("data-href-" + system);
    if (href) {
      button.setAttribute("href", href);
      button.textContent = button.getAttribute("data-label-" + system);
    }
    var row = document.querySelector('.system[data-os="' + system + '"]');
    if (row) {
      row.classList.add("is-yours");
      var tag = row.querySelector(".yours");
      if (tag) tag.hidden = false;
    }
  });
})();

// Applies the saved theme before the first paint (no flash of the wrong theme).
// Plain script (not a module) so it runs synchronously in <head>; kept tiny on purpose.
(function () {
  var mode = "system";
  try {
    mode = localStorage.getItem("farba.theme") || "system";
  } catch (e) {}
  var dark =
    mode === "dark" ||
    (mode === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
})();

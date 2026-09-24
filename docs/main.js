// The two things this page does after it has been read: remember which
// theme you chose, and say which section you are in.

(function () {
  var root = document.documentElement;
  var glyph = document.getElementById('theme-glyph');
  var button = document.getElementById('theme-switch');

  function show(theme) {
    if (glyph) glyph.textContent = theme === 'dark' ? '☀' : '☾';
  }

  show(root.getAttribute('data-theme'));

  if (button) {
    button.addEventListener('click', function () {
      var next = root.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-theme', next);
      show(next);
      try { localStorage.setItem('obelus-theme', next); } catch (e) {}
    });
  }

  // Follow the system where the visitor has not chosen for themselves.
  try {
    matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function (event) {
      if (localStorage.getItem('obelus-theme')) return;
      var theme = event.matches ? 'dark' : 'light';
      root.setAttribute('data-theme', theme);
      show(theme);
    });
  } catch (e) {}

  // Which section is being read. Bottom-most heading above the fold wins,
  // which is what a reader scrolling down expects the mark to follow.
  var links = [].slice.call(document.querySelectorAll('.contents a[href^="#"]'));
  var sections = links
    .map(function (link) { return document.getElementById(link.hash.slice(1)); })
    .filter(Boolean);

  if (!sections.length) return;

  var here = null;
  function mark() {
    var line = window.scrollY + 120;
    var found = sections[0];
    for (var i = 0; i < sections.length; i += 1) {
      if (sections[i].offsetTop <= line) found = sections[i];
    }
    if (found === here) return;
    here = found;
    links.forEach(function (link) {
      link.classList.toggle('here', link.hash.slice(1) === found.id);
    });
  }

  var waiting = false;
  window.addEventListener('scroll', function () {
    if (waiting) return;
    waiting = true;
    requestAnimationFrame(function () { waiting = false; mark(); });
  }, { passive: true });
  window.addEventListener('resize', mark, { passive: true });
  mark();
})();

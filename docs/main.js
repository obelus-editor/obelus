// The three things this page does after it has been read: remember which
// theme you chose, say which section you are in, and hand over a command.

(function () {
  var root = document.documentElement;
  var button = document.getElementById('theme-switch');

  // The button says what pressing it gives, not what is showing: what is
  // showing is the page, and a key is named by what it does. In the page's
  // own language, which is why the words are on the button and not here.
  function show(theme) {
    if (!button) return;
    var next = theme === 'dark' ? 'light' : 'dark';
    button.textContent = button.getAttribute('data-' + next);
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

  // An install line is copied far more often than it is read, and half of
  // these blocks are one command with nothing else in them for that reason.
  // No button where there is nothing to write with: one that did nothing
  // would be worse than none at all.
  if (navigator.clipboard) {
    [].slice.call(document.querySelectorAll('pre')).forEach(function (pre) {
      var block = document.createElement('div');
      block.className = 'block';
      pre.parentNode.insertBefore(block, pre);
      block.appendChild(pre);

      var copy = document.createElement('button');
      copy.type = 'button';
      copy.className = 'switch copy';
      copy.textContent = document.body.getAttribute('data-copy');
      block.appendChild(copy);

      // It says it again when it has: a click with nothing on screen to
      // show for it is a button a visitor presses twice.
      var back = null;
      copy.addEventListener('click', function () {
        navigator.clipboard.writeText(pre.textContent).then(function () {
          copy.textContent = document.body.getAttribute('data-copied');
          copy.classList.add('done');
          clearTimeout(back);
          back = setTimeout(function () {
            copy.textContent = document.body.getAttribute('data-copy');
            copy.classList.remove('done');
          }, 1600);
        });
      });
    });
  }

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

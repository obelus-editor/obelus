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

  // A theme swap must not tween. Every colour on the page would move
  // independently over the transition and the page would visibly smear, so
  // transitions are off for the swap and back on the frame after it.
  function swap(theme) {
    root.classList.add('swapping');
    root.setAttribute('data-theme', theme);
    show(theme);
    requestAnimationFrame(function () {
      requestAnimationFrame(function () { root.classList.remove('swapping'); });
    });
  }

  show(root.getAttribute('data-theme'));

  if (button) {
    button.addEventListener('click', function () {
      var next = root.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
      swap(next);
      try { localStorage.setItem('obelus-theme', next); } catch (e) {}
    });
  }

  // Follow the system where the visitor has not chosen for themselves.
  try {
    matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function (event) {
      if (localStorage.getItem('obelus-theme')) return;
      swap(event.matches ? 'dark' : 'light');
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

  // The main screens, one after another. A visitor who picks one has
  // chosen what to look at, so the clock stops for good; a pointer resting
  // on them only holds it. And no clock at all for somebody who has asked
  // their machine for less motion.
  var screens = document.querySelector('.screens');
  if (screens) {
    var tabs = [].slice.call(screens.querySelectorAll('[role="tab"]'));
    var frames = [].slice.call(screens.querySelectorAll('.frames > .shot'));
    var at = 0;
    var chosen = false;
    var held = false;
    // How far behind the front each one is, counted round the stack: the
    // one after the front is next to come forward.
    var bring = function (i) {
      at = i;
      tabs.forEach(function (tab, j) {
        tab.setAttribute('aria-selected', j === i ? 'true' : 'false');
        tab.tabIndex = j === i ? 0 : -1;
      });
      frames.forEach(function (frame, j) {
        frame.setAttribute('data-depth', String((j - i + frames.length) % frames.length));
      });
    };
    // The tabs are only any use with this script, so they are hidden in the
    // page and shown here; without it the stack still shows its front.
    screens.querySelector('[role="tablist"]').hidden = false;
    tabs.forEach(function (tab, i) {
      tab.addEventListener('click', function () { chosen = true; bring(i); });
      // The arrows walk a row of tabs, the way a row of tabs is walked.
      tab.addEventListener('keydown', function (event) {
        var step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
        if (!step) return;
        event.preventDefault();
        var next = (i + step + tabs.length) % tabs.length;
        chosen = true;
        bring(next);
        tabs[next].focus();
      });
    });
    // One showing behind the front can be picked by itself.
    frames.forEach(function (frame, i) {
      frame.addEventListener('click', function () {
        if (frame.getAttribute('data-depth') === '0') return;
        chosen = true;
        bring(i);
      });
    });
    // A mouse resting on them holds the clock. Only a mouse: a finger that
    // taps has entered and never leaves, and would hold it for good.
    screens.addEventListener('pointerenter', function (event) { if (event.pointerType === 'mouse') held = true; });
    screens.addEventListener('pointerleave', function (event) { if (event.pointerType === 'mouse') held = false; });
    var still = false;
    try { still = matchMedia('(prefers-reduced-motion: reduce)').matches; } catch (e) {}
    if (!still) {
      setInterval(function () {
        if (chosen || held || document.hidden) return;
        bring((at + 1) % frames.length);
      }, 5000);
    }
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

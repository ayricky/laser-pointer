// The product film: a Windows desktop where the laser demonstrates itself, chapter by chapter,
// until the visitor presses on the screen; then they're drawing, with the same code. Pages give
// the screen element and hooks for the captions, keys and chapter progress they show around it.
(function () {
  'use strict';

  const APPS = { present: '#d24726', teach: '#7719aa', demo: '#0f6cbd', code: '#23a9f2' };
  const COLOURS = ['#ff2a2a', '#ff8a00', '#ffd400', '#2ecc40', '#1e90ff', '#d62aff'];
  const svgUri = svg => 'data:image/svg+xml,' + encodeURIComponent(svg);
  const TRAY = colour => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2.5 13C5 13.6 7.4 11.6 7.6 9.4C7.9 6.6 4.6 6.4 4.4 8.6C4.2 11 9 11.4 12.2 4.6" fill="none" stroke="#888" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12.4" cy="4" r="2" fill="${colour}"/></svg>`;
  const WC = '<span class="wc">—&nbsp;&nbsp;▢&nbsp;&nbsp;✕</span>';
  /** After this long without the visitor, the film starts again. */

  const DESKTOP = `
    <div class="window shown" data-scene="present">
      <div class="titlebar"><i class="glyph" style="background:${APPS.present}"></i>Northstar Q3 Review — PowerPoint${WC}</div>
      <div class="body"><div class="slide">
        <div>
          <div class="eyebrow">NORTHSTAR · Q3 REVIEW</div>
          <h3><span data-m="p-title">Less friction. More shipped.</span></h3>
          <div class="figure" data-m="p-figure">+38<small>%</small></div>
          <ul><li>Build times down to 2.4 minutes</li><li>99.98% platform availability</li><li>One release. Every team. Every day.</li></ul>
        </div>
        <div class="chart"><div style="height:42%"><span>92</span></div><div style="height:58%"><span>121</span></div><div style="height:52%"><span>110</span></div><div class="lead" data-m="p-bar" style="height:82%"><span>157</span></div></div>
      </div></div>
    </div>
    <div class="window" data-scene="teach">
      <div class="titlebar"><i class="glyph" style="background:${APPS.teach}"></i>Algebra · Week 3 — OneNote${WC}</div>
      <div class="body"><div class="sheet">
        <h4>Solve for x</h4>
        <span class="step">3(x + 4) = 21</span>
        <span class="step"><span class="mistake" data-m="t-mistake">3x + 4</span> = 21</span>
        <span class="step" data-m="t-fix">3x + 12 = 21</span>
        <span class="step"><span data-m="t-answer">x = 3</span></span>
        <div class="note">Multiply the 3 into both terms in the brackets.</div>
      </div></div>
    </div>
    <div class="window" data-scene="demo">
      <div class="titlebar"><i class="glyph" style="background:${APPS.demo}"></i>Acme Console — Microsoft Edge${WC}</div>
      <div class="body"><div class="app">
        <aside><div>Overview</div><div class="on">Notifications</div><div>Billing</div><div>Members</div></aside>
        <main>
          <h4>Notifications</h4>
          <div class="field"><span>Weekly summary email</span><span class="toggle on"></span></div>
          <div class="field" data-m="d-row"><span>Alert me when a deploy fails</span><span class="toggle" data-m="d-toggle"></span></div>
          <div class="field"><span>Mentions in comments</span><span class="toggle"></span></div>
          <span class="save" data-m="d-save">Save changes</span>
        </main>
      </div></div>
    </div>
    <div class="window dark" data-scene="code">
      <div class="titlebar"><i class="glyph" style="background:${APPS.code}"></i>cache.ts — Visual Studio Code${WC}</div>
      <div class="body"><div class="code">
        <span class="line c">// src/cache.ts</span>
        <span class="line"><span class="k">export async function</span> <span class="f">resolve</span>(key: string) {</span>
        <span class="line del">-  <span class="k">return await</span> <span class="f">fetchValue</span>(key);</span>
        <span class="line add">+  <span class="k">const</span> hit = cache.<span class="f">get</span>(key);</span>
        <span class="line add">+  <span data-m="c-hit"><span class="k">if</span> (hit) <span class="k">return</span> hit;</span></span>
        <span class="line add">+  <span class="k">const</span> value = <span class="k">await</span> <span class="f">fetchValue</span>(key);</span>
        <span class="line add">+  <span data-m="c-set">cache.<span class="f">set</span>(key, value, { ttl: <span class="s">60_000</span> });</span></span>
        <span class="line add">+  <span data-m="c-return"><span class="k">return</span> value;</span></span>
        <span class="line">}</span>
      </div></div>
    </div>
    <div class="taskbar">
      <div class="icon"><i style="background:linear-gradient(135deg,#60a5fa,#2563eb)"></i></div>
      <div class="icon" data-m="tb-app"><i style="background:${APPS.present}"></i></div>
      <div class="icon active"><img alt="" src="LOGO"></div>
      <div class="tray"><span style="opacity:.6">^</span><img data-m="tray" alt=""><span>10:24</span></div>
    </div>
    <div class="film-tools" data-m="tools" aria-label="Laser settings" inert>
      <div class="seg" data-m="presets"><button data-preset="Laser">Laser</button><button data-preset="Highlighter">Highlighter</button><button data-preset="Pen">Pen</button></div>
      <span class="sep"></span><div data-m="dots" style="display:flex;gap:8px"></div>
      <span class="sep"></span><div data-m="curs" style="display:flex;gap:2px"></div>
      <span class="sep"></span><button data-m="wipe">Clear</button><button class="resume" data-m="resume">▶ Resume film</button>
    </div>`;

  const INVITES = {
    follow: '<div class="film-follow" data-m="follow"><i></i>It\'s live · press and drag to draw</div>',
    badge: '<button class="film-badge" data-m="badge"><i></i>Live demo · Try it</button>',
  };

  /**
   * hooks: caption(text, chapter), keys(list), snap(shape), progress(chapter, 0..1), mode(driving).
   * invites: any of 'follow' (a tag by the mouse) and 'badge' (a button in the corner).
   */
  function create(screen, { hooks = {}, invites = ['follow'], logo = 'img/logo.svg?v=2' } = {}) {
    const call = (name, ...args) => hooks[name] && hooks[name](...args);
    screen.classList.add('film-screen');
    screen.insertAdjacentHTML('beforeend', DESKTOP.replace('LOGO', logo) + invites.map(i => INVITES[i]).join(''));
    const m = name => screen.querySelector(`[data-m="${name}"]`);
    const cursorById = id => CURSORS.find(c => c.id === id);

    const stage = new Laser.Stage(screen, { cursor: cursorById('soft-glow'), manual: true, onChange: changed });
    const K = GhostKit.kit(stage), { act, draw, click } = GhostKit;
    const film = { stage, chapter: 0, driving: false, started: false, chapters: null, play, drive };
    let colour = '#ff2a2a';

    function changed() {
      m('tray').src = svgUri(TRAY(stage.on ? stage.look.color : '#88888866'));
      if (stage.lastShape) { call('snap', stage.lastShape); stage.lastShape = null; }
      const preset = stage.look.ink ? 'Pen' : stage.look.width >= 12 ? 'Highlighter' : 'Laser';
      for (const b of m('presets').children) b.setAttribute('aria-pressed', b.dataset.preset === preset);
      for (const b of m('dots').children) b.setAttribute('aria-pressed', b.dataset.colour === stage.look.color);
      for (const b of m('curs').children) b.setAttribute('aria-pressed', b.dataset.id === stage.cursor.id);
    }

    // ---- What the film does between strokes -----------------------------------------------------
    function show(scene) {
      for (const w of screen.querySelectorAll('.window')) w.classList.toggle('shown', w.dataset.scene === scene);
      m('tb-app').firstElementChild.style.background = APPS[scene];
      stage.clear(() => true);
      stage.forgetHistory();
      if (scene === 'demo') m('d-toggle').classList.remove('on');
    }
    function look(preset, options = {}) {
      stage.usePreset(preset);
      stage.setLook(options);
      if (options.quiet) stage.toast = null;
    }
    const caption = text => call('caption', text, film.chapter);
    const keys = list => call('keys', list);
    const holdAlt = () => ({ before: () => keys(['Alt']), after: () => keys([]) });

    // Each chapter opens with its scene, so jumping to one is just starting there.
    const opening = {
      present: () => { show('present'); look('Laser', { color: '#ff2a2a', style: 'glow', quiet: true }); stage.setCursor(cursorById('soft-glow')); caption('Point at it. It fades by itself.'); keys([]); },
      teach: () => { show('teach'); look('Pen'); stage.setCursor(cursorById('circle-dot')); caption('The Pen keeps what you write.'); keys([]); },
      demo: () => { show('demo'); look('Highlighter'); stage.setCursor(cursorById('segment-ring')); caption('Highlight exactly where to look.'); keys([]); },
      code: () => { show('code'); look('Laser', { color: '#1e90ff', style: 'neon', quiet: true }); stage.setCursor(cursorById('brackets')); caption('Glow, neon or solid, in any colour.'); keys([]); },
    };
    film.chapters = [
      { name: 'Presenting', blurb: 'Underline, circle, point. It fades by itself.', scene: 'present', steps: [
        act(opening.present, 900),
        draw(() => K.underline(m('p-title'), { gap: 8 })),
        act(() => caption('Circle the number. It snaps clean.'), 200),
        draw(() => K.circle(m('p-figure'), { pad: 18 })),
        act(() => caption('Draw an arrow. It straightens up.'), 200),
        draw(() => K.arrow(m('p-bar'), { from: 'above', length: 130 }), { wait: 1500 }),
      ] },
      { name: 'Teaching', blurb: 'The Pen keeps your marks. Alt draws dead straight.', scene: 'teach', steps: [
        act(opening.teach, 1000),
        draw(() => K.circle(m('t-mistake'), { pad: 12 })),
        draw(() => K.tick(m('t-fix'), { size: 26 }), { snap: false }),
        act(() => caption('Hold Alt for a perfectly straight line.'), 300),
        draw(() => K.underline(m('t-answer'), { gap: 10, wave: 7, from: -0.15, to: 1.3 }), { lock: 'horizontal', ...holdAlt() }),
        act(() => { caption('Alt + X wipes it clean.'); keys(['Alt', 'X']); }, 500),
        act(() => { stage.clear(() => true); stage.forgetHistory(); setTimeout(() => keys([]), 400); }, 1100),
      ] },
      { name: 'Demos', blurb: 'Highlight the setting, then click right through.', scene: 'demo', steps: [
        act(opening.demo, 1000),
        draw(() => K.across(m('d-row'), { pad: 4, wave: 5 }), { lock: 'horizontal', ...holdAlt() }),
        act(() => { look('Laser', { color: '#ff2a2a', quiet: true }); caption('Right-click clicks right through.'); }, 300),
        click(m('d-toggle'), K, () => m('d-toggle').classList.add('on')),
        act(() => caption('Then point them to the next step.'), 100),
        draw(() => K.arrow(m('d-save'), { from: 'right', length: 140 }), { wait: 1400 }),
      ] },
      { name: 'Code review', blurb: 'Neon in any colour, and eight cursors.', scene: 'code', steps: [
        act(opening.code, 1000),
        // The whole change, boxed: a box much wider than it is tall reads as a line, not a box.
        draw(() => { const top = K.box(m('c-hit')), wide = K.box(m('c-set')), end = K.box(m('c-return')); return K.rectAt(top.x - 12, top.y - 8, wide.x + wide.w + 14, end.y + end.h + 8); }),
        act(() => { stage.setCursor(cursorById('classic-cross')); caption('Eight cursors, each with its own motion.'); }, 500),
        draw(() => K.underline(m('c-set'), { gap: 4 })),
        draw(() => K.arrow(m('c-return'), { from: 'right', length: 150 }), { wait: 1600 }),
      ] },
    ];

    const script = [], starts = [];
    film.chapters.forEach((ch, c) => {
      starts.push(script.length);
      ch.steps.forEach((step, i) => script.push(s => { setChapter(c, (i + 1) / ch.steps.length); return step(s); }));
    });
    function setChapter(c, k) { film.chapter = c; call('progress', c, k); }

    /** Plays from chapter `c`, switching to it at once. */
    function play(c = film.chapter) {
      film.started = true;
      setDriving(false);
      setChapter(c, 1 / film.chapters[c].steps.length);
      film.chapter = c;
      opening[film.chapters[c].scene]();
      stage.play(script, { start: starts[c] + 1, pause: 650, again: 1800, delay: 700 });
      if (!stage.ghost) stage.setOn(false); // reduced motion: the scene, without the film
    }

    // ---- Your turn -------------------------------------------------------------------------------
    // Once the visitor takes the laser, the film stays stopped until they ask for it back: a page
    // that starts moving again by itself is one nobody can pause.
    function setDriving(on) {
      film.driving = on;
      screen.classList.toggle('driving', on);
      // Whatever is hidden can't be tabbed to; focus moves to what replaced it.
      const tools = m('tools'), badge = m('badge'), focused = document.activeElement;
      tools.inert = !on;
      if (badge) badge.inert = on;
      if (!on && tools.contains(focused) && badge) badge.focus();
      if (on && badge && badge === focused) m('presets').firstElementChild.focus();
      call('mode', on);
    }
    /** The visitor takes the laser. With a press, that press starts their first stroke. */
    function drive(press) {
      film.started = true;
      if (!film.driving) {
        stage.takeOver();
        keys([]);
        look('Laser', { color: colour, quiet: true });
        setDriving(true);
        caption('Your turn. Draw anywhere on the screen.');
      }
      if (!stage.on) stage.setOn(true);
      if (press) {
        const r = stage.canvas.getBoundingClientRect();
        stage.pointer = { x: press.clientX - r.left, y: press.clientY - r.top };
        stage.inside = true;
        stage.canvas.dispatchEvent(new PointerEvent('pointerdown', {
          bubbles: true, pointerId: press.pointerId, pointerType: press.pointerType, isPrimary: true, button: 0, buttons: 1,
          clientX: press.clientX, clientY: press.clientY, altKey: press.altKey, shiftKey: press.shiftKey, ctrlKey: press.ctrlKey,
        }));
      }
    }
    // A press anywhere on the screen, film playing or laser off, draws. Not a finger while the
    // film plays: that swipe is someone scrolling the page, so touch takes over with the buttons,
    // and from then on the screen is for drawing.
    screen.addEventListener('pointerdown', e => {
      if (e.button !== 0 || e.target.closest('.film-tools, .film-badge')) return;
      if (e.pointerType === 'touch' && !film.driving) return;
      if (film.driving && stage.on) return;
      e.preventDefault();
      drive(e);
    });
    screen.addEventListener('pointerenter', () => screen.classList.add('hover'));
    screen.addEventListener('pointerleave', () => screen.classList.remove('hover'));
    const follow = m('follow');
    screen.addEventListener('pointermove', e => {
      if (!follow) return;
      const r = screen.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
      follow.style.transform = `translate(${Math.min(x + 18, r.width - follow.offsetWidth - 8)}px, ${y + 22}px)`;
    });

    if (m('badge')) m('badge').addEventListener('click', () => drive());

    m('resume').addEventListener('click', () => play(film.chapter));
    m('wipe').addEventListener('click', () => stage.clear(() => true));
    m('presets').addEventListener('click', e => {
      const name = e.target.dataset.preset; if (!name) return;
      stage.usePreset(name);
      if (name === 'Laser') stage.setLook({ color: colour });
    });
    for (const c of COLOURS) {
      const b = document.createElement('button');
      b.className = 'dot'; b.style.background = c; b.dataset.colour = c; b.setAttribute('aria-label', c);
      b.addEventListener('click', () => { colour = c; stage.setLook({ color: c }); });
      m('dots').append(b);
    }
    for (const c of CURSORS) {
      const b = document.createElement('button');
      b.className = 'cur'; b.dataset.id = c.id; b.title = c.name;
      b.innerHTML = `<img alt="${c.name}" src="${svgUri(c.svg.replaceAll('COLOR', '#ff2a2a'))}">`;
      b.addEventListener('click', () => { stage.setCursor(c); changed(); });
      m('curs').append(b);
    }
    // Tools are buttons, not the screen: using them shouldn't draw.
    m('tools').addEventListener('pointerdown', e => e.stopPropagation());

    changed();
    return film;
  }

  window.Film = { create, svgUri };
})();

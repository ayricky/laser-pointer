(function () {
  'use strict';
  const $ = s => document.querySelector(s);
  const $$ = s => [...document.querySelectorAll(s)];
  const overlay = $('#page-laser');
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  let stage, expanded = false, userTouched = false, hasDrawn = false, lastPointerAt = 0, pointerDown = false, scrollTimer, previewTimer;
  function sync(s) {
    document.body.classList.toggle('laser-on', s.on);
    $('#laser-toggle span').textContent = 'Laser ' + (s.on ? 'on' : 'off');
    $('#laser-toggle').classList.toggle('off', !s.on);
    $('#laser-toggle').setAttribute('aria-pressed', String(s.on));
    $('#panel-toggle').setAttribute('aria-checked', String(s.on));
    $('#panel-status').innerHTML = '<i style="background:' + (s.on ? s.look.color : '#999') + '"></i> Laser is ' + (s.on ? 'on' : 'off');
    $$('.preset-buttons [data-preset]').forEach(b => b.setAttribute('aria-pressed', String(Laser.presets[s.preset]?.name === b.dataset.preset)));
    $$('[data-colour]').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.colour === s.look.color)));
    $$('[data-style]').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.style === s.look.style)));
    $$('[data-key]').forEach(input => {
      const key = input.dataset.key, value = s.look[key];
      input.value = value;
      input.style.background = 'linear-gradient(to right,#33342e ' + ((value - input.min) / (input.max - input.min) * 100) + '%,#dcd8cd 0)';
      $('#' + key + '-value').textContent = key === 'fade' ? (value / 1000).toFixed(1) + ' s' : value + ({width:' px',opacity:'%',trail:' px'}[key] || '');
    });
    $('#keep').checked = s.look.ink;
    const line = $('#preview-line');
    line.style.stroke = s.look.color; line.style.strokeWidth = s.look.width;
    line.style.opacity = s.look.opacity / 100;
    line.style.filter = s.look.style === 'solid' ? 'none' : 'drop-shadow(0 0 ' + (s.look.style === 'neon' ? '5' : '3') + 'px ' + s.look.color + '80)';
  }
  stage = Laser.stage(overlay, {on:true, cursor:MarginCursor, keyboard:true, onChange:sync});
  window.marginDemo = stage;
  // Bootstrap at viewport height so the first ghost arrives immediately, even
  // on a long mobile page. Then grow the SAME stage to the document. The engine
  // preserves existing stroke coordinates when its canvas is resized.
  function expandStage() {
    if (expanded) return;
    expanded = true;
    const restartHero = stage.state.ghosting && !userTouched && scrollY < 40;
    if (stage.state.ghosting) stage.stop();
    overlay.classList.add('document-stage');
    if (restartHero) requestAnimationFrame(() => { if (!userTouched && scrollY < 40) heroGhost(); });
  }
  setTimeout(expandStage, 1000);
  function rect(el) {
    const r = (typeof el === 'string' ? $(el) : el).getBoundingClientRect();
    const origin = overlay.getBoundingClientRect();
    return {x:(r.left - origin.left) / overlay.clientWidth,y:(r.top - origin.top) / overlay.clientHeight,w:r.width / overlay.clientWidth,h:r.height / overlay.clientHeight};
  }
  function circle(el, padding) {
    const r = rect(el), pad = (padding || 8);
    return {circle:[r.x + r.w / 2,r.y + r.h / 2,r.w / 2 + pad / overlay.clientWidth,r.h * .43 + pad / overlay.clientHeight]};
  }
  function underline(x1, x2, y) {
    return {path:Array.from({length:25},(_,i) => [x1 + (x2 - x1) * i / 24,y + Math.sin(i / 24 * Math.PI * 3) / overlay.clientHeight])};
  }
  function heroGhost() {
    const mark = rect('#mark');
    const tickX = Math.max(.035,mark.x - 34 / innerWidth), tickY = mark.y + mark.h * .5;
    stage.play([{preset:'Laser'},{look:{fade:1900,trail:1100,width:3}},
      {tick:[tickX,tickY,19 / innerWidth]}, circle('#moment',9),
      underline(mark.x,mark.x + mark.w,mark.y + mark.h * .96),
      {wait:1200},{loop:true}]);
  }
  heroGhost();
  // One document-sized stage anchors both fading and kept ink to the page.
  // No scroll adapter, stroke translation, or synthetic pointer input is used.
  const seen = new Set();
  function artefactSteps(el) {
    const kind = el.dataset.scene, steps = [];
    if (kind === 'teaching') {
      steps.push({preset:'Pen'},{look:{width:2.5}}, circle('#mistake',7));
    } else if (kind === 'reviewing') {
      steps.push({preset:'Highlighter'});
      const origin = overlay.getBoundingClientRect();
      for (const r of $('#clause').getClientRects()) steps.push(underline((r.left - origin.left) / overlay.clientWidth,(r.right - origin.left) / overlay.clientWidth,(r.top - origin.top + r.height * .54) / overlay.clientHeight));
    } else if (kind === 'designing') {
      const r = rect('#spacing');
      steps.push({preset:'Laser'},{look:{fade:2200}},circle('#spacing',7),{arrow:[r.x + r.w * .6,r.y + r.h + 46 / overlay.clientHeight,r.x + r.w * .6,r.y + r.h + 7 / overlay.clientHeight]});
    } else if (kind === 'coaching') {
      const r = rect('.tactics svg');
      steps.push({preset:'Pen'},{look:{width:3}},{arrow:[r.x + r.w * .62,r.y + r.h * .25,r.x + r.w * .84,r.y + r.h * .16]}, {arrow:[r.x + r.w * .46,r.y + r.h * .49,r.x + r.w * .66,r.y + r.h * .3]});
    } else {
      steps.push({preset:'Laser'},{look:{fade:2200}},circle('#growth',8));
    }
    steps.push({wait:900},{preset:'Laser'});
    return steps;
  }
  function scrollScene() {
    clearTimeout(scrollTimer);
    expandStage();
    if (location.search.includes('check=')) return;
    if (!stage.state.on || pointerDown) return;
    // Cancel ghost travel while scrolling; all real and kept strokes survive.
    stage.stop();
    if (hasDrawn) return;
    scrollTimer = setTimeout(() => {
      if (pointerDown || Date.now() - lastPointerAt < 350) return;
      const visible = $$('.artifact').filter(el => {
        const r = el.getBoundingClientRect();
        return !seen.has(el) && r.top > 65 && r.top + Math.min(r.height, 350) < innerHeight - 20;
      });
      if (visible.length) { visible.forEach(el => seen.add(el));stage.play(visible.flatMap(artefactSteps)); }
      else if ($('#the-app').getBoundingClientRect().top < innerHeight && $('#the-app').getBoundingClientRect().bottom > 0) stage.usePreset('Laser');
    },450);
  }
  addEventListener('scroll',scrollScene,{passive:true});
  overlay.addEventListener('pointermove', () => { userTouched = true;lastPointerAt = Date.now();clearTimeout(scrollTimer); },{passive:true});
  overlay.addEventListener('pointerdown', e => { userTouched = true;if (e.button === 0) {pointerDown = true;hasDrawn = true;}clearTimeout(scrollTimer); },{passive:true});
  addEventListener('pointerup',() => {pointerDown = false;},{passive:true});
  addEventListener('pointercancel',() => {pointerDown = false;},{passive:true});
  function toggle() { userTouched = true;stage.stop().toggle(); }
  $('#laser-toggle').onclick = toggle; $('#panel-toggle').onclick = toggle;
  // Keyboard works even before the pointer enters the full-page stage.
  document.addEventListener('keydown',e => {
    if (e.key === 'Escape') {userTouched = true;stage.stop().setOn(false);return;}
    if (e.defaultPrevented || e.target.closest('input,textarea,select') || e.ctrlKey || e.metaKey) return;
    if (e.key.toLowerCase() === 'l') {e.preventDefault();toggle();}
  });
  // Add Ctrl+wheel thickness, which the shared stage does not implement.
  overlay.addEventListener('wheel',e => {
    if (!e.ctrlKey) return;
    e.preventDefault();stage.stop().setLook({width:Math.max(2,Math.min(24,stage.state.look.width + (e.deltaY < 0 ? 1 : -1)))});
  },{passive:false});
  // Right-click activates the underlying page link. Links and controls remain
  // above the canvas, so capture at document level and route their ripple to
  // the shared engine too. Only contextmenu is forwarded, never pointer input.
  let forwardingContext = false;
  document.addEventListener('contextmenu',e => {
    if (forwardingContext || !stage.state.on) return;
    e.preventDefault();
    const canvas = overlay.querySelector('canvas');
    canvas.style.pointerEvents = 'none';
    const beneath = document.elementFromPoint(e.clientX,e.clientY);
    canvas.style.pointerEvents = stage.state.on ? 'auto' : 'none';
    if (!overlay.contains(e.target)) {
      forwardingContext = true;
      try { overlay.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:e.clientX,clientY:e.clientY,button:2})); }
      finally { forwardingContext = false; }
    }
    const link = beneath?.closest('a[href]');
    // Briefly show the ripple before navigation changes the document.
    if (link) setTimeout(() => link.click(), 120);
  },true);
  function preview() {
    clearTimeout(scrollTimer);clearTimeout(previewTimer);userTouched = true;stage.stop().setOn(true);
    const saved = stage.state.look;
    const r = rect('#live-preview');
    // The flyout's controls sit above the shared canvas, its paper below it.
    const path = [[.08,.64],[.24,.38],[.4,.62],[.58,.35],[.76,.57],[.91,.36]].map(p => [r.x + r.w * p[0], r.y + r.h * p[1]]);
    $('#preview-line').style.visibility = 'hidden';
    stage.play([{look:saved},{path:path}]);
    if (!reduced.matches) previewTimer = setTimeout(() => {$('#preview-line').style.visibility = 'visible';},Math.max(1600,saved.fade + 700));
  }
  $$('[data-preset]').forEach(b => b.onclick = () => {stage.stop().usePreset(b.dataset.preset);preview();});
  $$('[data-style]').forEach(b => b.onclick = () => {stage.stop().setLook({style:b.dataset.style});preview();});
  $$('[data-colour]').forEach(b => b.onclick = () => {stage.stop().setLook({color:b.dataset.colour});preview();});
  $$('[data-key]').forEach(input => input.oninput = () => {const value = Number(input.value);clearTimeout(scrollTimer);stage.stop().setLook({[input.dataset.key]:value});clearTimeout(previewTimer);previewTimer = setTimeout(preview,180);});
  $('#keep').onchange = e => {const ink = e.target.checked;stage.stop().setLook({ink:ink});preview();};
  $('#clear').onclick = () => stage.stop().clear();
  function openTab(b) {
    $$('.tabs button').forEach(t => { const active = t === b;t.setAttribute('aria-selected',String(active));t.tabIndex = active ? 0 : -1;$('#' + t.getAttribute('aria-controls')).hidden = !active; });
  }
  $$('.tabs button').forEach(b => {
    b.onclick = () => openTab(b);
    b.onkeydown = e => {if (!['ArrowRight','ArrowLeft','Home','End'].includes(e.key)) return;e.preventDefault();const tabs = $$('.tabs button');const index = e.key === 'Home' ? 0 : e.key === 'End' ? 3 : (tabs.indexOf(b) + (e.key === 'ArrowRight' ? 1 : 3)) % 4;openTab(tabs[index]);tabs[index].focus();};
  });
  // QA hook: same file:// page, deterministic sections and behavioural checks.
  if (location.search.includes('check=')) {
    const check = new URLSearchParams(location.search).get('check');
    if (check !== 'hero') {
      stage.stop();
      setTimeout(() => {
        const el = document.getElementById(check) || document.querySelector('[data-scene="' + check + '"]');
        el?.scrollIntoView({block:'center',behavior:'instant'});
        if (el?.dataset.scene) stage.clear().play(artefactSteps(el));
      },100);
    }
  }
})();

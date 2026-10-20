(function () {
  'use strict';
  const $ = id => document.getElementById(id);
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const syntheticId = 6001;
  const format = n => Math.round(n).toLocaleString('en-GB');
  let inputTimes = [];

  // Scripted comparison ONLY: the shared API has no raw-point feed. Real
  // mouse/pen/touch input goes directly to Laser.stage, without interception.
  function feedGhost(canvas, type, x, y, stamp, samples) {
    const e = new PointerEvent(type, {bubbles:true, pointerId:syntheticId,
      pointerType:'mouse', clientX:x, clientY:y, button:0,
      buttons:type === 'pointerup' ? 0 : 1, isPrimary:true});
    Object.defineProperty(e, 'timeStamp', {value:stamp});
    if (samples) Object.defineProperty(e, 'getCoalescedEvents', {value:() => samples});
    canvas.dispatchEvent(e);
  }
  function comparisonCapture(canvas) {
    for (const method of ['setPointerCapture','releasePointerCapture','hasPointerCapture']) {
      const native = canvas[method].bind(canvas);
      canvas[method] = id => id === syntheticId ? (method === 'hasPointerCapture' ? false : undefined) : native(id);
    }
  }
  function interactiveStage(element, options) {
    return Laser.stage(element, {...options, cursor:PrecisionCursor, keyboard:true,
      onChange(state) {
        element.dataset.laserOn=String(state.on);
        options.onChange?.(state);
      }
    });
  }
  $('hero-stage').addEventListener('pointermove', e => {
    const samples=e.getCoalescedEvents?.(), t=performance.now();
    for(let i=0;i<(samples?.length||1);i++)inputTimes.push(t);
  }, {passive:true});

  function heroScript() {
    // Anchor annotations to real slide content at every viewport size.
    const stage=$('hero-stage'), r=stage.getBoundingClientRect();
    const insight=stage.querySelector('.slide-insight').getBoundingClientRect();
    const title=stage.querySelector('.slide h2').getBoundingClientRect();
    const bar=stage.querySelector('.latest i').getBoundingClientRect();
    const x=v=>(v-r.left)/r.width, y=v=>(v-r.top)/r.height;
    return [
      {wait:30}, {circle:[x(insight.left+insight.width/2),y(insight.top+insight.height/2),(insight.width/2+13)/r.width,(insight.height/2+8)/r.height]}, {wait:100},
      {underline:[x(title.left),x(title.left+Math.min(180,title.width*.7)),y(title.bottom+5)]}, {wait:130},
      {arrow:[x(bar.left-35),y(bar.top-55),x(bar.left+bar.width/2),y(bar.top-6)]}, {wait:400}, {loop:true}
    ];
  }
  let heroGhost = heroScript();
  const hero = interactiveStage($('hero-stage'), {ghost:heroGhost,
    look:{width:3,fade:1500,trail:620,smoothing:2}, resume:0});
  let heroSize = '';
  new ResizeObserver(entries => {
    const r=entries[0].contentRect, key=r.width+'x'+r.height;
    if(heroSize && key!==heroSize && hero.state.ghosting) {heroGhost=heroScript();hero.clear().play(heroGhost);}
    heroSize=key;
  }).observe($('hero-stage'));

  // Instrument ONLY this canvas. Observe the shared renderer's actual polygons;
  // do not fork its geometry or its scheduler. A microtask finishes the counters
  // after each render. Stroke bounding rectangles are a browser-side estimate:
  // the shared demo clears its canvas, unlike the native app's dirty-rect renderer.
  const instrumentCanvas = $('hero-stage').querySelector('canvas');
  const ctx = instrumentCanvas.getContext('2d');
  const original = Object.fromEntries(['clearRect','beginPath','moveTo','lineTo','fill'].map(k => [k,ctx[k].bind(ctx)]));
  let path = [], rectangles = [], previousRectangles = [], fillIndex = 0;
  let cursorRendering = false, instruments = true, lastFrame = 0, lastDisplay = -1000, intervals = [];
  function unionArea(rects) {
    const xs = [...new Set(rects.flatMap(r => [r.l,r.r]))].sort((a,b) => a-b);
    let area = 0;
    for (let i = 1; i < xs.length; i++) {
      const ys = rects.filter(r => r.l < xs[i] && r.r > xs[i-1]).map(r => [r.t,r.b]).sort((a,b) => a[0]-b[0]);
      let length = 0, bottom = -Infinity;
      for (const [top,end] of ys) { length += Math.max(0,end-Math.max(top,bottom)); bottom = Math.max(bottom,end); }
      area += (xs[i]-xs[i-1])*length;
    }
    return area;
  }
  ctx.clearRect = function (...args) {
    original.clearRect(...args);
    const t = performance.now(), dt = t-lastFrame;
    if (lastFrame && dt > 0 && dt < 120) { intervals.push(dt); intervals = intervals.slice(-36); }
    lastFrame = t; previousRectangles = rectangles; rectangles = []; fillIndex = 0;
    queueMicrotask(() => {
      inputTimes = inputTimes.filter(n => t-n < 1000);
      const area = rectangles.length ? unionArea([...rectangles,...previousRectangles])*(devicePixelRatio || 1)**2 : 0;
      if (t-lastDisplay > 100 || !area) {
        $('input-rate').innerHTML = format(inputTimes.length)+' <small>samples</small>';
        $('pixel-count').textContent = format(area);
        $('frame-time').textContent = intervals.length ? intervals[intervals.length-1].toFixed(1) : '—';
        $('spark').setAttribute('d', intervals.map((v,i) => (i?'L':'M')+(i*76/35).toFixed(1)+' '+(19-Math.min(18,v/3)).toFixed(1)).join(' '));
        lastDisplay = t;
      }
    });
  };
  ctx.beginPath = function () { path = []; original.beginPath(); };
  ctx.moveTo = function (x,y) { path.push([x,y]); original.moveTo(x,y); };
  ctx.lineTo = function (x,y) { path.push([x,y]); original.lineTo(x,y); };
  ctx.fill = function (...args) {
    original.fill(...args);
    // Hero always uses Glow: the first of each three fills is its outer bound.
    if (cursorRendering || !path.length) return;
    const outlineThisFill = fillIndex++ % 3 === 0;
    const w = $('hero-stage').clientWidth, h = $('hero-stage').clientHeight;
    const r = {l:Math.max(0,Math.min(...path.map(p=>p[0]))),r:Math.min(w,Math.max(...path.map(p=>p[0]))),t:Math.max(0,Math.min(...path.map(p=>p[1]))),b:Math.min(h,Math.max(...path.map(p=>p[1])))};
    if (r.r <= r.l || r.b <= r.t) return;
    rectangles.push(r);
    if (instruments && outlineThisFill) {
      ctx.save();ctx.strokeStyle='#67727e80';ctx.lineWidth=.65;ctx.setLineDash([3,3]);
      ctx.strokeRect(Math.floor(r.l)+.5,Math.floor(r.t)+.5,Math.ceil(r.r-r.l),Math.ceil(r.b-r.t));ctx.restore();
    }
  };
  hero.setCursor({size:24,draw(c,x,y,s) {cursorRendering=true;try {PrecisionCursor.draw(c,x,y,s);} finally {cursorRendering=false;}}});
  $('instruments').addEventListener('change', e => {instruments=e.target.checked;$('telemetry').classList.toggle('instruments-off',!instruments);});
  // A single delayed idle correction; no persistent polling or second rAF loop.
  let inputReset;
  $('hero-stage').addEventListener('pointermove', () => {
    clearTimeout(inputReset); inputReset=setTimeout(()=>{$('input-rate').innerHTML='0 <small>samples</small>';},1100);
  }, {capture:true});

  // Deterministic sample comparison. A 1px clock stage uses the shared engine's
  // scheduler and visibility/reduced-motion handling. Its render drives both
  // actual Laser stages with identical packets, decimating only the left feed.
  // There is no page-level requestAnimationFrame loop.
  const compareLook = {width:3,fade:2000,trail:1900,smoothing:0,style:'neon'};
  let comparisonActive = false, comparisonRunning = false, cycleStart = 0;
  let fullStarted=false, processed=-1, lastSample=-100, lastLeft=null, lastFull=null;
  let sampleCount=0, fullCount=0;
  const sampled = interactiveStage($('sampled-stage'), {look:compareLook});
  const full = interactiveStage($('full-stage'), {look:compareLook});
  const canvases = [$('sampled-stage').querySelector('canvas'),$('full-stage').querySelector('canvas')];
  canvases.forEach(comparisonCapture);
  const dotLayers=canvases.map(canvas=>{
    const dots=document.createElement('canvas');dots.className='sample-dots';dots.setAttribute('aria-hidden','true');
    canvas.parentElement.append(dots);return dots;
  });
  let sampleClouds=[[],[]];
  const clock = Laser.stage($('comparison-clock'), {on:false});
  const clockCtx = $('comparison-clock').querySelector('canvas').getContext('2d');
  const clockClear = clockCtx.clearRect.bind(clockCtx);
  const flickDuration = 210;
  function flick(u) {
    // A tight, fast scribble: roughly 700 CSS px of travel in 210 ms on desktop.
    return [.12+.76*u, .5-.27*Math.sin(u*Math.PI*5.4)*Math.pow(Math.sin(Math.PI*u),.45)];
  }
  function drawDots(t, kept=false) {
    dotLayers.forEach((canvas,i)=>{
      const w=canvas.parentElement.clientWidth,h=canvas.parentElement.clientHeight,dpr=devicePixelRatio||1;
      if(canvas.width!==Math.round(w*dpr)||canvas.height!==Math.round(h*dpr)){canvas.width=Math.round(w*dpr);canvas.height=Math.round(h*dpr);}
      const c=canvas.getContext('2d');c.setTransform(dpr,0,0,dpr,0,0);c.clearRect(0,0,w,h);
      sampleClouds[i]=sampleClouds[i].filter(point=>kept||t-point.t<compareLook.fade);
      for(const point of sampleClouds[i]) {
        const fade=kept?1:Math.min(1,(compareLook.fade-(t-point.t))/450);
        c.globalAlpha=.48*fade;c.beginPath();c.arc(point.p[0]*w,point.p[1]*h,2.2,0,Math.PI*2);
        c.fillStyle='#ffd5d5';c.fill();c.strokeStyle='#111214';c.lineWidth=.8;c.stroke();
      }
      c.globalAlpha=1;
      if(!kept&&comparisonActive) {
        const p=i?lastFull:lastLeft;
        if(p){c.save();PrecisionCursor.draw(c,p[0]*w,p[1]*h,{color:compareLook.color||'#ff2a2a',ignite:1,press:processed<flickDuration?1:0,dpr,t});c.restore();}
      }
    });
  }
  function plotLeft(point,rect,t) {
    // Straight-line reconstruction is deliberate and labelled on the page.
    // Each pair uses the shared engine's two-point stroke path; no renderer fork.
    if(lastLeft) {
      feedGhost(canvases[0],'pointerdown',rect.left+lastLeft[0]*rect.width,rect.top+lastLeft[1]*rect.height,t);
      feedGhost(canvases[0],'pointermove',rect.left+point[0]*rect.width,rect.top+point[1]*rect.height,t);
      feedGhost(canvases[0],'pointerup',rect.left+point[0]*rect.width,rect.top+point[1]*rect.height,t);
    }
    lastLeft=point;sampleClouds[0].push({p:point,t});sampleCount++;
  }
  function endComparisonStroke(t) {
    if(fullStarted&&lastFull) {
      const r=canvases[1].getBoundingClientRect();
      feedGhost(canvases[1],'pointerup',r.left+lastFull[0]*r.width,r.top+lastFull[1]*r.height,t);
    }
    fullStarted=false;
  }
  function resetCycle(t) {endComparisonStroke(t);sampled.clear();full.clear();cycleStart=t;processed=-1;lastSample=-100;sampleCount=0;fullCount=0;lastLeft=null;lastFull=null;sampleClouds=[[],[]];}
  clockCtx.clearRect = function (...args) {
    clockClear(...args);
    if (!comparisonActive || reduced.matches) return;
    const t=performance.now();
    if (!cycleStart || t-cycleStart>3100) resetCycle(t);
    const elapsed=t-cycleStart-180;
    if (elapsed<0) {drawDots(t);return;}
    const to=Math.min(flickDuration,elapsed);
    const rects=canvases.map(c=>c.getBoundingClientRect());
    const fullPacket=[];
    for (let stamp=processed+1;stamp<=to;stamp++) {
      const p=flick(stamp/flickDuration);
      if (stamp%4===0 || stamp===flickDuration) {
        const r=rects[1];fullPacket.push({clientX:r.left+p[0]*r.width,clientY:r.top+p[1]*r.height,timeStamp:cycleStart+180+stamp});
        lastFull=p;sampleClouds[1].push({p,t:cycleStart+180+stamp});fullCount++;
      }
      if (stamp-lastSample>=33 || stamp===flickDuration) {
        plotLeft(p,rects[0],cycleStart+180+stamp);lastSample=stamp;
      }
      processed=stamp;
    }
    if(fullPacket.length) {
      if(!fullStarted) {
        const p=fullPacket[0];feedGhost(canvases[1],'pointerdown',p.clientX,p.clientY,p.timeStamp);fullStarted=true;
      }
      const p=fullPacket[fullPacket.length-1];feedGhost(canvases[1],'pointermove',p.clientX,p.clientY,p.timeStamp,fullPacket);
    }
    $('sampled-count').textContent=sampleCount+' points';$('full-count').textContent=fullCount+' points';
    if(elapsed>=flickDuration&&fullStarted)endComparisonStroke(t);
    drawDots(t);
  };
  function replay() {
    if (reduced.matches) {
      // No autoplay under reduced motion; one static, kept annotated state.
      comparisonActive=false;comparisonRunning=false;clock.stop();
      const times=[[],[]];for(let t=0;t<=flickDuration;t+=33)times[0].push(t);times[0].push(flickDuration);
      for(let t=0;t<=flickDuration;t+=4)times[1].push(t);times[1].push(flickDuration);
      const points=times.map(list=>list.map(t=>flick(t/flickDuration)));
      sampled.clear().play(points[0].slice(1).map((p,i)=>({path:[points[0][i],p]})));
      full.clear().play([{path:points[1]}]);
      sampleClouds=points.map(list=>list.map(p=>({p,t:performance.now()})));drawDots(performance.now(),true);
      $('sampled-count').textContent=points[0].length+' points';$('full-count').textContent=points[1].length+' points';return;
    }
    comparisonActive=true;comparisonRunning=true;cycleStart=0;
    sampled.setLook({...compareLook,ink:false});full.setLook({...compareLook,ink:false});
    clock.play([{wait:3500},{loop:true}]);
  }
  for(const id of ['sampled-stage','full-stage']) {
    // Finish the scripted pointer BEFORE the shared engine receives real input.
    $(id).parentElement.addEventListener('pointermove',e=>{
      if(e.pointerId===syntheticId||!comparisonRunning)return;
      comparisonActive=false;comparisonRunning=false;endComparisonStroke(performance.now());clock.stop();sampleClouds=[[],[]];drawDots(performance.now());
    },{capture:true});
    $(id).parentElement.addEventListener('pointerdown',e=>{
      if(e.pointerId===syntheticId)return;
      comparisonActive=false;comparisonRunning=false;endComparisonStroke(performance.now());clock.stop();sampleClouds=[[],[]];drawDots(performance.now());
    },{capture:true});
  }
  $('replay').addEventListener('click',replay);
  // The shared engine preserves CSS-pixel positions on resize. Restart this
  // controlled experiment so old-size and new-size samples never join up.
  let previousSize = '';
  const compareResize = new ResizeObserver(entries => {
    const r=entries[0].contentRect, key=r.width+'x'+r.height;
    if(previousSize&&key!==previousSize&&reduced.matches)replay();
    if (key!==previousSize && comparisonRunning) {
      endComparisonStroke(performance.now());sampled.clear();full.clear();cycleStart=0;
    }
    previousSize=key;
  });
  compareResize.observe($('full-stage'));
  replay();
  reduced.addEventListener('change',()=>{comparisonActive=false;clock.stop();replay();});

  // Real settings structure, with all pen controls wired to this Laser stage.
  const previewGhost=[{wait:30},{path:[[.12,.65],[.28,.42],[.45,.62],[.63,.4],[.85,.55]]},{wait:200},{loop:true}];
  const preview=interactiveStage($('panel-preview'),{ghost:previewGhost,onChange:syncPanel});
  let controlTimer;
  function syncPanel(state) {
    const look=state.look;
    $('power').setAttribute('aria-checked',String(state.on));$('power').setAttribute('aria-label',state.on?'Laser on':'Laser off');
    $('panel-state').textContent=state.on?'Laser active':'Laser off';
    $('colour-value').textContent=look.color.toUpperCase();
    $('keep-strokes').checked=look.ink;
    document.querySelectorAll('[data-colour]').forEach(b=>{const yes=b.dataset.colour===look.color;b.classList.toggle('selected',yes);b.setAttribute('aria-pressed',String(yes));});
    document.querySelectorAll('[data-style]').forEach(b=>{const yes=b.dataset.style===look.style;b.classList.toggle('selected',yes);b.setAttribute('aria-pressed',String(yes));});
    document.querySelectorAll('[data-preset]').forEach(b=>{const yes=Laser.presets[state.preset]?.name===b.dataset.preset;b.classList.toggle('selected',yes);b.setAttribute('aria-pressed',String(yes));});
    document.querySelectorAll('[data-look]').forEach(input=>{
      const key=input.dataset.look,value=look[key];input.value=value;
      input.style.setProperty('--range',((value-input.min)/(input.max-input.min)*100)+'%');
      const unit={width:'px',opacity:'%',fade:'s',trail:'px',smoothing:''}[key];
      const number=key==='fade'?(value/1000).toFixed(1):value;
      input.nextElementSibling.innerHTML=number+(unit?' <small>'+unit+'</small>':'');
      input.setAttribute('aria-valuetext',number+(unit?' '+unit:''));
    });
  }
  function demonstrate() {
    clearTimeout(controlTimer);
    preview.stop().clear();
    // Replay with the current look: script contains no preset/look overrides.
    if(preview.state.on) preview.play(previewGhost);
  }
  $('power').addEventListener('click',()=>{preview.stop().toggle();if(preview.state.on)demonstrate();});
  document.querySelectorAll('[data-preset]').forEach(b=>b.addEventListener('click',()=>{preview.usePreset(b.dataset.preset);demonstrate();}));
  document.querySelectorAll('[data-colour]').forEach(b=>b.addEventListener('click',()=>{preview.setLook({color:b.dataset.colour});demonstrate();}));
  document.querySelectorAll('[data-style]').forEach(b=>b.addEventListener('click',()=>{preview.setLook({style:b.dataset.style});demonstrate();}));
  document.querySelectorAll('[data-look]').forEach(input=>input.addEventListener('input',()=>{
    preview.setLook({[input.dataset.look]:Number(input.value)});
    clearTimeout(controlTimer);controlTimer=setTimeout(demonstrate,100);
  }));
  $('keep-strokes').addEventListener('change',e=>{preview.setLook({ink:e.target.checked});demonstrate();});
  $('clear-preview').addEventListener('click',()=>preview.stop().clear());
  $('reset').addEventListener('click',()=>{preview.usePreset('Laser').setOn(true);demonstrate();});
  const tabs=[...document.querySelectorAll('[role=tab]')];
  function selectTab(tab) {
    for(const b of tabs){const yes=b===tab;b.setAttribute('aria-selected',String(yes));b.tabIndex=yes?0:-1;$(b.getAttribute('aria-controls')).hidden=!yes;}
  }
  tabs.forEach((tab,i)=>{
    tab.addEventListener('click',()=>selectTab(tab));
    tab.addEventListener('keydown',e=>{
      let index;if(e.key==='ArrowRight')index=(i+1)%tabs.length;else if(e.key==='ArrowLeft')index=(i+tabs.length-1)%tabs.length;else if(e.key==='Home')index=0;else if(e.key==='End')index=tabs.length-1;else return;
      e.preventDefault();selectTab(tabs[index]);tabs[index].focus();
    });
  });
  const benchObserver=new IntersectionObserver(entries=>{if(entries[0].isIntersecting){$('benchmarks').classList.add('revealed');benchObserver.disconnect();}},{threshold:.15});
  benchObserver.observe($('benchmarks'));

  // Keep the fixed pill available in empty space, but yield to actual content
  // when scrolling brings a heading, caption or demo into its corner. No rAF
  // loop: only scroll/resize events schedule this small, read-then-write check.
  const conceptSwitch=document.querySelector('.concept-switch');
  const switchCandidates=[...document.querySelectorAll('main h1,main h2,main h3,main p,main .eyebrow,.spec-strip>span,.stage,.stage-hint,.comparison-foot,.settings-panel,.code-window,.taskbar,.taskbar-caption,.shortcut-row,.app-identity,footer>a,footer>span,footer>div')];
  const overlaps=(a,b)=>a.left<b.right&&a.right>b.left&&a.top<b.bottom&&a.bottom>b.top;
  function positionSwitcher() {
    if(innerWidth<600)return;
    const pill=conceptSwitch.getBoundingClientRect();
    const obscured=switchCandidates.some(element=>{
      const box=element.getBoundingClientRect();if(!overlaps(pill,box))return false;
      if(element.matches('.stage,.settings-panel,.code-window,.taskbar,.app-identity'))return true;
      const range=document.createRange();range.selectNodeContents(element);
      return [...range.getClientRects()].some(rect=>overlaps(pill,rect));
    });
    conceptSwitch.dataset.obscured=String(obscured);conceptSwitch.inert=obscured;
    conceptSwitch.setAttribute('aria-hidden',String(obscured));
  }
  let switchTimer;
  function scheduleSwitcher(){clearTimeout(switchTimer);switchTimer=setTimeout(positionSwitcher,0);}
  addEventListener('scroll',scheduleSwitcher,{passive:true});addEventListener('resize',scheduleSwitcher,{passive:true});positionSwitcher();

  // Opt-in local verification, used solely by the headless self-check.
  if(new URLSearchParams(location.search).has('check')) {
    const report=document.createElement('pre');report.id='check-results';report.style.cssText='position:relative;margin:20px;padding:20px;font:12px monospace;white-space:pre-wrap';document.body.append(report);
    const checks=[];const check=(yes,message)=>checks.push((yes?'PASS ':'FAIL ')+message);
    addEventListener('error',e=>{document.title='ERR '+e.message;report.textContent+='\nERROR '+e.message;});
    setTimeout(()=>{
      try {
        check(document.querySelectorAll('[data-laser]').length===5,'shared engine stages');
        document.querySelector('[data-preset=Highlighter]').click();check(preview.state.look.width===18&&preview.state.look.opacity===40&&preview.state.look.fade===3000,'Highlighter changes live look');
        document.querySelector('[data-colour="#d62aff"]').click();check(preview.state.look.color==='#d62aff','colour changes live');
        document.querySelector('[data-style=neon]').click();check(preview.state.look.style==='neon','style changes live');
        const width=document.querySelector('[data-look=width]');width.value=12;width.dispatchEvent(new Event('input'));check(preview.state.look.width===12,'thickness changes live');
        for(const tab of tabs){tab.click();check(!$(tab.getAttribute('aria-controls')).hidden,'tab '+tab.textContent);}
        $('power').click();check(!preview.state.on,'power off');$('power').click();check(preview.state.on,'power on');
        $('reset').click();check(preview.state.look.color==='#ff2a2a'&&preview.state.look.width===4,'reset preset');
        selectTab(tabs[0]);
        check(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow');
        report.textContent=checks.join('\n');document.title=checks.some(c=>c.startsWith('FAIL'))?'FAIL Precision checks':'PASS Precision checks';
      } catch(e){document.title='ERR '+e.message;report.textContent=checks.join('\n')+'\nERROR '+e.stack;}
    },700);
  }
})();

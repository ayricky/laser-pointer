(function () {
  'use strict';
  const $ = s => document.querySelector(s);
  const $$ = s => [...document.querySelectorAll(s)];
  const stages = {};
  const scripts = {};
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const unit = { width:' px', opacity:' %', fade:' s', trail:' px', smoothing:'' };

  function make(name, id, look, ghost, onChange) {
    const element = $('#'+id);
    scripts[name] = ghost;
    const stage = Laser.stage(element, {cursor:EmberCursor, keyboard:true, look, ghost, onChange(state) {
      element.classList.toggle('laser-on',state.on);
      if(onChange)onChange(state);
    }});
    stages[name] = stage;
    return stage;
  }
  function box(stage, target) {
    const a = stage.getBoundingClientRect(), b = target.getBoundingClientRect();
    return {x:(b.left+b.width/2-a.left)/a.width, y:(b.top+b.height/2-a.top)/a.height, rx:(b.width/2+12)/a.width, ry:(b.height/2+6)/a.height, left:(b.left-a.left)/a.width, right:(b.right-a.left)/a.width, bottom:(b.bottom-a.top)/a.height, top:(b.top-a.top)/a.height};
  }
  function heroScript() {
    const host = $('#hero-stage');
    const stat = box(host, $('.big-stat')), bullet = box(host, $('.slide-story li')), bar = box(host, $('.chart-bar.bright'));
    return [{move:[stat.x+stat.rx,stat.y-.02]}, {circle:[stat.x,stat.y,stat.rx,stat.ry]}, {wait:120}, {underline:[bullet.left,bullet.right,bullet.bottom+.008]}, {arrow:[bar.x-.17,bar.top-.07,bar.x,bar.top+.03]}, {wait:450}, {loop:true}];
  }
  make('hero','hero-stage',{fade:1700,trail:1400,width:innerWidth<600?2.4:3.5},heroScript());
  $('.uptime').innerHTML = '<i></i>'.repeat(30);
  function fadeScript() {
    const host=$('#fade-stage'), stat=box(host,$('.availability-value'));
    return [{circle:[stat.x,stat.y,stat.rx,stat.ry]},{wait:2100},{underline:[.06,.34,stat.bottom+.07]},{wait:1900},{loop:true}];
  }
  function keepScript() {
    const host=$('#keep-stage'), line=box(host,$('.code-row.added > span')), ttl=box(host,$('.code-number'));
    return [
    {do:()=>$('#keep-key').classList.add('lit')},
    {underline:[line.left,line.right,line.bottom+.007]}, {circle:[ttl.x,ttl.y,ttl.rx,ttl.ry]},
    {tick:[.88,.73,.055]}, {wait:2200},
    {do:()=>$('#keep-key').classList.remove('lit')},{wait:900},{loop:true}
    ];
  }
  make('fade','fade-stage',{fade:1600,trail:1300,width:3.5},fadeScript());
  make('keep','keep-stage',{...Laser.presets[2].look},keepScript());
  function lookScript(style) {
    const host=$('#'+style+'-stage'), heading=box(host,host.querySelector('h3')), stat=box(host,host.querySelector('strong'));
    const y=heading.bottom+.035;
    const rx=Math.min(stat.rx*1.15,1-stat.x-20/host.clientWidth);
    const ry=Math.min(stat.ry*1.2,1-stat.y-20/host.clientHeight);
    return [{path:[[.12,y],[.32,y+.02],[.56,y-.02],[.85,y+.01]]},{circle:[stat.x,stat.y,rx,ry]},{wait:900},{loop:true}];
  }
  ['solid','glow','neon'].forEach(style => make(style,style+'-stage',{style,fade:2400,trail:2000,width:8},lookScript(style)));
  make('weight','weight-stage',{fade:1500,width:2.5},[{circle:[.5,.52,.045,.14]},{underline:[.73,.91,.76]},{wait:1700},{loop:true}]);
  const previewScript = [{path:[[.12,.65],[.24,.5],[.38,.68],[.54,.39],[.7,.57],[.88,.37]]},{wait:1100},{loop:true}];
  let app, preview, previous = '';
  function syncPanel(state) {
    const key = JSON.stringify([state.look,state.on]);
    if (key === previous) return;
    previous = key;
    const look = state.look;
    $('#laser-toggle').setAttribute('aria-checked',String(state.on));
    $('#panel-status').textContent = state.on ? (look.ink ? 'Pen is on · strokes stay' : 'Laser is on') : 'Laser is off';
    $('#colour-value').textContent = look.color.toUpperCase();
    $$('[data-preset]').forEach(el => { const active = Laser.presets[state.preset]?.name === el.dataset.preset; el.classList.toggle('selected',active); el.setAttribute('aria-pressed',String(active)); });
    $$('[data-style]').forEach(el => {const active = el.dataset.style === look.style; el.classList.toggle('selected',active); el.setAttribute('aria-pressed',String(active));});
    $$('[data-colour]').forEach(el => el.setAttribute('aria-pressed',String(el.dataset.colour === look.color)));
    $$('[data-look]').forEach(el => {
      const k = el.dataset.look;
      el.value = look[k];
      $('#'+k+'-out').textContent = (k === 'fade' ? look[k]/1000 : look[k]) + unit[k];
      fill(el);
    });
    if (preview) preview.setLook(look).setOn(state.on);
  }
  function appScript(){
    const host=$('#app-stage'), heading=box(host,$('.workspace-slide h3'));
    return [{circle:[heading.x,heading.y,heading.rx*1.2,heading.ry*1.3]},{underline:[.13,.75,.79]},{wait:900},{loop:true}];
  }
  app = make('app','app-stage',{},appScript(),syncPanel);
  preview = make('preview','panel-preview',{},previewScript);
  syncPanel(app.state); preview.setLook(app.state.look);
  function fill(el) {el.style.setProperty('--fill',((Number(el.value)-Number(el.min))/(Number(el.max)-Number(el.min))*100)+'%');}
  $$('input[type=range]').forEach(fill);
  function refreshPreview() {
    preview.stop().clear().setLook(app.state.look).setOn(app.state.on);
    if (app.state.on) preview.play(previewScript);
  }
  function change(partial) {app.stop().setLook(partial);refreshPreview();}
  $$('[data-look]').forEach(el => el.addEventListener('input',()=>change({[el.dataset.look]:Number(el.value)})));
  $$('[data-colour]').forEach(el => el.addEventListener('click',()=>change({color:el.dataset.colour})));
  $$('[data-style]').forEach(el => el.addEventListener('click',()=>change({style:el.dataset.style})));
  $$('[data-preset]').forEach(el => el.addEventListener('click',()=>{app.stop().usePreset(el.dataset.preset);refreshPreview();}));
  $('#laser-toggle').addEventListener('click',()=>{app.stop().toggle();refreshPreview();});
  $('#fade-time').addEventListener('input', e => {
    const value = Number(e.target.value);
    fill(e.target); $('#fade-value').textContent = (value/1000).toFixed(1)+' s';
    stages.fade.setLook({fade:value});
  });
  $$('[data-replay]').forEach(el => el.addEventListener('click',()=>{
    const name = el.dataset.replay;
    stages[name].stop().clear().setOn(true).play(scripts[name]);
  }));
  const tabs = $$('[role=tab]');
  function selectTab(tab) {
    tabs.forEach(el=>{const active=el===tab;el.setAttribute('aria-selected',String(active));el.tabIndex=active?0:-1;$('#'+el.getAttribute('aria-controls')).hidden=!active;});
  }
  tabs.forEach((tab,i)=>{
    tab.addEventListener('click',()=>selectTab(tab));
    tab.addEventListener('keydown',e=>{
      let target;
      if(e.key==='ArrowRight')target=tabs[(i+1)%tabs.length];
      if(e.key==='ArrowLeft')target=tabs[(i+tabs.length-1)%tabs.length];
      if(e.key==='Home')target=tabs[0];
      if(e.key==='End')target=tabs[tabs.length-1];
      if(target){e.preventDefault();selectTab(target);target.focus();}
    });
  });
  let resizeTimer;
  window.addEventListener('resize',()=>{clearTimeout(resizeTimer);resizeTimer=setTimeout(()=>{
    for(const [name,script] of [['hero',heroScript()],['fade',fadeScript()],['keep',keepScript()],['app',appScript()],...['solid','glow','neon'].map(style=>[style,lookScript(style)])]){
      scripts[name]=script;
      if(stages[name].state.ghosting)stages[name].stop().clear().play(script);
    }
    stages.hero.setLook({width:innerWidth<600?2.4:3.5});
  },180);});
  // A DOM-only audit is available to headless Edge, without a browser driver.
  if(new URLSearchParams(location.search).has('check')) {
    const results=[];
    const assert=(ok,label)=>results.push((ok?'PASS ':'FAIL ')+label);
    try {
      assert($$('[data-laser]').length===9,'nine stages use the shared engine');
      $('[data-preset="Highlighter"]').click();
      assert(app.state.look.width===18&&app.state.look.opacity===40&&app.state.look.fade===3000,'Highlighter updates real stage');
      $('[data-colour="#d62aff"]').click();
      assert(app.state.look.color==='#d62aff'&&preview.state.look.color==='#d62aff','colour syncs both previews');
      $('[data-style="neon"]').click();
      assert(app.state.look.style==='neon'&&preview.state.look.style==='neon','style syncs both previews');
      for(const el of $$('[data-look]')){
        el.value=el.dataset.look==='fade'?2400:el.dataset.look==='trail'?900:el.dataset.look==='opacity'?70:7;
        el.dispatchEvent(new Event('input',{bubbles:true}));
        assert(app.state.look[el.dataset.look]===Number(el.value)&&preview.state.look[el.dataset.look]===Number(el.value),el.dataset.look+' slider changes engine');
      }
      $('#laser-toggle').click(); assert(!app.state.on&&!preview.state.on,'off switch controls both stages');
      $('#laser-toggle').click();
      $('[data-preset="Pen"]').click(); assert(app.state.look.ink&&preview.state.look.ink,'Pen keeps strokes');
      tabs[2].click(); assert(!$('#tab-shortcuts').hidden&&$('#tab-pen').hidden,'tabs show correct content');
      selectTab(tabs[0]); $('[data-preset="Laser"]').click();
      $('#fade-time').value=2500;$('#fade-time').dispatchEvent(new Event('input'));
      assert(stages.fade.state.look.fade===2500,'story fade slider controls engine');
      assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow');
      assert($$('img').every(img=>img.complete&&img.naturalWidth>0),'all local icons load');
      assert($$('.stage').every(el=>el.clientHeight>0&&el.clientWidth>0),'all demo stages have dimensions');
      assert($$('script[src]').every(el=>new URL(el.src).origin===location.origin),'all scripts are local');
      assert(stages.hero.state.ghosting||reduced.matches,'hero ghost starts automatically');
    } catch(e){results.push('FAIL '+e.message);}
    const report=document.createElement('pre');report.id='check-results';report.textContent=results.join('\n');report.style.cssText='padding:24px;color:#fff;background:#151515;white-space:pre-wrap';document.body.append(report);
    document.title=results.some(s=>s.startsWith('FAIL'))?'FAIL Spotlight checks':'PASS Spotlight checks';
  }
})();

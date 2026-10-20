(function () {
  'use strict';
  const $ = s => document.querySelector(s);
  const $$ = s => [...document.querySelectorAll(s)];
  const colours = ['#ff2a2a', '#ff8a00', '#ffd400', '#2ecc40', '#1e90ff', '#d62aff'];
  const neon = {color:'#d62aff',style:'neon',width:5,fade:1800,trail:1100,opacity:100,smoothing:3,ink:false};
  const cursor = window.NeonCursor;
  let hero, playground, preview;
  function svgPoint(svg, stage, x, y) {
    const m=svg.getScreenCTM(),b=stage.getBoundingClientRect(),v=svg.viewBox.baseVal;
    return [(m.a*x*v.width+m.e-b.x)/b.width,(m.d*y*v.height+m.f-b.y)/b.height];
  }

  function heroScript() {
    const box = $('#hero').getBoundingClientRect();
    const relative = el => { const b = el.getBoundingClientRect(); return {x:(b.x-box.x)/box.width,y:(b.y-box.y)/box.height,w:b.width/box.width,h:b.height/box.height}; };
    const screen = relative($('#screen-word')), live = relative($('#live-word')), cta = relative($('#hero-cta')), map = relative($('.hero-map'));
    const mapTarget=svgPoint($('.hero-map'),$('#hero'),335/460,108/270);
    return [
      {look:{...neon,fade:2600,trail:1600}},
      {circle:[screen.x+screen.w*.5,screen.y+screen.h*.53,screen.w*.56,screen.h*.58]},
      {underline:[live.x,live.x+live.w,live.y+live.h*.98]},
      {look:{color:'#1e90ff'}},
      {circle:[...mapTarget,map.w*.067,map.h*.125]},
      {look:{color:'#ff2a2a'}},
      {arrow:[cta.x+cta.w+.09,cta.y-.035,cta.x+cta.w+.012,cta.y+cta.h*.42]},
      {wait:900},{loop:true}
    ];
  }
  // Input goes directly to Laser: hovering moves the cursor; left-button dragging draws.
  // This hook only mirrors the power state for text-selection styling.
  function selectionState(element, state) { element.dataset.laserOn=String(state.on); }
  hero = Laser.stage($('#hero'), {look:neon,cursor,keyboard:true,ghost:heroScript(),onChange:s=>selectionState($('#hero'),s)});

  const grid = '<defs><pattern id="stage-grid" width="40" height="40" patternUnits="userSpaceOnUse"><path d="M40 0H0V40" fill="none" stroke="#ffffff" stroke-opacity=".035"/></pattern></defs><rect width="1000" height="560" fill="#0b0b0b"/><rect width="1000" height="560" fill="url(#stage-grid)"/>';
  const svg = content => '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 560" role="img" preserveAspectRatio="xMidYMid meet">'+grid+content+'</svg>';
  const scenes = {
    stream: svg('<title>Esports replay: round 8, take site B through the lower route</title><g font-family="Segoe UI, sans-serif"><text x="40" y="43" font-size="11" fill="#777" letter-spacing="2">FIELDNOTES / MATCH REVIEW</text><text x="40" y="83" font-size="26" fill="#eee" font-weight="600">The retake.</text><text x="40" y="108" font-size="12" fill="#777">Round 08 · map: Foundry · attack phase</text></g><g transform="translate(65,145)" fill="none" stroke="#525252" stroke-width="2"><path d="M0 0H210V50H330V0H565V130H515V335H365V285H210V355H0Z"/><path d="M38 38H165V100H225V155H135V302H38ZM377 45H520V95H465V183H405V143H330V80H377Z" fill="#141414"/><path d="M225 155V285M515 230H405M310 195H365V245H310Z"/><path d="M165 50V0M365 285V335M265 50V100" stroke-dasharray="5 7"/><rect x="250" y="105" width="53" height="46" rx="3" fill="#202020"/><rect x="170" y="225" width="44" height="36" rx="2" fill="#202020"/><circle cx="459" cy="139" r="47" stroke="#6b6b6b" stroke-dasharray="4 6"/></g><g font-family="Segoe UI, sans-serif" font-size="11" fill="#888" letter-spacing="1"><text x="120" y="176">SPAWN</text><text x="484" y="174">SITE B</text><text x="403" y="460">LOWER</text><text x="324" y="340">MID</text></g><g fill="#ddd"><circle cx="133" cy="464" r="7"/><circle cx="161" cy="464" r="7"/><circle cx="189" cy="464" r="7"/></g><g fill="#1e90ff" opacity=".75"><path d="m518 276 8 14h-16Z"/><path d="m548 318 8 14h-16Z"/></g><path d="M162 445V352L225 309" stroke="#868686" stroke-dasharray="5 6" fill="none"/><g transform="translate(726,153)" font-family="Segoe UI, sans-serif"><text y="0" fill="#686868" font-size="9" letter-spacing="2">ROUND SCORE</text><text y="52" fill="#e6e6e6" font-size="41" font-weight="600">04 <tspan fill="#525252">:</tspan> 03</text><path d="M0 78H210" stroke="#333"/><text y="115" font-size="11" fill="#888">Time remaining</text><text y="148" font-size="25" fill="#ccc">00:42</text><text y="204" fill="#888" font-size="11">Players alive</text><text y="239" fill="#ddd" font-size="25">3 <tspan fill="#555" font-size="14">vs</tspan> 2</text><text y="315" fill="#777" font-size="10">WATCH THE LOWER ROUTE</text></g><g font-family="Consolas, monospace" font-size="10" fill="#656565"><text x="40" y="539">REPLAY 01:24 / 03:16</text><text x="756" y="539">SESSION 04 / 19 OCT</text></g>'),
    whiteboard: svg('<title>Physics lesson: projectile motion at a 35 degree angle</title><g font-family="Segoe UI, sans-serif"><text x="48" y="45" font-size="11" fill="#777" letter-spacing="2">PHYSICS / LESSON 12</text><text x="48" y="91" font-size="29" fill="#eee" font-weight="600">One launch. Two motions.</text><text x="48" y="121" font-size="13" fill="#888">Projectile motion · neglecting air resistance</text></g><g fill="none"><path d="M95 436H690M115 453V180" stroke="#777" stroke-width="1.5"/><path d="m680 430 10 6-10 6M109 190l6-10 6 10" stroke="#777"/><path d="M116 434Q365 -15 636 434" stroke="#bcbcbc" stroke-width="2" stroke-dasharray="6 7"/><path d="M116 434l110-80" stroke="#eee" stroke-width="2"/><path d="m209 355 17-1-6 16" stroke="#eee" stroke-width="2"/><path d="M163 435a48 48 0 0 0-9-28" stroke="#666"/><path d="M376 211V436M116 211H376" stroke="#414141" stroke-dasharray="3 7"/></g><g fill="#eee"><circle cx="116" cy="434" r="7"/><circle cx="375" cy="210" r="6"/></g><g font-family="Consolas, monospace" fill="#aaa" font-size="13"><text x="178" y="423">35°</text><text x="169" y="346">v₀ = 18 m/s</text><text x="391" y="216">apex</text><text x="680" y="462">x / m</text><text x="69" y="184">y / m</text><text x="340" y="463">15.5</text><text x="621" y="463">31.1</text></g><g transform="translate(732,201)" font-family="Segoe UI, sans-serif"><text y="0" font-size="10" fill="#777" letter-spacing="2">HORIZONTAL</text><text y="34" font-size="19" fill="#ddd">x = v₀ cos(θ) · t</text><text y="97" font-size="10" fill="#777" letter-spacing="2">VERTICAL</text><text y="131" font-size="19" fill="#ddd">y = v₀ sin(θ) · t</text><text x="31" y="159" font-size="19" fill="#ddd">− ½gt²</text><path d="M0 192H218" stroke="#333"/><text y="222" font-size="12" fill="#888">Same time. Independent axes.</text></g><text x="48" y="532" font-family="Consolas, monospace" font-size="10" fill="#666">12 / MECHANICS</text>'),
    code: '<div class="editor"><div class="editor-top"><span>◈ &nbsp; trajectory.ts</span><span>src / physics</span></div><div class="editor-main"><div class="line-numbers">1<br>2<br>3<br>4<br>5<br>6<br>7<br>8<br>9<br>10<br>11<br>12<br>13<br>14<br>15<br>16</div><pre><span class="code-comment">// A small step. A smooth trail.</span>\n<span class="code-blue">type</span> Point = { x: <span class="code-blue">number</span>; y: <span class="code-blue">number</span> };\n\n<span class="code-blue">export function</span> <span class="code-white">interpolate</span>(\n  from: Point, to: Point, t: <span class="code-blue">number</span>\n): Point {\n  <span class="code-blue">const</span> eased = t * t * (<span class="code-yellow">3</span> - <span class="code-yellow">2</span> * t);\n\n  <span class="code-blue">return</span> {\n    x: from.x + (to.x - from.x) * eased,\n    y: from.y + (to.y - from.y) * eased,\n  };\n}\n\n<span class="code-comment">// Ease in. Ease out. Keep it fluid.</span>\n<span class="code-white">interpolate</span>({ x: <span class="code-yellow">0</span>, y: <span class="code-yellow">0</span> }, target, <span class="code-yellow">0.5</span>);</pre></div><div class="editor-status"><span>main &nbsp; / &nbsp; 0 problems</span><span>TypeScript &nbsp; UTF-8 &nbsp; Ln 7, Col 18</span></div></div>',
    tactics: svg('<title>Football coaching board: create an overload on the right flank</title><g font-family="Segoe UI, sans-serif"><text x="40" y="44" font-size="11" fill="#777" letter-spacing="2">TOUCHLINE / MATCH PLAN</text><text x="40" y="85" font-size="27" fill="#eee" font-weight="600">Create the overload.</text><text x="40" y="112" font-size="12" fill="#777">4–3–3 · attacking phase · right flank</text></g><g fill="none" stroke="#454545" stroke-width="1.5"><path d="M72 150H703V506H72Z"/><path d="M388 150V506"/><circle cx="388" cy="328" r="48"/><path d="M72 220H171V436H72M72 272H111V384H72M703 220H604V436H703M703 272H664V384H703"/><path d="M171 284a56 56 0 0 1 0 88M604 284a56 56 0 0 0 0 88M72 302H58V354H72M703 302H717V354H703"/></g><g fill="#b6b6b6" stroke="#101010" stroke-width="3"><circle cx="100" cy="328" r="12"/><circle cx="228" cy="201" r="12"/><circle cx="228" cy="285" r="12"/><circle cx="228" cy="371" r="12"/><circle cx="228" cy="455" r="12"/><circle cx="360" cy="248" r="12"/><circle cx="330" cy="328" r="12"/><circle cx="360" cy="408" r="12"/><circle cx="489" cy="195" r="12"/><circle cx="510" cy="328" r="12"/><circle cx="489" cy="460" r="12"/></g><g fill="#222" stroke="#717171" stroke-width="1.5"><circle cx="674" cy="328" r="11"/><circle cx="583" cy="216" r="11"/><circle cx="583" cy="288" r="11"/><circle cx="583" cy="368" r="11"/><circle cx="583" cy="440" r="11"/><circle cx="462" cy="260" r="11"/><circle cx="440" cy="328" r="11"/><circle cx="462" cy="397" r="11"/></g><g font-family="Consolas, monospace" font-size="9" fill="#111" text-anchor="middle"><text x="228" y="458">2</text><text x="360" y="411">8</text><text x="489" y="463">7</text><text x="510" y="331">9</text></g><circle cx="244" cy="458" r="4" fill="#fff"/><path d="M242 452L345 413M372 414L476 455" stroke="#888" stroke-dasharray="4 6" fill="none"/><g transform="translate(770,185)" font-family="Segoe UI, sans-serif"><text fill="#777" font-size="10" letter-spacing="2">THE IDEA</text><text y="43" fill="#ddd" font-size="21">3 vs 2.</text><text y="71" fill="#aaa" font-size="12">Wide. Patient. Then go.</text><path d="M0 99H185" stroke="#333"/><text y="136" fill="#888" font-size="11">02 → 08 → 07</text><text y="167" fill="#888" font-size="11">Third-player run</text><text y="250" fill="#666" font-size="10">TRAINING / WEEK 07</text></g>')
  };
  const scripts = {
    stream:[{circle:[.523,.49,.082,.145]},{arrow:[.241,.756,.468,.67]},{underline:[.758,.9,.381]}],
    whiteboard:[{circle:[.373,.376,.061,.096]},{arrow:[.42,.51,.61,.73]},{underline:[.732,.94,.665]}],
    code:[{underline:[.225,.71,.451]},{circle:[.485,.645,.32,.08]},{arrow:[.73,.32,.57,.445]}],
    tactics:[{circle:[.427,.778,.115,.11]},{arrow:[.49,.798,.582,.855]},{arrow:[.275,.79,.45,.8]}]
  };
  let scene = 'stream';
  $('#scene-content').innerHTML = scenes.stream;
  function fitScene() {
    const svg=$('#scene-content>svg');
    if(svg)svg.setAttribute('viewBox',innerWidth<=600?'0 0 720 560':'0 0 1000 560');
  }
  fitScene();
  const scriptFor = name => {
    let steps=scripts[name];
    const sceneSvg=$('#scene-content>svg'),stage=$('#play-stage');
    if(sceneSvg){
      if(innerWidth<=600&&name==='stream')steps=[...steps.slice(0,2),{underline:[.04,.21,.171]}];
      if(innerWidth<=600&&name==='whiteboard')steps=[...steps.slice(0,2),{underline:[.1,.65,.87]}];
      const p=(x,y)=>svgPoint(sceneSvg,stage,x*1000/sceneSvg.viewBox.baseVal.width,y),origin=p(0,0),unit=p(1,1);
      steps=steps.map(step=>{
        if(step.circle){const [x,y,rx,ry]=step.circle;return {circle:[...p(x,y),rx*(unit[0]-origin[0]),ry*(unit[1]-origin[1])]};}
        if(step.arrow){const [x,y,xx,yy]=step.arrow;return {arrow:[...p(x,y),...p(xx,yy)]};}
        if(step.underline){const [x,xx,y]=step.underline;return {underline:[p(x,y)[0],p(xx,y)[0],p(x,y)[1]]};}
        return step;
      });
    }else if(name==='code'){
      const pre=$('.editor pre'),r=pre.getBoundingClientRect(),b=stage.getBoundingClientRect(),line=parseFloat(getComputedStyle(pre).lineHeight),font=parseFloat(getComputedStyle(pre).fontSize);
      const x=(r.x-b.x)/b.width,y=(r.y-b.y+line*7)/b.height;
      steps=[{underline:[x,x+font*29/b.width,y]},{circle:[x+font*18/b.width,(r.y-b.y+line*10.5)/b.height,font*19/b.width,line*2.2/b.height]},{arrow:[.83,.3,x+font*18/b.width,y-.02]}];
    }
    return [...steps,{wait:600},{loop:true}];
  };
  playground = Laser.stage($('#play-stage'), {look:neon,cursor,keyboard:true,ghost:scriptFor(scene),onChange:updatePlay});
  function updatePlay(s) {
    selectionState($('#play-stage'),s);
    $('#play-style').value = s.look.style;
    $('#play-width').value = s.look.width;
    $('#play-width-out').textContent = s.look.width+' px';
    $('#keep').setAttribute('aria-pressed',s.look.ink);
    $('#play-state').textContent = s.look.style.toUpperCase()+' / LASER '+(s.on?'ON':'OFF');
    syncColourAria($('#play-colours'),s.look.color);
  }
  function colourControls(host, stage) {
    colours.forEach(colour => {
      const b = document.createElement('button'); b.className = 'swatch'; b.style.setProperty('--colour',colour); b.dataset.colour=colour;
      b.setAttribute('aria-label','Colour '+colour); b.setAttribute('aria-pressed',stage.state.look.color===colour);
      b.onclick=()=>{stage.stop().setLook({color:colour});syncColourAria(host,colour);};host.append(b);
    });
    syncColourAria(host,stage.state.look.color);
  }
  function syncColourAria(host,colour) { host.querySelectorAll('button').forEach(b=>{const selected=b.dataset.colour===colour;b.classList.toggle('selected',selected);b.setAttribute('aria-pressed',selected);}); }
  colourControls($('#play-colours'),playground);
  $$('[data-scene]').forEach(b=>b.onclick=()=>{
    scene=b.dataset.scene;
    $$('[data-scene]').forEach(t=>{t.setAttribute('aria-selected',t===b);t.tabIndex=t===b?0:-1;});
    $('#play-stage').setAttribute('aria-labelledby',b.id);
    $('#scene-content').innerHTML=scenes[scene];
    fitScene();
    playground.stop().clear().setOn(true).play(scriptFor(scene));
  });
  $('#play-style').onchange=e=>{const style=e.target.value;playground.stop().setLook({style});};
  $('#play-width').oninput=e=>{const width=+e.target.value;playground.stop().setLook({width});};
  $('#keep').onclick=()=>playground.stop().setLook({ink:!playground.state.look.ink});
  $('#undo').onclick=()=>playground.stop().undo();
  $('#clear').onclick=()=>playground.stop().clear();
  $('#replay').onclick=()=>playground.stop().clear().setOn(true).play(scriptFor(scene));

  const sliderFields=[['width','Thickness',1,24,1,'px',1],['opacity','Opacity',10,100,1,'%',1],['fade','Fade time',.2,5,.1,'s',1000],['trail','Trail length',50,2000,50,'px',1],['smoothing','Smoothing',0,10,1,'',1]];
  sliderFields.forEach(([key,label,min,max,step,unit,scale])=>{
    const row=document.createElement('div');row.className='slider-field';
    row.innerHTML='<label for="setting-'+key+'">'+label+'</label><input type="range" id="setting-'+key+'" min="'+min+'" max="'+max+'" step="'+step+'"><output for="setting-'+key+'" id="value-'+key+'"></output>';
    $('#panel-sliders').append(row);
    row.querySelector('input').oninput=e=>{const value=+e.target.value*scale;preview.stop().setLook({[key]:value});};
  });
  const previewScript = [{path:[[.1,.67],[.23,.35],[.36,.66],[.52,.35],[.7,.61],[.88,.4]]},{wait:200},{loop:true}];
  preview=Laser.stage($('#panel-preview'),{look:{...neon,width:4,fade:2000},cursor,keyboard:true,ghost:previewScript,onChange:updatePanel});
  function updatePanel(s) {
    selectionState($('#panel-preview'),s);
    $('#panel-power').setAttribute('aria-checked',s.on);
    $('#panel-power').setAttribute('aria-label',s.on?'Laser on':'Laser off');
    $('#panel-status').textContent=s.on?'ON':'OFF';
    $('#colour-value').textContent=s.look.color.toUpperCase();
    $('#settings').style.setProperty('--accent',s.look.color);
    syncColourAria($('#panel-colours'),s.look.color);
    $$('#panel-styles button').forEach(b=>{b.classList.toggle('selected',b.dataset.style===s.look.style);b.setAttribute('aria-pressed',b.dataset.style===s.look.style);});
    $$('[data-preset]').forEach(b=>b.classList.toggle('selected',Laser.presets[s.preset]?.name===b.dataset.preset));
    sliderFields.forEach(([key,label,min,max,step,unit,scale])=>{const value=Math.round(s.look[key]/scale*10)/10;$('#setting-'+key).value=value;$('#value-'+key).textContent=value+(unit?' '+unit:'');});
  }
  colourControls($('#panel-colours'),preview);
  $('#panel-power').onclick=()=>preview.stop().toggle();
  $$('[data-preset]').forEach(b=>b.onclick=()=>{preview.stop().clear().setOn(true).usePreset(b.dataset.preset).play(previewScript);});
  $$('#panel-styles button').forEach(b=>b.onclick=()=>preview.stop().setLook({style:b.dataset.style}));
  $('#reset-panel').onclick=()=>preview.stop().clear().setOn(true).setLook({...neon,width:4,fade:2000}).play(previewScript);
  $$('[data-panel]').forEach(b=>b.onclick=()=>{
    $$('[data-panel]').forEach(t=>{t.setAttribute('aria-selected',t===b);t.tabIndex=t===b?0:-1;});
    $$('.panel-content').forEach(p=>p.hidden=p.id!=='panel-'+b.dataset.panel);
  });
  // Standard arrow-key navigation for both tab strips.
  $$('[role=tablist]').forEach(list=>list.addEventListener('keydown',e=>{
    if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;
    const tabs=[...list.querySelectorAll('[role=tab]')],i=tabs.indexOf(document.activeElement);
    if(i<0)return;e.preventDefault();
    const next=e.key==='Home'?0:e.key==='End'?tabs.length-1:(i+(e.key==='ArrowRight'?1:-1)+tabs.length)%tabs.length;
    tabs[next].focus();tabs[next].click();
  }));
  $$('[role=tab]').forEach(tab=>tab.tabIndex=tab.getAttribute('aria-selected')==='true'?0:-1);
  $$('.store-link').forEach(link=>link.setAttribute('aria-label','Get it on the Microsoft Store — US$2.99 · coming soon'));
  let resizeTimer;
  window.addEventListener('resize',()=>{clearTimeout(resizeTimer);resizeTimer=setTimeout(()=>{fitScene();if(hero.state.ghosting)hero.stop().clear().play(heroScript());if(playground.state.ghosting)playground.stop().clear().play(scriptFor(scene));},200);});
})();

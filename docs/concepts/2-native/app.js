(function(){
 'use strict';
 const $=s=>document.querySelector(s), $$=s=>Array.from(document.querySelectorAll(s));
 const desktop=$('#desktop'), shell=$('#desktop-shell'), panel=$('#settings-panel'), dock=$('#panel-dock');
 let main, preview, activeApp='slides', slide=6, flyout=false, previewTimer;
 const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
 const settings=[['width','Thickness',1,24,1,'px',1],['opacity','Opacity',10,100,1,'%',1],['fade','Fade time',.3,6,.1,'s',1000],['trail','Trail length',100,2000,50,'px',1],['smoothing','Smoothing',0,10,1,'',1]];
 const narrow=matchMedia('(max-width:600px)').matches;
 const textRange=document.createRange();textRange.selectNodeContents($('#metric-value'));
 const numberRect=textRange.getBoundingClientRect(),desktopRect=desktop.getBoundingClientRect();
 const ring=[(numberRect.left+numberRect.width/2-desktopRect.left)/desktopRect.width,(numberRect.top+numberRect.height/2-desktopRect.top)/desktopRect.height,(numberRect.width/2+11)/desktopRect.width,(numberRect.height/2+5)/desktopRect.height];
 const ghost=[{wait:100},{on:true},{do:()=>{$('#coach').textContent='Alt+L. A little laser for everything on your screen.';}},{move:[ring[0],ring[1]-.08]},{circle:ring},{wait:140},{arrow:narrow?[.5,.72,.79,.685]:[.55,.68,.79,.57]},{wait:220},{underline:narrow?[.12,.62,.49]:[.125,.45,.407]},{wait:700},{on:false},{do:()=>{coach();$('#ghost-arrow').style.left=(narrow?62:45)+'%';$('#ghost-arrow').style.top=(narrow?49:40.7)+'%';} }];
 function coach(){const el=$('#coach');el.innerHTML='Press <kbd>L</kbd> (or <kbd>Alt+L</kbd>) — or click the tray icon';}
 function sync(s){
   document.body.classList.toggle('laser-is-on',s.on);
   const status=s.on?'Laser on':'Laser off';
   $('#laser-status span').textContent=status; $('.panel-status span').textContent=status;
   $('#power-toggle').setAttribute('aria-checked',s.on);
   document.documentElement.style.setProperty('--accent',s.look.color);panel.style.setProperty('--accent',s.look.color);
   $('#colour-value').textContent=s.look.color.toUpperCase();
   $$('[data-style]').forEach(b=>{const selected=b.dataset.style===s.look.style;b.setAttribute('aria-checked',selected);b.tabIndex=selected?0:-1;});$('#keep-ink').checked=s.look.ink;
   $$('.swatches button').forEach(b=>b.setAttribute('aria-pressed',b.dataset.color===s.look.color));
   $$('.presets button').forEach(b=>b.classList.toggle('chosen',Laser.presets[s.preset]?.name===b.dataset.preset));
   settings.forEach(([key,label,min,max,step,unit,factor])=>{const input=$('#range-'+key);if(!input)return;input.value=s.look[key]/factor;input.style.setProperty('--progress',((Number(input.value)-min)/(max-min)*100)+'%');$('#value-'+key).textContent=(factor===1000?(s.look[key]/factor).toFixed(1):s.look[key])+(unit?' '+unit:'');});
   if(preview)preview.setLook(s.look);
 }
 main=Laser.stage(desktop,{on:false,keyboard:true,cursor:NativeCursor,look:{fade:1550,trail:680},onChange:sync});
 preview=Laser.stage($('#preview'),{on:true,cursor:NativeCursor,keyboard:false});
 function playPreview(){preview.stop().clear().setLook(main.state.look).play([{on:true},{move:[.12,.64]},{path:[[.15,.65],[.31,.72],[.48,.59],[.64,.62],[.82,.46]]}]);}
 function updatePreview(){clearTimeout(previewTimer);previewTimer=setTimeout(playPreview,70);}
 function takeControl(){main.stop();coach();$('#ghost-arrow').style.display='none';}
 function applyLook(look){takeControl();main.setLook(look);updatePreview();}
 ['#ff2a2a','#ff8a00','#ffd400','#2ecc40','#1e90ff','#d62aff'].forEach(color=>{const b=document.createElement('button');b.style.setProperty('--swatch',color);b.dataset.color=color;b.setAttribute('aria-label','Use '+color);b.setAttribute('aria-pressed',color===main.state.look.color);b.onclick=()=>applyLook({color});$('.swatches').append(b);});
 settings.forEach(([key,label,min,max,step,unit,factor])=>{const row=document.createElement('label');row.className='slider-row';row.innerHTML='<span>'+label+'</span><input type="range" id="range-'+key+'" min="'+min+'" max="'+max+'" step="'+step+'" aria-label="'+label+'"><output id="value-'+key+'"></output>';$('.sliders').append(row);row.querySelector('input').oninput=e=>applyLook({[key]:Number(e.target.value)*factor});});
 sync(main.state);playPreview();
 $$( '[data-preset]' ).forEach(b=>b.onclick=()=>{takeControl();main.usePreset(b.dataset.preset);updatePreview();});
 $$('[data-style]').forEach(b=>{
   b.onclick=()=>applyLook({style:b.dataset.style});
   b.onkeydown=e=>{const buttons=$$('[data-style]'),i=buttons.indexOf(b);let next;
     if(e.key==='ArrowRight'||e.key==='ArrowDown')next=(i+1)%3;
     else if(e.key==='ArrowLeft'||e.key==='ArrowUp')next=(i+2)%3;
     else if(e.key==='Home')next=0;else if(e.key==='End')next=2;
     if(next!==undefined){e.preventDefault();buttons[next].click();buttons[next].focus();}
   };
 });
 $('#keep-ink').onchange=e=>applyLook({ink:e.target.checked});
 $('#clear-strokes').onclick=()=>{takeControl();main.clear();preview.clear();};
 $('#power-toggle').onclick=()=>{takeControl();main.toggle();};
 $$('.panel-tabs button').forEach(b=>b.onclick=()=>{$$('.panel-tabs button').forEach(t=>t.setAttribute('aria-selected',t===b));$$('.tab-content').forEach(t=>t.classList.toggle('hidden',t.id!=='tab-'+b.dataset.tab));if(b.dataset.tab==='pen')updatePreview();});
 function showFlyout(open){flyout=open;$('#tray-button').setAttribute('aria-expanded',open);if(open){takeControl();shell.append(panel);main.setOn(true);updatePreview();}else dock.append(panel);}
 $('#tray-button').onclick=()=>showFlyout(!flyout);
 $('.panel-close').onclick=()=>showFlyout(false);
 document.addEventListener('pointerdown',e=>{if(flyout&&!panel.contains(e.target)&&!$('#tray-button').contains(e.target))showFlyout(false);});
 $$('.task-app').forEach(b=>b.onclick=()=>{takeControl();activeApp=b.dataset.app;$$('.task-app').forEach(x=>x.classList.toggle('active',x===b));$$('.app-window').forEach(w=>w.classList.toggle('active',w.dataset.window===activeApp));if(flyout)showFlyout(false);});
 // The shared engine handles mouse gestures directly. Hover only moves the cursor;
 // a real left-button press starts a stroke. Hide the scripted arrow on takeover.
 desktop.addEventListener('pointermove',()=>{$('#ghost-arrow').style.display='none';});
 // Touch must switch on before the shared engine handles the first press.
 shell.addEventListener('pointerdown',e=>{if(desktop.contains(e.target)&&e.pointerType==='touch'&&!main.state.on){takeControl();main.setOn(true);}},{capture:true});
 desktop.addEventListener('contextmenu',()=>{if(main.state.on&&activeApp==='slides'){slide=slide===12?1:slide+1;$('#slide-number').textContent=String(slide).padStart(2,'0')+' / 12';$('#slide-count').textContent=slide;$('.slide h2').innerHTML=slide%2?'Small changes.<br>Lasting impact.':'Less friction.<br>More momentum.';$('#metric-value').innerHTML=(slide%2?'76':'68')+'<span>%</span>';}});
 // The engine owns the overlay; route wheel input to the app below it.
 desktop.addEventListener('wheel',e=>{if(e.ctrlKey){e.preventDefault();applyLook({width:Math.max(1,Math.min(24,main.state.look.width+(e.deltaY>0?-1:1)))});}else if(activeApp==='code'){e.preventDefault();$('#code-scroll').scrollTop+=e.deltaY;}else if(activeApp==='sheet'){e.preventDefault();$('.sheet-content').scrollTop+=e.deltaY;}},{passive:false});
 document.addEventListener('keydown',e=>{
   if(e.defaultPrevented||e.target.closest('input,select,textarea')||e.metaKey)return;
   const key=e.key.toLowerCase();let fn;
   if(key==='l'&&!e.ctrlKey)fn=()=>main.toggle();
   else if(key==='escape')fn=()=>{main.setOn(false);if(flyout)showFlyout(false);};
   else if(e.ctrlKey&&!e.altKey&&key==='z')fn=()=>main.undo();
   else if(e.ctrlKey&&!e.altKey&&key==='y')fn=()=>main.redo();
   else if(e.altKey&&key==='x')fn=()=>main.clear();
   else if(e.altKey&&key==='k')fn=()=>main.setLook({ink:!main.state.look.ink});
   else if(e.altKey&&(key==='['||key===']'))fn=()=>main.usePreset((main.state.preset+(key===']'?1:2)+3)%3);
   if(fn){e.preventDefault();takeControl();fn();updatePreview();}
 });
 $('#replay').onclick=()=>{if(flyout)showFlyout(false);activeApp='slides';$('#ghost-arrow').style.display='';$$('.task-app').forEach(b=>b.classList.toggle('active',b.dataset.app==='slides'));$$('.app-window').forEach(w=>w.classList.toggle('active',w.dataset.window==='slides'));main.stop().clear().usePreset('Laser').setLook({fade:1550,trail:680}).setOn(false).play(ghost);};
 if(reduced){main.play([{on:true},{circle:ring},{arrow:[.55,.68,.79,.57]},{on:false}]);coach();}
 else main.play(ghost);
 window.addEventListener('error',e=>{document.documentElement.dataset.error=e.message;});
 // Opt-in, in-folder headless inspection. No effect on the normal page.
 if(new URLSearchParams(location.search).has('check'))setTimeout(()=>{
   const results=[];const assert=(ok,name)=>results.push((ok?'PASS ':'FAIL ')+name);
   main.stop();main.setOn(true);assert(main.state.on,'power');main.setOn(false);assert(!main.state.on,'off');
   $('[data-app="code"]').click();assert($('[data-window="code"]').classList.contains('active'),'taskbar');
   $('#tray-button').click();assert(panel.parentElement===shell,'tray flyout');
   $('[data-preset="Highlighter"]').click();assert(main.state.look.width===18&&main.state.look.color==='#ffd400','preset');
   $('[data-style="neon"]').click();assert(main.state.look.style==='neon','style');
   $('#range-width').value='7';$('#range-width').dispatchEvent(new Event('input'));assert(main.state.look.width===7,'slider');
   $('.panel-close').click();assert(panel.parentElement===dock,'flyout returns');
   assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow');
   const out=document.createElement('pre');out.id='check-results';out.textContent=results.join('\n');document.body.append(out);document.title=results.some(x=>x.startsWith('FAIL'))?'FAIL Native':'PASS Native';
 },600);
})();

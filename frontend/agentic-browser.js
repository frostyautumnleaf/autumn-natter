(function () {
  'use strict';

  var style = document.createElement('style');
  style.textContent = `
    .agentic-browser-panel { position: fixed; z-index: 96; top: 12px; right: 12px; bottom: 12px;
      width: min(900px, calc(100vw - 24px)); display: flex; flex-direction: column;
      overflow: hidden; border: 1px solid var(--line-strong); border-radius: 14px;
      background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
    .agentic-browser-panel.hidden { display: none; }
    .agentic-browser-head { display: flex; align-items: center; gap: 6px; padding: 8px;
      border-bottom: 1px solid var(--line); background: var(--surface-2); }
    .agentic-browser-label { flex: 0 0 auto; padding: 0 5px; color: var(--text-2); font-size: 12px; font-weight: 650; }
    .agentic-browser-address { min-width: 0; flex: 1 1 auto; height: 30px; padding: 0 9px;
      border: 1px solid var(--line); border-radius: 7px; background: var(--surface); color: var(--text);
      font: 12px ui-monospace, monospace; }
    .agentic-browser-action { flex: 0 0 auto; width: 30px; height: 30px; border: 1px solid var(--line);
      border-radius: 7px; background: var(--surface); color: var(--text); cursor: pointer; }
    .agentic-browser-action:hover { border-color: var(--accent-line); background: var(--hover); }
    .agentic-browser-tabs { display: flex; gap: 4px; overflow-x: auto; padding: 6px 8px;
      border-bottom: 1px solid var(--line); background: var(--surface); }
    .agentic-browser-tab { flex: 0 0 auto; max-width: 210px; overflow: hidden; text-overflow: ellipsis;
      white-space: nowrap; padding: 4px 9px; border: 1px solid transparent; border-radius: 7px;
      background: transparent; color: var(--text-2); font-size: 11.5px; cursor: pointer; }
    .agentic-browser-tab.active { border-color: var(--line); background: var(--surface-2); color: var(--ink); }
    .agentic-browser-tab-wrap { display: flex; flex: 0 0 auto; align-items: center; }
    .agentic-browser-tab-close { width: 20px; height: 25px; margin-left: -4px; border: 0; border-radius: 5px;
      background: transparent; color: var(--text-2); cursor: pointer; }
    .agentic-browser-tab-close:hover { background: var(--hover); color: var(--ink); }
    .agentic-browser-frame { width: 100%; min-height: 0; flex: 1 1 auto; border: 0; background: #fff; }
    .agentic-browser-reader { max-height: 36vh; overflow: auto; flex: 0 0 auto; border-top: 1px solid var(--line);
      background: var(--surface); color: var(--text); }
    .agentic-browser-reader summary { padding: 7px 10px; color: var(--text-2); font-size: 11.5px; cursor: pointer; }
    .agentic-browser-reader pre { margin: 0; padding: 4px 10px 10px; white-space: pre-wrap; overflow-wrap: anywhere;
      color: var(--text); font-size: 12px; line-height: 1.5; }
    .agentic-browser-links { display: flex; flex-wrap: wrap; gap: 6px; padding: 0 10px 9px; }
    .agentic-browser-links a { color: var(--ink); font-size: 11px; }
    .agentic-browser-empty { display: flex; align-items: center; justify-content: center; flex: 1 1 auto;
      padding: 32px; color: var(--dim); text-align: center; font-size: 13px; }
    .agentic-browser-approval { position: absolute; z-index: 2; inset: 0; display: flex; flex-direction: column;
      gap: 10px; padding: 20px; background: color-mix(in srgb, var(--surface) 96%, transparent);
      color: var(--text); }
    .agentic-browser-approval.hidden { display: none; }
    .agentic-browser-approval strong { font-size: 14px; }
    .agentic-browser-approval pre { min-height: 0; flex: 1; overflow: auto; margin: 0; padding: 12px;
      border: 1px solid var(--line); border-radius: 8px; background: var(--surface-2); color: var(--text);
      white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.5 ui-monospace, monospace; }
    .agentic-browser-approval-actions { display: flex; justify-content: flex-end; gap: 8px; }
    .agentic-browser-approval-actions button { min-width: 84px; padding: 7px 12px; border: 1px solid var(--line);
      border-radius: 7px; background: var(--surface-2); color: var(--text); cursor: pointer; }
    .agentic-browser-approval-actions [data-browser-approve="yes"] { border-color: var(--accent-line); background: var(--accent); color: var(--on-accent); }
    @media (max-width: 700px) { .agentic-browser-panel { inset: 8px; width: auto; } }
    @media (max-width: 540px) {
      .agentic-browser-label { display: none; }
      .agentic-browser-head { gap: 4px; padding: 6px; }
      .agentic-browser-action { width: 26px; height: 28px; }
    }
  `;
  document.head.appendChild(style);

  var root = document.createElement('section');
  root.className = 'agentic-browser-panel hidden';
  root.setAttribute('aria-label', 'Browser use panel');
  root.innerHTML = '<div class="agentic-browser-head">' +
    '<span class="agentic-browser-label">Browser use</span>' +
    '<input class="agentic-browser-address" type="url" aria-label="Browser address" placeholder="Enter a public page address">' +
    '<button class="agentic-browser-action" type="button" data-browser-nav="back" title="Back" aria-label="Back">←</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-nav="forward" title="Forward" aria-label="Forward">→</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-nav="reload" title="Reload" aria-label="Reload">↻</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-newtab title="New tab" aria-label="New tab">＋</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-go title="Open address in panel" aria-label="Open address in panel">↵</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-open-external title="Open this page in a new browser tab" aria-label="Open this page in a new browser tab">↗</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-close title="Close browser panel" aria-label="Close browser panel">×</button>' +
    '</div><div class="agentic-browser-tabs" role="tablist"></div>' +
    '<div class="agentic-browser-empty">Enable Browser use, then enter a public web address or ask the model to open one.</div>' +
    '<iframe class="agentic-browser-frame hidden" title="Isolated page preview" sandbox="allow-scripts" referrerpolicy="no-referrer"></iframe>' +
    '<details class="agentic-browser-reader"><summary>Page text read by the model</summary><div class="agentic-browser-links"></div><pre></pre></details>' +
    '<div class="agentic-browser-approval hidden" role="dialog" aria-modal="true" aria-label="Approve browser action"><strong>Review browser action</strong><pre></pre><div class="agentic-browser-approval-actions"><button type="button" data-browser-approve="no">Deny</button><button type="button" data-browser-approve="yes">Allow</button></div></div>';
  document.body.appendChild(root);

  var address = root.querySelector('.agentic-browser-address');
  var tabs = root.querySelector('.agentic-browser-tabs');
  var frame = root.querySelector('.agentic-browser-frame');
  var empty = root.querySelector('.agentic-browser-empty');
  var readerText = root.querySelector('.agentic-browser-reader pre');
  var readerLinks = root.querySelector('.agentic-browser-links');
  var approval = root.querySelector('.agentic-browser-approval');
  var approvalText = approval.querySelector('pre');
  var approvalId = '';
  var currentKey = '';
  var currentUrl = '';
  var newTab = false;
  var lastActiveUrl = '';
  var latest = null;
  var currentPending = null;
  var pagePort = null;
  var sentActions = new Set();

  function open() {
    root.classList.remove('hidden');
    if (latest) { render(latest, false); }
  }
  function denyApproval() {
    if (!approvalId) { return; }
    if (pagePort) { pagePort.postMessage({ type: 'autumn-browser-approval', id: approvalId, approved: false }); }
    approvalId = '';
    approvalText.textContent = '';
    approval.classList.add('hidden');
  }
  function close() {
    denyApproval();
    if (currentPending) {
      answerBrowserAction(currentPending.id, { ok: false, error: 'The user closed the browser panel and stopped this action.' });
      currentPending = null;
    }
    root.classList.add('hidden');
  }
  function validUrl(value) {
    try {
      var parsed = new URL(value);
      return (parsed.protocol === 'http:' || parsed.protocol === 'https:') ? parsed.href : '';
    } catch (_) { return ''; }
  }
  function render(state, autoOpen) {
    var browser = state.browser || {};
    var list = Array.isArray(browser.tabs) ? browser.tabs : [];
    var active = Number(browser.active) || 0;
    currentPending = browser.pending_action && typeof browser.pending_action.id === 'string'
      ? browser.pending_action : null;
    if (!currentPending && approvalId) { denyApproval(); }
    if (currentPending) { open(); }
    tabs.replaceChildren();
    list.forEach(function (tab, index) {
      var url = validUrl((tab && tab.url) || '');
      if (!url) { return; }
      var wrap = document.createElement('span');
      wrap.className = 'agentic-browser-tab-wrap';
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'agentic-browser-tab' + (active === index + 1 ? ' active' : '');
      button.setAttribute('role', 'tab');
      button.textContent = (index + 1) + ' · ' + url;
      button.title = url;
      button.addEventListener('click', function () {
        window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
          detail: { path: 'browser_tab', body: { tab: index + 1 } }
        }));
      });
      var closeTab = document.createElement('button');
      closeTab.type = 'button';
      closeTab.className = 'agentic-browser-tab-close';
      closeTab.textContent = '×';
      closeTab.title = 'Close tab ' + (index + 1);
      closeTab.setAttribute('aria-label', closeTab.title);
      closeTab.addEventListener('click', function (event) {
        event.stopPropagation();
        window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
          detail: { path: 'browser_close_tab', body: { tab: index + 1 } }
        }));
      });
      wrap.appendChild(button);
      wrap.appendChild(closeTab);
      tabs.appendChild(wrap);
    });
    var page = list[active - 1];
    var nextUrl = validUrl((page && page.url) || '');
    readerText.textContent = (page && typeof page.text === 'string') ? page.text : '';
    readerLinks.replaceChildren();
    ((page && Array.isArray(page.links)) ? page.links : []).forEach(function (link) {
      var href = validUrl((link && link.url) || '');
      if (!href) { return; }
      var anchor = document.createElement('a');
      anchor.href = href;
      anchor.target = '_blank';
      anchor.rel = 'noopener noreferrer';
      anchor.textContent = ((link && link.label) || href).slice(0, 160);
      readerLinks.appendChild(anchor);
    });
    if (!nextUrl) {
      if (document.activeElement !== address) { address.value = ''; }
      currentKey = '';
      currentUrl = '';
      frame.srcdoc = '';
      frame.classList.add('hidden');
      empty.classList.remove('hidden');
      empty.textContent = 'Enable Browser use, then enter a public web address or ask the model to open one.';
      lastActiveUrl = '';
      return;
    }
    if (document.activeElement !== address) { address.value = nextUrl; }
    var nextKey = [active, nextUrl, page && page.revision || 0].join('|');
    if (nextKey !== currentKey) {
      setPage(nextUrl, active, nextKey);
      if (autoOpen) { open(); }
    } else {
      sendPendingAction();
    }
    lastActiveUrl = nextUrl;
  }
  function safeDocument(html, url) {
    var source = String(html || '');
    source = source.replace(/<script\b[^>]*>[\s\S]*?<\/script\s*>/gi, '')
      .replace(/<form\b([^>]*)>/gi, function (_, attrs) {
        return '<form' + attrs.replace(/\s+(?:action|method|target|enctype|accept-charset)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, '') + '>';
      })
      .replace(/\s+(?:formaction|action|method|target)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, '')
      .replace(/<input\b(?=[^>]*\btype\s*=\s*(?:"file"|'file'|file))[^>]*>/gi, '<input disabled aria-label="File uploads are unavailable in the safe browser preview">')
      .replace(/<(?:iframe|object)\b[^>]*>[\s\S]*?<\/(?:iframe|object)\s*>/gi, '')
      .replace(/<embed\b[^>]*>/gi, '')
      .replace(/<base\b[^>]*>/gi, '')
      .replace(/<meta\b[^>]*http-equiv\s*=\s*(["']?)(?:refresh|content-security-policy)\1[^>]*>/gi, '')
      .replace(/\s+on[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, '');
    var safeUrl = String(url || '').replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
    var nonce = '';
    try { nonce = Array.from(crypto.getRandomValues(new Uint8Array(16))).map(function (n) { return n.toString(16).padStart(2, '0'); }).join(''); }
    catch (_) { nonce = String(Date.now()) + Math.random().toString(16).slice(2); }
    var policy = "default-src 'none'; img-src https: data: blob:; style-src https: 'unsafe-inline'; font-src https: data:; media-src https: blob:; script-src 'nonce-" + nonce + "' 'unsafe-eval'; connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; base-uri https: http:";
    var agent = `<script nonce="${nonce}">
      (function(){
        var ids=new WeakMap(), refs=new Map(), next=1, approvals=new Map(), bridge=null;
        function clean(s,max){s=String(s||'').replace(/\\s+/g,' ').trim();return s.length>(max||160)?s.slice(0,(max||160)-1)+'…':s}
        function visible(el){var r=el.getBoundingClientRect(),s=getComputedStyle(el);return r.width>1&&r.height>1&&s.display!=='none'&&s.visibility!=='hidden'&&s.opacity!=='0'}
        function role(el){var r=el.getAttribute('role');if(r)return r;var t=el.tagName.toLowerCase();if(t==='a'&&el.href)return 'link';if(t==='button')return 'button';if(t==='select')return 'combobox';if(t==='textarea'||t==='input')return el.type==='checkbox'?'checkbox':el.type==='radio'?'radio':'textbox';if(el.isContentEditable)return 'textbox';return 'element'}
        function name(el){return clean(el.getAttribute('aria-label')||el.getAttribute('placeholder')||el.innerText||el.textContent||el.getAttribute('title')||el.name||el.tagName.toLowerCase(),160)}
        function scan(){var out=[];var all=document.querySelectorAll('a[href],button,input:not([type=hidden]),textarea,select,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[tabindex]');for(var el of all){if(!visible(el))continue;var id=ids.get(el);if(!id){id='r'+next++;ids.set(el,id);refs.set(id,el)}out.push({ref:id,role:role(el),name:name(el),href:el.tagName==='A'?el.href:'',type:el.type||'',disabled:!!el.disabled});if(out.length>=400)break}return out}
        function text(limit){return String(document.body&&document.body.innerText||'').replace(/\\r/g,'').split(/\\n+/).map(function(line){return clean(line,1200)}).filter(Boolean).join('\\n').slice(0,limit||24000)}
        function send(id,result){if(bridge)bridge.postMessage({type:'autumn-browser-result',id:id,result:result})}
        function ask(id,message){return new Promise(function(resolve){var timer=setTimeout(function(){approvals.delete(id);resolve(false)},60000);approvals.set(id,function(answer){clearTimeout(timer);approvals.delete(id);resolve(answer===true)});if(bridge)bridge.postMessage({type:'autumn-browser-confirm',id:id,message:message})})}
        function point(el,x,y){if(el){var r=el.getBoundingClientRect();return [r.left+r.width/2,r.top+r.height/2]}return [Number(x)||0,Number(y)||0]}
        function byRef(ref){var el=refs.get(String(ref||''));return el&&el.isConnected?el:null}
        function formSubmit(el){if(!el)return false;var form=el.form||el.closest&&el.closest('form');if(!form)return false;var t=(el.type||'').toLowerCase();return t==='submit'||el.tagName==='BUTTON'||el.getAttribute('role')==='button'}
        function xml(s){return String(s||'').replace(/[\\u0000-\\u0008\\u000b\\u000c\\u000e-\\u001f]/g,' ').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;')}
        function pageShot(){return new Promise(function(resolve){var w=Math.max(1,Math.min(1600,innerWidth)),h=Math.max(1,Math.min(1200,innerHeight)),sx=innerWidth/w,sy=innerHeight/h,out=['<svg xmlns="http://www.w3.org/2000/svg" width="'+w+'" height="'+h+'" viewBox="0 0 '+w+' '+h+'">'];out.push('<rect width="100%" height="100%" fill="'+xml(getComputedStyle(document.body).backgroundColor||'#fff')+'"/>');
          for(var el of document.querySelectorAll('body *')){var r=el.getBoundingClientRect();if(r.width<1||r.height<1||r.right<0||r.bottom<0||r.left>w*sx||r.top>h*sy)continue;var s=getComputedStyle(el),x=Math.max(0,r.left/sx),y=Math.max(0,r.top/sy),rw=Math.min(w-x,r.width/sx),rh=Math.min(h-y,r.height/sy),bg=s.backgroundColor;if(bg&&bg!=='rgba(0, 0, 0, 0)'&&bg!=='transparent')out.push('<rect x="'+x+'" y="'+y+'" width="'+rw+'" height="'+rh+'" fill="'+xml(bg)+'"/>');var bw=parseFloat(s.borderBottomWidth)||0;if(bw>0&&s.borderBottomColor)out.push('<path d="M'+x+' '+(y+rh)+'h'+rw+'" stroke="'+xml(s.borderBottomColor)+'" stroke-width="'+Math.max(.5,bw/((sx+sy)/2))+'"/>');}
          var walk=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT),node,count=0;while((node=walk.nextNode())&&count<1200){var raw=String(node.nodeValue||'').replace(/\\s+/g,' ').trim();if(!raw)continue;var parent=node.parentElement;if(!parent||/^(SCRIPT|STYLE|NOSCRIPT|TITLE)$/.test(parent.tagName))continue;var range=document.createRange();range.selectNodeContents(node);var rect=range.getBoundingClientRect();if(rect.width<1||rect.height<1||rect.right<0||rect.bottom<0||rect.left>w*sx||rect.top>h*sy)continue;var st=getComputedStyle(parent),size=Math.max(7,Math.min(30,parseFloat(st.fontSize)||14))/((sx+sy)/2),chars=Math.max(1,Math.floor(rect.width/Math.max(4,size*.55))),piece=raw.slice(0,Math.min(raw.length,chars*2)),tx=Math.max(0,rect.left/sx),ty=Math.min(h-1,Math.max(size,rect.top/sy+size));out.push('<text x="'+tx+'" y="'+ty+'" fill="'+xml(st.color||'#111')+'" font-family="'+xml(st.fontFamily||'sans-serif')+'" font-size="'+size+'">'+xml(piece)+'</text>');count++;}
          out.push('</svg>');var blob=new Blob([out.join('')],{type:'image/svg+xml'}),url=URL.createObjectURL(blob),img=new Image();img.onload=function(){try{var canvas=document.createElement('canvas');canvas.width=w;canvas.height=h;var ctx=canvas.getContext('2d');ctx.fillStyle='#fff';ctx.fillRect(0,0,w,h);ctx.drawImage(img,0,0,w,h);resolve({width:w,height:h,data:canvas.toDataURL('image/png')})}catch(e){resolve(null)}URL.revokeObjectURL(url)};img.onerror=function(){URL.revokeObjectURL(url);resolve(null)};img.src=url})}
        async function run(msg){var a=msg.action||{},op=a.action||a.op||'',el=byRef(a.ref),controls=scan();if(!el&&(op==='click'||op==='move'||op==='drag')&&Number.isFinite(Number(a.x))&&Number.isFinite(Number(a.y)))el=document.elementFromPoint(Number(a.x),Number(a.y));
          if(op==='snapshot'){var scope=el||document.body,items=scope===document.body?controls:controls.filter(function(item){var child=byRef(item.ref);return child&&scope.contains(child)});return {ok:true,title:document.title,text:clean(scope.innerText||scope.textContent||'',a.full_page?100000:24000),links:items}}
          if(op==='find'){var q=clean(a.query||a.text,200).toLowerCase();var found=controls.filter(function(x){return (x.name+' '+x.role).toLowerCase().includes(q)});var snippets=(document.body&&document.body.innerText||'').split(/\\n+/).filter(function(x){return x.toLowerCase().includes(q)}).slice(0,10);return {ok:true,title:document.title,text:snippets.join('\\n'),links:found}}
          if(op==='screenshot'){var shot=await pageShot();return shot?{ok:true,title:document.title,text:'Visible browser page screenshot.',image:shot.data,width:shot.width,height:shot.height}:{ok:false,error:'The browser page screenshot could not be captured.'}}
          if(op==='evaluate'){if(!await ask(msg.id,'Run this JavaScript in the current page?\\n\\n'+String(a.script||'')))return {ok:false,error:'The user declined the page script.'};try{var value=await (new Function('return (async function(){'+String(a.script||'')+'}).call(null)'))();return {ok:true,title:document.title,text:typeof value==='string'?value:JSON.stringify(value)}}catch(e){return {ok:false,error:'Page script failed: '+clean(e&&e.message,500)}}}
          if(op==='click'&&el&&el.tagName==='A'&&el.href){if(new URL(el.href).host!==location.host&&!await ask(msg.id,'Open this link in the browser panel?\\n'+el.href))return {ok:false,error:'The user declined to open the link.'};return {ok:true,navigate:el.href,title:document.title}}
          if(op==='click'&&el&&(/\\b(buy|purchase|pay|place order|send|delete|remove|sign in|log in|subscribe|transfer|publish|submit)\\b/i.test(name(el)))&&!await ask(msg.id,'Allow this page action?\\n'+name(el)))return {ok:false,error:'The user declined this page action.'};
          if(op==='click'&&formSubmit(el)){if(!await ask(msg.id,'Submit this page form?\\n'+clean(name(el),160)))return {ok:false,error:'The user declined to submit the form.'};return {ok:false,error:'The safe page preview blocks form submission. Complete this step in your own browser, then ask the model to continue.'}}
          if(op==='click'&&!a.ref&&el&&name(el)==='element'&&!await ask(msg.id,'Allow a click at page coordinates '+Math.round(Number(a.x)||0)+', '+Math.round(Number(a.y)||0)+'?'))return {ok:false,error:'The user declined this unlabelled page click.'};
          if(op==='type'){if(!el)return {ok:false,error:'That ref is not on the page. Take a fresh snapshot.'};if((el.type||'').toLowerCase()==='password'||/password|passcode|card number|security code/i.test(name(el)))return {ok:false,error:'The browser will not type into password or payment fields.'};if(!('value'in el)&&!el.isContentEditable)return {ok:false,error:'That ref is not a text field.'};if(!await ask(msg.id,'Type '+String(a.text||'').length+' characters into '+clean(name(el),160)+'?'))return {ok:false,error:'The user declined to type into this field.'};if(el.isContentEditable)el.textContent=String(a.text||'');else{var proto=el.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;var setter=Object.getOwnPropertyDescriptor(proto,'value').set;setter.call(el,String(a.text||''))}el.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:String(a.text||'')}));el.dispatchEvent(new Event('change',{bubbles:true}));return {ok:true,title:document.title,text:'Typed into '+name(el)+(a.submit?' (form submission is blocked in the safe preview).':'.')}}
          if(op==='select'){if(!el)return {ok:false,error:'That ref is not on the page. Take a fresh snapshot.'};var value=String(a.value||'');if(el.tagName==='SELECT'){var option=Array.from(el.options).find(function(o){return o.label.toLowerCase()===value.toLowerCase()||o.value===value});if(!option)return {ok:false,error:'No dropdown option matched that value.'};el.value=option.value}else if(el.type==='checkbox'||el.type==='radio'){el.checked=value.toLowerCase()==='true'||value.toLowerCase()==='yes'||value==='1'}else return {ok:false,error:'That ref is not a dropdown, checkbox, or radio button.'};el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));return {ok:true,title:document.title,text:'Changed '+name(el)+'.'}}
          if(op==='click'){if(!el)return {ok:false,error:'That ref is not on the page. Take a fresh snapshot.'};el.click();return {ok:true,title:document.title,text:'Clicked '+name(el)+'.'}}
          if(op==='move'){var p=point(el,a.x,a.y);var target=el||document.elementFromPoint(p[0],p[1]);if(target)target.dispatchEvent(new MouseEvent('mousemove',{bubbles:true,clientX:p[0],clientY:p[1]}));return {ok:true,title:document.title,text:'Moved over '+(target?name(target):'the page')+'.'}}
          if(op==='drag'){if(!await ask(msg.id,'Allow dragging an item on this page?'))return {ok:false,error:'The user declined to drag the page item.'};var start=point(el,a.x,a.y),end=point(byRef(a.to_ref),a.to_x,a.to_y),target=document.elementFromPoint(start[0],start[1]);if(!target)return {ok:false,error:'No page element was at the drag start point.'};target.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,clientX:start[0],clientY:start[1],button:0}));document.dispatchEvent(new MouseEvent('mousemove',{bubbles:true,clientX:end[0],clientY:end[1],button:0}));document.dispatchEvent(new MouseEvent('mouseup',{bubbles:true,clientX:end[0],clientY:end[1],button:0}));return {ok:true,title:document.title,text:'Dragged the page item.'}}
          if(op==='scroll'){if(a.ref){var near=byRef(a.ref);if(!near)return {ok:false,error:'That ref is no longer on the page. Take a fresh snapshot.'};near.scrollIntoView({block:'center',behavior:'instant'})}else{var amount=Math.max(1,Math.min(30,Number(a.amount)||1));window.scrollBy({top:(a.direction==='up'?-1:1)*Math.max(200,window.innerHeight*.75)*amount,behavior:'instant'})}return {ok:true,title:document.title,text:text(),links:scan()}}
          if(op==='press'){if(!el)el=document.activeElement||document.body;var key=String(a.key||'').toLowerCase();if(key==='enter'&&formSubmit(el)){if(!await ask(msg.id,'Submit this page form?\\n'+clean(name(el),160)))return {ok:false,error:'The user declined to submit the form.'};return {ok:false,error:'The safe page preview blocks form submission. Complete this step in your own browser, then ask the model to continue.'}}var map={enter:'Enter',return:'Enter',tab:'Tab',escape:'Escape',esc:'Escape',backspace:'Backspace',delete:'Delete',space:' ',up:'ArrowUp',down:'ArrowDown',left:'ArrowLeft',right:'ArrowRight',pageup:'PageUp',pagedown:'PageDown',home:'Home',end:'End'};var actual=map[key]||(/^f([1-9]|1[0-2])$/i.test(key)?key.toUpperCase():String(a.key||''));el.dispatchEvent(new KeyboardEvent('keydown',{key:actual,bubbles:true,cancelable:true}));el.dispatchEvent(new KeyboardEvent('keyup',{key:actual,bubbles:true,cancelable:true}));return {ok:true,title:document.title,text:'Pressed '+clean(actual,20)+'.'}}
          return {ok:false,error:'Unsupported browser action.'}}
        function receive(e){if(!e.data)return;if(e.data.type==='autumn-browser-approval'){var answer=approvals.get(e.data.id);if(answer)answer(e.data.approved);return}if(e.data.type==='autumn-browser-command')run(e.data).then(function(result){send(e.data.id,result)},function(error){send(e.data.id,{ok:false,error:clean(error&&error.message,500)})})}
        function connect(e){if(e.source!==parent||!e.data||e.data.type!=='autumn-browser-init'||!e.ports||!e.ports[0])return;bridge=e.ports[0];bridge.onmessage=receive;bridge.start();removeEventListener('message',connect)}
        addEventListener('message',connect);
        document.addEventListener('click',function(e){var a=e.target&&e.target.closest?e.target.closest('a[href]'):null;if(!a)return;e.preventDefault();e.stopPropagation();if(bridge)bridge.postMessage({type:'autumn-browser-link',href:a.href})},true);
      })();
      <\\/script>`;
    var head = '<meta http-equiv="Content-Security-Policy" content="' + policy + '"><base href="' + safeUrl + '">';
    if (/<head\b[^>]*>/i.test(source)) { source = source.replace(/<head\b[^>]*>/i, function (match) { return match + head; }); }
    else { source = '<!doctype html><html><head>' + head + '</head><body>' + source + '</body></html>'; }
    if (/<\/body\s*>/i.test(source)) { return source.replace(/<\/body\s*>/i, agent + '</body>'); }
    return source + agent;
  }
  function requestPage(tab, url, key) {
    var invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
    var load = invoke
      ? invoke('get_browser_page', { tab: tab })
      : fetch('/api/browser_page?tab=' + encodeURIComponent(tab), { cache: 'no-store' }).then(function (response) {
          if (!response.ok) { throw new Error('page expired'); }
          return response.json();
        });
    Promise.resolve(load).then(function (page) {
      if (currentKey !== key || !page || validUrl(page.url) !== url) { return; }
      frame.onload = initializePageAgent;
      frame.srcdoc = safeDocument(page.html, url);
      empty.classList.add('hidden');
      frame.classList.remove('hidden');
    }).catch(function () {
      if (currentKey === key) {
        empty.textContent = 'The page preview expired. Reload the page from the model or enter its address again.';
        empty.classList.remove('hidden');
        frame.classList.add('hidden');
      }
    });
  }
  function setPage(value, tab, key) {
    var safe = validUrl(value);
    if (!safe) { return; }
    currentKey = key;
    currentUrl = safe;
    address.value = safe;
    empty.textContent = 'Loading a safe, read-only preview…';
    empty.classList.remove('hidden');
    frame.classList.add('hidden');
    requestPage(tab, safe, key);
  }

  function navigateAddress() {
    var safe = validUrl(address.value.trim());
    if (!safe) { address.setCustomValidity('Enter a complete public http:// or https:// address.'); address.reportValidity(); return; }
    address.setCustomValidity('');
    window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
      detail: { path: 'browser_open', body: { url: safe, new_tab: newTab } }
    }));
    newTab = false;
    address.placeholder = 'Enter a public page address';
  }

  function sendPendingAction() {
    if (!currentPending || !pagePort || root.classList.contains('hidden')) { return; }
    if (sentActions.has(currentPending.id)) { return; }
    sentActions.add(currentPending.id);
    if (sentActions.size > 256) { sentActions.delete(sentActions.values().next().value); }
    pagePort.postMessage({ type: 'autumn-browser-command', id: currentPending.id, action: currentPending.action });
  }

  function initializePageAgent() {
    if (pagePort) { pagePort.close(); }
    var channel = new MessageChannel();
    pagePort = channel.port1;
    pagePort.onmessage = handleBrowserPageMessage;
    pagePort.start();
    frame.contentWindow.postMessage({ type: 'autumn-browser-init' }, '*', [channel.port2]);
    sendPendingAction();
  }

  function answerBrowserAction(id, result) {
    window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
      detail: { path: 'browser_action_result', body: { id: id, result: result } }
    }));
  }

  document.addEventListener('click', function (event) {
    var target = event.target && event.target.closest ? event.target.closest('[data-agentic-browser-open]') : null;
    if (target) {
      event.preventDefault();
      event.stopPropagation();
      open();
      return;
    }
    if (event.target === root.querySelector('[data-browser-close]')) { close(); }
    if (event.target === root.querySelector('[data-browser-open-external]')) {
      var safe = validUrl(address.value);
      if (safe) { window.open(safe, '_blank', 'noopener,noreferrer'); }
    }
    if (event.target === root.querySelector('[data-browser-newtab]')) {
      newTab = true;
      address.value = '';
      address.placeholder = 'Address for the new tab';
      address.focus();
    }
    if (event.target === root.querySelector('[data-browser-go]')) { navigateAddress(); }
    var approvalButton = event.target && event.target.closest ? event.target.closest('[data-browser-approve]') : null;
    if (approvalButton && approvalId) {
      if (pagePort) { pagePort.postMessage({ type: 'autumn-browser-approval', id: approvalId,
        approved: approvalButton.getAttribute('data-browser-approve') === 'yes' }); }
      approvalId = '';
      approvalText.textContent = '';
      approval.classList.add('hidden');
    }
    var nav = event.target && event.target.closest ? event.target.closest('[data-browser-nav]') : null;
    if (nav) {
      window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
        detail: { path: 'browser_history_step', body: { action: nav.getAttribute('data-browser-nav') } }
      }));
    }
  });
  address.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') { event.preventDefault(); navigateAddress(); }
  });
  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape' && !root.classList.contains('hidden')) { close(); }
  });
  window.addEventListener('autumn-agentic-state', function (event) {
    if (!event.detail || typeof event.detail !== 'object') { return; }
    latest = event.detail;
    if (!((latest.tool_switches || {}).browser_use)) { close(); return; }
    var browser = latest.browser || {};
    var page = Array.isArray(browser.tabs) ? browser.tabs[(Number(browser.active) || 0) - 1] : null;
    var nextUrl = validUrl((page && page.url) || '');
    render(latest, !!nextUrl && nextUrl !== lastActiveUrl && !!latest.generating);
  });
  function handleBrowserPageMessage(event) {
    if (!event.data) { return; }
    if (event.data.type === 'autumn-browser-result') {
      answerBrowserAction(String(event.data.id || ''), event.data.result || { ok: false, error: 'The browser returned no result.' });
      return;
    }
    if (event.data.type === 'autumn-browser-confirm') {
      approvalId = String(event.data.id || '');
      approvalText.textContent = String(event.data.message || 'Allow this browser action?');
      approval.classList.remove('hidden');
      open();
      return;
    }
    if (event.data.type !== 'autumn-browser-link') { return; }
    var safe = validUrl(event.data.href || '');
    if (safe) {
      window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
        detail: { path: 'browser_open', body: { url: safe, new_tab: false } }
      }));
    }
  }
})();

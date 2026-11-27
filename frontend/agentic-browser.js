(function () {
  'use strict';

  var style = document.createElement('style');
  style.textContent = `
    .chat-layout { position: relative; display: flex; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .chat-layout-content { display: flex; flex: 1 1 auto; flex-direction: column; min-width: 0; min-height: 0; }
    .agentic-browser-panel { position: relative; z-index: 19; flex: 0 1 min(760px, 42vw);
      width: min(760px, 42vw); min-width: min(240px, 100%); max-width: 48%; height: 100%;
      display: flex; flex-direction: column; overflow: hidden; border-left: 1px solid var(--line);
      background: var(--bg); color: var(--text); }
    .agentic-browser-panel.hidden { display: none; }
    .general-sidebar-tabs { flex: 0 0 auto; display: flex; align-items: center; gap: 4px; overflow-x: auto; overflow-y: hidden;
      min-height: 40px; padding: 4px 8px; border-bottom: 1px solid var(--line); background: transparent; scrollbar-width: thin; }
    .general-sidebar-tab-wrap { display: inline-flex; flex: 0 0 auto; align-items: center; }
    .general-sidebar-tab { display: inline-flex; align-items: center; max-width: 220px; height: 30px; padding: 0 10px;
      border: 1px solid transparent; border-radius: 7px; background: transparent; color: var(--text-2);
      font: inherit; font-size: 12px; white-space: nowrap; cursor: pointer; }
    .general-sidebar-tab.active { border-color: var(--line); background: var(--surface); color: var(--ink); }
    .general-sidebar-tab-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .general-sidebar-tab-close { flex: 0 0 auto; width: 18px; height: 18px; padding: 0; border: 0;
      border-radius: 4px; background: transparent; color: var(--text-2); cursor: pointer; }
    .general-sidebar-tab-close:hover { background: var(--hover); color: var(--ink); }
    .general-sidebar-spacer { flex: 1 0 8px; }
    .general-sidebar-pane { min-width: 0; min-height: 0; flex: 1 1 auto; display: flex; flex-direction: column; }
    .general-sidebar-pane.hidden { display: none; }
    .general-sidebar-file-pane { overflow: hidden; }
    .agentic-browser-head { display: flex; align-items: center; gap: 6px; padding: 7px 8px;
      border-bottom: 1px solid var(--line); background: transparent; }
    .agentic-browser-label { flex: 0 0 auto; padding: 0 5px; color: var(--text-2); font-size: 12px; font-weight: 650; }
    .agentic-browser-address { min-width: 0; flex: 1 1 auto; height: 30px; padding: 0 9px;
      border: 1px solid var(--line); border-radius: 7px; background: var(--surface); color: var(--text);
      font: 12px ui-monospace, monospace; }
    .agentic-browser-action { flex: 0 0 auto; display: grid; place-items: center;
      width: 30px; height: 30px; border: 1px solid var(--line);
      border-radius: 7px; background: var(--surface); color: var(--text); cursor: pointer; }
    .agentic-browser-action:hover { border-color: var(--accent-line); background: var(--hover); }
    .agentic-browser-status { flex: 0 0 auto; margin: 0; padding: 7px 10px; color: var(--text-2);
      background: var(--surface-2); border-bottom: 1px solid var(--line); font-size: 12px; }
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
    .agentic-browser-empty { display: flex; align-items: center; justify-content: center; gap: 8px; flex: 1 1 auto;
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
    .general-sidebar-file-pane > .viewer { position: static; inset: auto; z-index: auto; min-width: 0; min-height: 0;
      flex: 1 1 auto; align-items: stretch; justify-content: stretch; padding: 0; background: transparent; }
    .general-sidebar-file-pane .look { width: 100%; height: 100%; max-height: none; border: 0; border-radius: 0;
      box-shadow: none; animation: none; }
    .general-sidebar-file-pane .look-close { display: none; }
    .agentic-computer-approval { position: fixed; z-index: 100000; inset: 0; display: grid; place-items: center;
      padding: 20px; background: rgba(0,0,0,.68); }
    .agentic-computer-approval.hidden { display: none; }
    .agentic-computer-approval-card { width: min(560px, 100%); max-height: min(80vh, 700px); display: flex;
      flex-direction: column; gap: 12px; padding: 18px; border: 1px solid var(--line); border-radius: 12px;
      background: var(--surface); color: var(--text); box-shadow: 0 20px 70px rgba(0,0,0,.45); }
    .agentic-computer-approval-card pre { overflow: auto; max-height: 55vh; white-space: pre-wrap; overflow-wrap: anywhere;
      margin: 0; padding: 12px; border-radius: 8px; background: var(--surface-2); font: inherit; }
    .agentic-computer-approval-actions { display: flex; justify-content: flex-end; gap: 8px; }
    .agentic-computer-approval-actions button { min-width: 84px; padding: 8px 12px; border: 1px solid var(--line);
      border-radius: 7px; background: var(--surface-2); color: var(--text); cursor: pointer; }
    .agentic-computer-approval-actions [data-computer-approval="yes"] { border-color: var(--accent-line);
      background: var(--accent); color: var(--on-accent); }
    @media (max-width: 700px) { .agentic-browser-panel { flex-basis: min(360px, 44vw); min-width: min(200px, 44vw); } }
    @media (max-width: 540px) {
      .agentic-browser-label { display: none; }
      .agentic-browser-head { gap: 4px; padding: 6px; }
      .agentic-browser-action { width: 26px; height: 28px; }
    }
  `;
  document.head.appendChild(style);

  var toggle = document.getElementById('generalSidebarBtn');
  var root = document.createElement('aside');
  root.id = 'generalSidebar';
  root.className = 'agentic-browser-panel hidden';
  root.setAttribute('aria-label', 'General sidebar');
  root.innerHTML = '<div class="general-sidebar-tabs" role="tablist" aria-label="Sidebar tabs">' +
    '<button class="general-sidebar-tab active" id="generalSidebarBrowserTab" type="button" role="tab" aria-selected="true" aria-controls="generalSidebarBrowserPane" data-sidebar-tab="browser"><span class="general-sidebar-tab-label">Browser</span></button>' +
    '<span class="general-sidebar-spacer"></span>' +
    '<button class="agentic-browser-action" type="button" data-sidebar-close title="Close sidebar" aria-label="Close sidebar">×</button>' +
    '</div><section class="general-sidebar-pane" id="generalSidebarBrowserPane" role="tabpanel" aria-labelledby="generalSidebarBrowserTab" data-sidebar-pane="browser"><div class="agentic-browser-head">' +
    '<span class="agentic-browser-label">Browser use</span>' +
    '<input class="agentic-browser-address" type="url" aria-label="Browser address" placeholder="Enter a public page address">' +
    '<button class="agentic-browser-action" type="button" data-browser-nav="reload" title="Reload" aria-label="Reload">↻</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-newtab title="New tab" aria-label="New tab">＋</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-go title="Open address in panel" aria-label="Open address in panel">↵</button>' +
    '<button class="agentic-browser-action" type="button" data-browser-open-external title="Open this page in a new browser tab" aria-label="Open this page in a new browser tab"><svg class="svg" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M4 12 12 4M6 4h6v6"/></svg></button>' +
    '</div><div class="agentic-browser-tabs" role="tablist"></div>' +
    '<p class="agentic-browser-status hidden" role="status"></p>' +
    '<div class="agentic-browser-empty"><span></span><button class="btn" type="button" data-browser-enable>Enable Browser use</button></div>' +
    '<iframe class="agentic-browser-frame hidden" title="Isolated page preview" sandbox="allow-scripts" referrerpolicy="no-referrer"></iframe>' +
    '<details class="agentic-browser-reader"><summary>Page text read by the model</summary><div class="agentic-browser-links"></div><pre></pre></details>' +
    '<div class="agentic-browser-approval hidden" role="dialog" aria-modal="true" aria-label="Approve browser action"><strong>Review browser action</strong><pre></pre><div class="agentic-browser-approval-actions"><button type="button" data-browser-approve="no">Deny</button><button type="button" data-browser-approve="yes">Allow</button></div></div></section>';
  var area = document.getElementById('area');
  var dock = document.getElementById('dock');
  var layout = document.createElement('div');
  var chat = document.createElement('div');
  layout.className = 'chat-layout';
  chat.className = 'chat-layout-content';
  if (area && dock && area.parentNode === dock.parentNode) {
    area.parentNode.insertBefore(layout, area);
    layout.appendChild(chat);
    chat.appendChild(area);
    chat.appendChild(dock);
    layout.appendChild(root);
  } else {
    document.body.appendChild(root);
  }

  var sidebarTabs = root.querySelector('.general-sidebar-tabs');
  var browserPane = root.querySelector('[data-sidebar-pane="browser"]');
  var activeSidebarTab = 'browser';
  var sidebarFileSerial = 0;
  var sidebarFiles = new Map();
  var sidebarViewer = document.getElementById('viewer');
  function setSidebarTab(id, notify) {
    var pane = id === 'browser' ? browserPane : sidebarFiles.get(id);
    if (!pane) { return; }
    activeSidebarTab = id;
    root.querySelectorAll('[data-sidebar-tab]').forEach(function (tab) {
      var selected = tab.getAttribute('data-sidebar-tab') === id;
      tab.classList.toggle('active', selected);
      tab.setAttribute('aria-selected', String(selected));
    });
    browserPane.classList.toggle('hidden', id !== 'browser');
    sidebarFiles.forEach(function (filePane, fileId) {
      filePane.classList.toggle('hidden', id !== fileId);
    });
    if (id !== 'browser' && sidebarViewer && sidebarViewer.parentNode !== pane) { pane.appendChild(sidebarViewer); }
    if (notify && id !== 'browser') {
      window.dispatchEvent(new CustomEvent('autumn-sidebar-file-select', { detail: { id: id } }));
    } else if (notify) {
      window.dispatchEvent(new CustomEvent('autumn-sidebar-tab-selected', { detail: { id: id } }));
    }
  }
  function closeSidebarFile(id) {
    var pane = sidebarFiles.get(id);
    if (!pane) { return; }
    var active = activeSidebarTab === id;
    var next = Array.from(sidebarFiles.keys()).filter(function (fileId) { return fileId !== id; }).pop() || 'browser';
    if (active) {
      if (next === 'browser' && sidebarViewer) {
        sidebarViewer.classList.add('hidden');
        root.appendChild(sidebarViewer);
      }
      setSidebarTab(next, true);
    }
    var button = sidebarTabs.querySelector('[data-sidebar-tab="' + id + '"]');
    if (button && button.parentNode) { button.parentNode.remove(); }
    pane.remove();
    sidebarFiles.delete(id);
    window.dispatchEvent(new CustomEvent('autumn-sidebar-file-close', { detail: { id: id } }));
  }
  window.autumnOpenSidebarFile = function (label, onCreate) {
    var id = 'file-' + (++sidebarFileSerial);
    var wrap = document.createElement('span');
    wrap.className = 'general-sidebar-tab-wrap';
    var button = document.createElement('button');
    button.type = 'button';
    button.className = 'general-sidebar-tab';
    button.id = id + '-tab';
    button.setAttribute('role', 'tab');
    button.setAttribute('aria-controls', id + '-pane');
    button.setAttribute('data-sidebar-tab', id);
    button.setAttribute('aria-selected', 'false');
    button.title = String(label || 'File');
    var text = document.createElement('span');
    text.className = 'general-sidebar-tab-label';
    text.textContent = String(label || 'File');
    button.appendChild(text);
    button.addEventListener('click', function () { setSidebarTab(id, true); });
    var closeButton = document.createElement('button');
    closeButton.type = 'button';
    closeButton.className = 'general-sidebar-tab-close';
    closeButton.textContent = '×';
    closeButton.title = 'Close ' + String(label || 'file');
    closeButton.setAttribute('aria-label', closeButton.title);
    closeButton.addEventListener('click', function (event) { event.stopPropagation(); closeSidebarFile(id); });
    wrap.appendChild(button);
    wrap.appendChild(closeButton);
    var spacer = sidebarTabs.querySelector('.general-sidebar-spacer');
    sidebarTabs.insertBefore(wrap, spacer);
    var pane = document.createElement('section');
    pane.className = 'general-sidebar-pane general-sidebar-file-pane hidden';
    pane.setAttribute('role', 'tabpanel');
    pane.id = id + '-pane';
    pane.setAttribute('aria-labelledby', id + '-tab');
    pane.setAttribute('data-sidebar-pane', id);
    root.appendChild(pane);
    sidebarFiles.set(id, pane);
    if (typeof onCreate === 'function') { onCreate(id); }
    showSidebar(true);
    setSidebarTab(id, true);
    return id;
  };
  window.autumnCloseActiveSidebarFile = function () {
    if (activeSidebarTab !== 'browser') { closeSidebarFile(activeSidebarTab); }
  };
  root.querySelector('[data-sidebar-tab="browser"]').addEventListener('click', function () {
    setSidebarTab('browser', true);
  });

  var computerApproval = document.createElement('div');
  computerApproval.className = 'agentic-computer-approval hidden';
  computerApproval.setAttribute('role', 'dialog');
  computerApproval.setAttribute('aria-modal', 'true');
  computerApproval.setAttribute('aria-label', 'Approve computer use');
  computerApproval.innerHTML = '<div class="agentic-computer-approval-card"><strong>Allow computer use?</strong>' +
    '<p>The model is asking to control your desktop. Review the actions before continuing.</p>' +
    '<pre></pre><div class="agentic-computer-approval-actions"><button type="button" data-computer-approval="no">Deny</button>' +
    '<button type="button" data-computer-approval="yes">Allow</button></div></div>';
  document.body.appendChild(computerApproval);

  var address = root.querySelector('.agentic-browser-address');
  var tabs = root.querySelector('.agentic-browser-tabs');
  var frame = root.querySelector('.agentic-browser-frame');
  var empty = root.querySelector('.agentic-browser-empty');
  var emptyText = empty.querySelector('span');
  var enableButton = empty.querySelector('[data-browser-enable]');
  var status = root.querySelector('.agentic-browser-status');
  var readerText = root.querySelector('.agentic-browser-reader pre');
  var readerLinks = root.querySelector('.agentic-browser-links');
  var approval = root.querySelector('.agentic-browser-approval');
  var approvalText = approval.querySelector('pre');
  var approvalId = '';
  var nativeApproval = null;
  var invokeNative = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
  var currentKey = '';
  var newTab = false;
  var lastActiveUrl = '';
  var latest = null;
  var currentPending = null;
  var computerApprovalId = '';
  var computerApprovalSent = false;
  var pagePort = null;
  var pageReady = false;
  var nativeSync = Promise.resolve(false);
  var nativeSyncKey = '';
  var sentActions = new Set();

  function showStatus(message) {
    status.textContent = message;
    status.classList.toggle('hidden', !message);
  }

  function showSidebar(visible) {
    root.classList.toggle('hidden', !visible);
    toggle.setAttribute('aria-expanded', String(visible));
    toggle.classList.toggle('on', visible);
  }

  function open() {
    showSidebar(true);
    render(latest || {}, false);
  }
  function denyApproval() {
    if (!approvalId) { return; }
    if (nativeApproval) {
      answerBrowserAction(approvalId, { ok: false, error: 'The user declined the browser action.' });
      nativeApproval = null;
    } else if (pagePort) {
      pagePort.postMessage({ type: 'autumn-browser-approval', id: approvalId, approved: false });
    }
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
    if (invokeNative && latest) {
      var tab = Number((latest.browser || {}).active) || 0;
      if (tab) { syncNativeWindow(tab, false); }
    }
    showSidebar(false);
    if (root.contains(document.activeElement)) { toggle.focus(); }
  }
  function validUrl(value) {
    try {
      var parsed = new URL(value);
      return (parsed.protocol === 'http:' || parsed.protocol === 'https:') ? parsed.href : '';
    } catch (_) { return ''; }
  }
  function render(state, autoOpen) {
    var enabled = !!(state.tool_switches || {}).browser_use;
    var browser = enabled ? state.browser || {} : {};
    var list = Array.isArray(browser.tabs) ? browser.tabs : [];
    var active = Number(browser.active) || 0;
    currentPending = browser.pending_action && typeof browser.pending_action.id === 'string'
      ? browser.pending_action : null;
    if (!currentPending && approvalId) { denyApproval(); }
    if (currentPending && root.classList.contains('hidden')) { showSidebar(true); }
    enableButton.classList.toggle('hidden', enabled);
    address.disabled = !enabled;
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
      button.textContent = (index + 1) + ' · ' + ((tab && tab.title) ? String(tab.title).slice(0, 80) + ' — ' : '') + url;
      button.title = ((tab && tab.title) ? String(tab.title) + '\n' : '') + url;
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
      if (pagePort) { pagePort.close(); pagePort = null; }
      pageReady = false;
      if (frame.hasAttribute('srcdoc')) { frame.removeAttribute('srcdoc'); }
      frame.classList.add('hidden');
      empty.classList.remove('hidden');
      emptyText.textContent = enabled ? 'Enter a public web address or ask the model to open one.' : '';
      lastActiveUrl = '';
      if (invokeNative) { syncNativeWindow(active || 1, false); }
      return;
    }
    if (document.activeElement !== address) { address.value = nextUrl; }
    var nextKey = JSON.stringify([state.chat && state.chat.id, active, nextUrl, page && page.revision || 0]);
    if (nextKey !== currentKey) {
      setPage(nextUrl, active, nextKey);
      if (autoOpen) { showSidebar(true); }
    }
    if (invokeNative && !root.classList.contains('hidden')) {
      syncNativeWindow(active, true).then(function (ready) {
        if (ready && nextKey === currentKey) { sendPendingAction(); }
      });
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
        function pageText(scope,limit){return String(scope&&(scope.innerText||scope.textContent)||'').replace(/(?:Uh oh!\\s*)?There was an error while loading\\.\\s*Please reload this page\\s*\\.?/gi,'').replace(/\\r/g,'').slice(0,limit||24000)}
        function text(limit){return pageText(document.body,limit).split(/\\n+/).map(function(line){return clean(line,1200)}).filter(Boolean).join('\\n')}
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
          if(op==='snapshot'){var scope=el||document.body,items=scope===document.body?controls:controls.filter(function(item){var child=byRef(item.ref);return child&&scope.contains(child)});return {ok:true,title:document.title,text:pageText(scope,a.full_page?100000:24000),links:items}}
          if(op==='find'){var q=clean(a.query||a.text,200).toLowerCase();var found=controls.filter(function(x){return (x.name+' '+x.role).toLowerCase().includes(q)});var snippets=pageText(document.body,30000).split(/\\n+/).filter(function(x){return x.toLowerCase().includes(q)}).slice(0,10);return {ok:true,title:document.title,text:snippets.join('\\n'),links:found}}
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
        function revealPage(){
          var main=document.querySelector('main, [role=main]')||document.body;
          // Static previews have no site scripts to reveal content after a page animation.
          for(var heading of main.querySelectorAll('h1, h2, h3')){
            var rect=heading.getBoundingClientRect();
            if(rect.width<=1||rect.height<=1||getComputedStyle(heading).visibility!=='visible')continue;
            for(var node=heading;node;node=node.parentElement){
              if(Number(getComputedStyle(node).opacity)!==0)continue;
              node.style.setProperty('animation','none','important');
              node.style.setProperty('transition','none','important');
              node.style.setProperty('opacity','1','important');
            }
          }
          return !!main.innerText.trim();
        }
        function connect(e){
          if(e.source!==parent||!e.data||e.data.type!=='autumn-browser-init'||!e.ports||!e.ports[0])return;
          bridge=e.ports[0];bridge.onmessage=receive;bridge.start();removeEventListener('message',connect);
          bridge.postMessage({type:'autumn-browser-ready',hasContent:revealPage()});
        }
        addEventListener('message',connect);
        document.addEventListener('click',function(e){var a=e.target&&e.target.closest?e.target.closest('a[href]'):null;if(!a)return;e.preventDefault();e.stopPropagation();if(bridge)bridge.postMessage({type:'autumn-browser-link',href:a.href})},true);
      })();
      </script>`;
    var head = '<meta http-equiv="Content-Security-Policy" content="' + policy + '"><base href="' + safeUrl + '">';
    // Put the policy and bridge first so a truncated page cannot swallow either of them.
    return '<!doctype html>' + head + agent + source;
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
      if (currentKey !== key) { return; }
      if (!page || validUrl(page.url) !== url || !page.html) { throw new Error('page expired'); }
      frame.onload = initializePageAgent;
      frame.srcdoc = safeDocument(page.html, url);
      empty.classList.add('hidden');
      frame.classList.remove('hidden');
    }).catch(function () {
      if (currentKey === key) {
        emptyText.textContent = 'The page preview is unavailable. Reload it or open the address in your browser.';
        empty.classList.remove('hidden');
        frame.classList.add('hidden');
      }
    });
  }
  function setPage(value, tab, key) {
    var safe = validUrl(value);
    if (!safe) { return; }
    currentKey = key;
    address.value = safe;
    showStatus('');
    if (pagePort) { pagePort.close(); pagePort = null; }
    pageReady = false;
    emptyText.textContent = 'Loading page preview…';
    empty.classList.remove('hidden');
    frame.classList.add('hidden');
    if (invokeNative) {
      empty.classList.add('hidden');
      frame.classList.remove('hidden');
    } else { requestPage(tab, safe, key); }
  }

  function syncNativeWindow(tab, visible) {
    if (!invokeNative || !tab) { return Promise.resolve(false); }
    var rect = frame.getBoundingClientRect();
    var bounds = {
      tab: tab,
      visible: !!visible,
      x: visible ? Math.max(0, Math.round(rect.left)) : 0,
      y: visible ? Math.max(0, Math.round(rect.top)) : 0,
      width: visible ? Math.max(1, Math.round(rect.width)) : 1,
      height: visible ? Math.max(1, Math.round(rect.height)) : 1
    };
    var key = JSON.stringify([currentKey, bounds]);
    if (key === nativeSyncKey) { return nativeSync; }
    nativeSyncKey = key;
    nativeSync = nativeSync.then(function () {
      if (nativeSyncKey !== key) { return false; }
      return invokeNative('browser_window_sync', bounds).then(function () {
        return visible && nativeSyncKey === key;
      }).catch(function (error) {
        if (visible && nativeSyncKey === key) {
          showStatus('The native browser page could not open: ' + String(error && error.message || error));
        }
        return false;
      });
    });
    return nativeSync;
  }

  function navigate(path, body) {
    showStatus('');
    window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
      detail: { path: path, body: body, onResult: function (result) {
        if (result && result.ok === false) { showStatus(result.status || 'The browser could not complete this action.'); }
      } }
    }));
  }

  function navigateAddress() {
    var safe = validUrl(address.value.trim());
    if (!safe) { address.setCustomValidity('Enter a complete public http:// or https:// address.'); address.reportValidity(); return; }
    address.setCustomValidity('');
    navigate('browser_open', { url: safe, new_tab: newTab });
    newTab = false;
    address.placeholder = 'Enter a public page address';
  }

  function sendPendingAction() {
    if (!currentPending || root.classList.contains('hidden')) { return; }
    if (!invokeNative && !pageReady) { return; }
    if (sentActions.has(currentPending.id)) { return; }
    sentActions.add(currentPending.id);
    if (sentActions.size > 256) { sentActions.delete(sentActions.values().next().value); }
    if (invokeNative) {
      var tab = Number(((latest || {}).browser || {}).active) || 0;
      var id = currentPending.id;
      var action = currentPending.action;
      invokeNative('browser_window_action', {
        tab: tab, action: action, approved: false, allowDownload: false, downloadUrl: null
      })
        .then(function (result) {
          if (!result || typeof result !== 'object') {
            answerBrowserAction(id, { ok: false, error: 'The native browser returned no action result.' });
          } else if (result.approval_required === true) {
            nativeApproval = {
              id: id,
              action: action,
              tab: tab,
              allowDownload: result.allow_download === true,
              downloadUrl: typeof result.download_url === 'string' ? result.download_url : null
            };
            approvalId = id;
            approvalText.textContent = String(result.approval || 'Allow this browser action?');
            approval.classList.remove('hidden');
          } else {
            answerBrowserAction(id, result);
          }
        })
        .catch(function (error) {
          answerBrowserAction(id, { ok: false, error: String(error && error.message || error) });
        });
      return;
    }
    pagePort.postMessage({ type: 'autumn-browser-command', id: currentPending.id, action: currentPending.action });
  }

  function initializePageAgent() {
    if (pagePort) { pagePort.close(); }
    var channel = new MessageChannel();
    pagePort = channel.port1;
    pagePort.onmessage = handleBrowserPageMessage;
    pagePort.start();
    frame.contentWindow.postMessage({ type: 'autumn-browser-init' }, '*', [channel.port2]);
  }

  function answerBrowserAction(id, result) {
    window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
      detail: { path: 'browser_action_result', body: { id: id, result: result } }
    }));
  }

  function updateComputerApproval(state) {
    var pending = state && state.computer_use_available ? state.computer_approval : null;
    if (!pending || !pending.id) {
      computerApprovalId = '';
      computerApprovalSent = false;
      computerApproval.classList.add('hidden');
      return;
    }
    if (pending.id === computerApprovalId && computerApprovalSent) { return; }
    if (pending.id !== computerApprovalId) {
      computerApprovalId = String(pending.id);
      computerApprovalSent = false;
      computerApproval.querySelector('pre').textContent = String(pending.message || 'Allow this desktop action?');
    }
    computerApproval.classList.remove('hidden');
  }

  function showDownloadNotice(event) {
    var payload = event && event.payload;
    if (!payload || !payload.message) { return; }
    showStatus(String(payload.message) + (payload.path ? ' ' + String(payload.path) : ''));
  }
  var eventApi = window.__TAURI__ && window.__TAURI__.event;
  if (invokeNative && eventApi && eventApi.listen) {
    eventApi.listen('agentic-browser-download', showDownloadNotice);
  }
  toggle.addEventListener('click', function () {
    if (root.classList.contains('hidden')) { open(); }
    else { close(); }
  });
  enableButton.addEventListener('click', function () {
    navigate('tool_switch', { part: 'browser_use', on: true });
  });

  document.addEventListener('click', function (event) {
    var computerButton = event.target && event.target.closest
      ? event.target.closest('[data-computer-approval]') : null;
    if (computerButton && computerApprovalId && !computerApprovalSent) {
      computerApprovalSent = true;
      computerApproval.querySelectorAll('button').forEach(function (button) { button.disabled = true; });
      window.dispatchEvent(new CustomEvent('autumn-agentic-action', {
        detail: { path: 'computer_approval_result', body: {
          id: computerApprovalId,
          approved: computerButton.getAttribute('data-computer-approval') === 'yes'
        } }
      }));
      computerApproval.classList.add('hidden');
      computerApproval.querySelectorAll('button').forEach(function (button) { button.disabled = false; });
      return;
    }
    var target = event.target && event.target.closest ? event.target.closest('[data-agentic-browser-open]') : null;
    if (target) {
      event.preventDefault();
      event.stopPropagation();
      open();
      return;
    }
    if (event.target === root.querySelector('[data-sidebar-close]')) { close(); }
    if (event.target.closest && event.target.closest('[data-browser-open-external]')) {
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
      var approved = approvalButton.getAttribute('data-browser-approve') === 'yes';
      if (nativeApproval) {
        var pending = nativeApproval;
        nativeApproval = null;
        if (approved) {
          invokeNative('browser_window_action', {
            tab: pending.tab,
            action: pending.action,
            approved: true,
            allowDownload: pending.allowDownload,
            downloadUrl: pending.downloadUrl
          })
            .then(function (result) { answerBrowserAction(pending.id, result || { ok: false, error: 'The browser returned no action result.' }); })
            .catch(function (error) { answerBrowserAction(pending.id, { ok: false, error: String(error && error.message || error) }); });
        } else {
          answerBrowserAction(pending.id, { ok: false, error: 'The user declined the browser action.' });
        }
      } else if (pagePort) {
        pagePort.postMessage({ type: 'autumn-browser-approval', id: approvalId, approved: approved });
      }
      approvalId = '';
      approvalText.textContent = '';
      approval.classList.add('hidden');
    }
    var nav = event.target && event.target.closest ? event.target.closest('[data-browser-nav]') : null;
    if (nav) { navigate('browser_history_step', { action: nav.getAttribute('data-browser-nav') }); }
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
    updateComputerApproval(latest);
    var browser = latest.browser || {};
    var page = Array.isArray(browser.tabs) ? browser.tabs[(Number(browser.active) || 0) - 1] : null;
    var nextUrl = validUrl((page && page.url) || '');
    render(latest, !!nextUrl && nextUrl !== lastActiveUrl && !!latest.generating);
  });
  var layoutPending = false;
  function syncBrowserLayout() {
    if (layoutPending || !invokeNative || root.classList.contains('hidden')) { return; }
    layoutPending = true;
    requestAnimationFrame(function () {
      layoutPending = false;
      if (root.classList.contains('hidden') || frame.classList.contains('hidden')) { return; }
      var tab = Number(((latest || {}).browser || {}).active) || 0;
      if (tab) { syncNativeWindow(tab, true); }
    });
  }
  window.addEventListener('resize', syncBrowserLayout, { passive: true });
  window.addEventListener('scroll', syncBrowserLayout, { passive: true, capture: true });
  if (typeof ResizeObserver === 'function') { new ResizeObserver(syncBrowserLayout).observe(frame); }
  function handleBrowserPageMessage(event) {
    if (!event.data) { return; }
    if (event.data.type === 'autumn-browser-ready') {
      pageReady = true;
      if (!event.data.hasContent) {
        showStatus('This page has no readable preview. Use the open-in-browser button to view the full site.');
      }
      sendPendingAction();
      return;
    }
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
      navigate('browser_open', { url: safe, new_tab: false });
    }
  }
})();

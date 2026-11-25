// A corporate answer alternates between activity cards and assistant messages. Tool messages
// own executed calls, and drafts belong only to the active model step.
'use strict';

const corporateOpen = new Set();
const READ_MARK = 'M8 4.75C6.45 3.35 4.35 2.95 1.75 3.5V12'
  + 'c2.6-.55 4.7-.15 6.25 1.25 1.55-1.4 3.65-1.8 6.25-1.25V3.5'
  + 'c-2.6-.55-4.7-.15-6.25 1.25zM8 4.75v8.5';
const workFolded = new Map();
const workStarted = new Map();
const workElapsed = new Map();
let workClock = 0;

function processingMark() {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 16');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('class', 'brain-paper');
  const brain = document.createElementNS(svg.namespaceURI, 'path');
  brain.setAttribute('class', 'brain');
  brain.setAttribute('d', 'M13 3.1c-.7-1.5-3.1-1.7-4.1-.3-1.9-.2-3.1 1.8-2.3 3.4-1.7 1.2-1.2 3.8.7 4.3.3 1.8 2.3 2.6 3.8 1.6 1.2 1 2.8.7 3.7-.3 1.5.8 3.3.1 3.6-1.5 1.9-.6 2.3-3.1.7-4.3.8-1.7-.5-3.6-2.3-3.4-1-1.5-3.4-1.2-4.1.3zM13 3.1v8.9M9.2 5.4c.8.1 1.2.7 1.1 1.5M16.8 5.4c-.8.1-1.2.7-1.1 1.5M9.5 9.3c.6-.5 1.3-.5 1.9 0M16.5 9.3c-.6-.5-1.3-.5-1.9 0');
  svg.appendChild(brain);
  for (const [name, x, y] of [['page-one', 1, 5], ['page-two', 3, 9]]) {
    const group = document.createElementNS(svg.namespaceURI, 'g');
    group.setAttribute('class', name);
    const page = document.createElementNS(svg.namespaceURI, 'path');
    page.setAttribute('class', 'page');
    page.setAttribute('d', `M${x} ${y - 2}h4l2 2v5H${x}z`);
    const lines = document.createElementNS(svg.namespaceURI, 'path');
    lines.setAttribute('class', 'page-line');
    lines.setAttribute('d', `M${x + 1} ${y + 1}h3M${x + 1} ${y + 2.5}h2`);
    group.append(page, lines);
    svg.appendChild(group);
  }
  return svg;
}

function corporateDisclosure(key, main) {
  const node = make('div', main ? 'work' : 'wblock');
  node.dataset.foldKey = key;
  const header = make('div', main ? 'whead' : 'wbhead');
  const arrow = make('span', 'wfold');
  arrow.textContent = '▾';
  header.append(arrow);
  const body = make('div', main ? 'wbody' : 'wbbody');
  node.append(header, body);
  function fold() {
    const open = corporateOpen.has(key);
    node.classList.toggle('shut', !open);
    header.setAttribute('aria-expanded', String(open));
  }
  function toggle(event) {
    if (!body.childElementCount || event.target.closest('a')) { return; }
    if (corporateOpen.has(key)) { corporateOpen.delete(key); }
    else { corporateOpen.add(key); }
    fold();
  }
  header.setAttribute('role', 'button');
  header.tabIndex = 0;
  header.onclick = toggle;
  header.onkeydown = function (event) {
    if (event.target === header && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      toggle(event);
    }
  };
  fold();
  return node;
}

function corporateToolTitle(one) {
  return (one.head || 'Working').replace(/^Looked at (a picture|pictures)/,
    one.image_count > 1 || /^Looked at pictures/.test(one.head) ? 'Viewed images' : 'Viewed image');
}

function corporateEntry(node, one) {
  const header = node.firstElementChild;
  const body = node.lastElementChild;
  const arrow = header.firstElementChild;
  const title = make('span', 'wbtitle');
  header.replaceChildren(arrow);
  body.replaceChildren();
  function detail(tag, style, text) {
    if (!text) { return; }
    const part = make(tag, style);
    part.textContent = text;
    body.append(part);
  }
  if (one.tool && one.tool !== 'work_log' && one.tool !== 'finish_answer') {
    header.append(mark(one.reading ? READ_MARK : toolShape(one.tool).mark, 'wtoolmark'));
  }
  title.textContent = corporateToolTitle(one);
  header.append(title);
  if ((one.tool === 'create_file' || one.tool === 'edit_file') && (one.adds || one.cuts)) {
    for (const [style, count] of [['wplus', '+' + (one.adds || 0)], ['wminus', '-' + (one.cuts || 0)]]) {
      const number = make('span', style);
      number.textContent = count;
      header.append(number);
    }
  }
  if (one.failed) {
    const failure = make('span', 'wbad');
    failure.textContent = 'it failed';
    header.append(failure);
  }
  for (const line of one.lines || []) { detail('div', 'wline', line); }
  detail('pre', 'wcmd', one.command);
  detail('div', 'wfile', one.file);
  detail('pre', 'wout', one.output);
  for (const [index, child] of (one.sub || []).entries()) {
    const nested = corporateDisclosure(node.dataset.foldKey + '/' + index, false);
    corporateEntry(nested, child);
    body.append(nested);
  }
  arrow.style.visibility = body.childElementCount ? '' : 'hidden';
  header.tabIndex = body.childElementCount ? 0 : -1;
  header.style.cursor = body.childElementCount ? 'pointer' : 'default';
}

function corporateMainMark(header, body, title) {
  let icon = null;
  if (title === 'Thinking' || title === 'Thought' || title.startsWith('Thought for ')) {
    icon = mark(THINK_MARK, 'wmainmark bulb');
  } else {
    const marks = body.querySelectorAll('.wtoolmark');
    if (marks.length) {
      icon = marks[marks.length - 1].cloneNode(true);
      icon.classList.remove('wtoolmark');
      icon.classList.add('wmainmark');
    }
  }
  const current = header.querySelector('.wmainmark');
  const label = header.querySelector('.wtitle');
  if (!icon) {
    current?.remove();
    return;
  }
  if (current && current.className.baseVal === icon.className.baseVal
      && current.querySelector('path')?.getAttribute('d') === icon.querySelector('path')?.getAttribute('d')) {
    return;
  }
  current?.remove();
  header.insertBefore(icon, label);
}

function workingMainMark(header, body, enabled) {
  const status = header.querySelector('.work-turn-state');
  const current = header.querySelector('.work-turn-mark');
  let source = null;
  if (enabled) {
    const marks = body.querySelectorAll('.work > .whead > .wmainmark');
    source = marks[marks.length - 1] || body.querySelector('.wtoolmark');
  }
  if (!source) {
    current?.remove();
    return;
  }
  const icon = source.cloneNode(true);
  icon.classList.remove('wmainmark', 'wtoolmark');
  icon.classList.add('work-turn-mark');
  if (current && current.className.baseVal === icon.className.baseVal
      && current.querySelector('path')?.getAttribute('d') === icon.querySelector('path')?.getAttribute('d')) {
    return;
  }
  current?.remove();
  header.insertBefore(icon, status);
}

// Separate activity at each stretch of visible prose. Local ordinals keep the same nodes
// when a streamed message is saved or a tool draft receives a permanent message id.
function corporateAnswer(run, state) {
  const parts = [];
  const seen = new Set();
  const executions = run.some(m => m.role === 'Tool' && m.activity);
  let card = null;
  function activity() {
    if (!card) {
      card = { key: parts.length, tools: [], ms: 0, busy: false };
      parts.push(card);
    }
    return card;
  }
  function thought(ms, busy) {
    const one = activity();
    one.ms += Number(ms) || 0;
    one.busy ||= busy;
  }
  function legacyHeading(one) {
    return { ...one, sub: (one.sub || []).filter(child => child.tool === 'work_log').map(legacyHeading) };
  }
  function legacy(work) {
    for (const one of work || []) {
      if (one.group) { legacy(one.sub); continue; }
      if (String(one.id || '').startsWith('stream:') || one.tool === 'finish_answer') { continue; }
      if (executions && one.tool !== 'work_log') { continue; }
      if (one.id && seen.has(one.id)) { continue; }
      if (one.id) { seen.add(one.id); }
      // Older chats can include the same executed calls inside work_log headings.
      activity().tools.push(executions ? legacyHeading(one) : one);
    }
  }
  function prose(text, m) {
    if (!text.trim()) { return null; }
    const one = { key: parts.length, text, message: m };
    parts.push(one);
    card = null;
    return one;
  }
  for (const m of run) {
    if (m.role === 'Tool') {
      if (m.reasoning?.trim()) { thought(m.reasoning_ms, false); }
      if (m.activity && !seen.has(m.id)) {
        seen.add(m.id);
        const one = activity();
        one.tools.push(m.activity);
        if (m.tool?.startsWith('Running ') || m.activity.head?.startsWith('Running ')) { one.busy = true; }
        if (m.answer_head) { one.summary = m.answer_head; }
      }
      continue;
    }
    legacy(m.work);
    if (card && m.answer_head) { card.summary = m.answer_head; }
    if (card && m.work_head && !executions && card.tools.length) { card.legacyTitle = m.work_head; }

    const body = m.body || '';
    const offsets = m.reasoning?.trim() ? (m.thought_offsets?.length ? m.thought_offsets : [0]) : [];
    let start = 0, lastText = null;
    for (const [index, end] of offsets.entries()) {
      if (end < start || end > body.length) { continue; }
      lastText = prose(body.slice(start, end), m) || lastText;
      thought(offsets.length === 1 ? m.reasoning_ms : 0,
        m.id === 'live' && !m.reasoning_done && index === offsets.length - 1);
      start = end;
    }
    lastText = prose(body.slice(start), m) || lastText;
    if (lastText) { lastText.actions = true; }

    if (m.id === 'live' && state.generating && !state.summarising && state.tool_previews?.length) {
      const one = activity();
      one.drafts = state.tool_previews;
      one.busy = true;
    }
  }
  // Keep the newest activity live until the whole answer ends. In particular, a returned tool
  // result can be under review while the model is thinking or writing the next short message.
  if (state.generating || state.running || state.summarising) {
    for (let n = parts.length - 1; n >= 0; n--) {
      if (parts[n].text === undefined) { parts[n].active = true; break; }
    }
  }
  return parts;
}

function updateCorporateCard(card, part) {
  const header = card.firstElementChild;
  header.classList.remove('processing');
  header.classList.toggle('busy', Boolean(part.active));
  const body = card.lastElementChild;
  const old = new Map(Array.from(body.children, node => [node.dataset.foldKey, node]));
  function insert(work, slot, index) {
    const id = card.dataset.foldKey + '/tool/' + slot;
    const node = old.get(id) || corporateDisclosure(id, false);
    old.delete(id);
    const stamp = JSON.stringify(work);
    if (node._activity !== stamp) {
      corporateEntry(node, work);
      node._activity = stamp;
    }
    if (body.children[index] !== node) { body.insertBefore(node, body.children[index] || null); }
  }
  part.tools.forEach((work, index) => insert(work, index, index));
  // A hidden cat preview still occupies a slot in a batch of tool calls.
  (part.drafts || []).forEach((work, index) =>
    insert(work, part.tools.length + (work.slot ?? index), part.tools.length + index));
  for (const stale of old.values()) { stale.remove(); }
  // Use the last rendered row, so an active card with visible actions always names one.
  const latest = body.lastElementChild?.firstElementChild.querySelector('.wbtitle')?.textContent;
  const summary = [part.summary, part.legacyTitle].find(head => head && head !== 'Done' && head !== 'Thinking');
  const thought = part.active && part.busy ? 'Thinking' :
    part.ms ? 'Thought for ' + spanWords(Math.max(1000, part.ms)) : 'Thought';
  const title = (!part.active && summary) || latest || thought;
  const label = header.lastElementChild;
  if (label.textContent !== title) { label.textContent = title; }
  corporateMainMark(header, body, title);

  const hasDetails = body.childElementCount > 0;
  header.querySelector('.wfold').style.visibility = hasDetails ? '' : 'hidden';
  header.tabIndex = hasDetails ? 0 : -1;
  header.style.cursor = hasDetails ? '' : 'default';
  if (hasDetails) {
    header.setAttribute('role', 'button');
    header.setAttribute('aria-expanded', String(!card.classList.contains('shut')));
  } else {
    header.removeAttribute('role');
    header.removeAttribute('aria-expanded');
  }
}

function updateCorporateProse(node, part) {
  const text = node.firstElementChild;
  const m = part.message;
  text.dataset.mid = m.id;
  if (node._text !== part.text) {
    text.replaceChildren(...blocks(part.text).map(blockNode));
    node._text = part.text;
  }
  const stamp = part.actions && m.id !== 'live' &&
    JSON.stringify([m.id, m.body, m.when, m.can_continue]);
  if (node._actions !== stamp) {
    node.querySelector('.stamp')?.remove();
    const actions = stamp && actLine(m);
    if (actions) { node.append(actions); }
    node._actions = stamp;
  }
}

function updateCorporateAnswer(turn, run, state) {
  const old = new Map(Array.from(turn.children)
    .filter(node => node.dataset.part !== undefined)
    .map(node => [node.dataset.part, node]));
  corporateAnswer(run, state).forEach(function (part, index) {
    const text = part.text !== undefined;
    const key = (text ? 'words/' : 'activity/') + part.key;
    let node = old.get(key);
    old.delete(key);
    if (text) {
      if (!node) {
        node = make('div', 'corporate-message');
        node.append(make('div', 'plain answer-words'));
      }
      updateCorporateProse(node, part);
    } else {
      if (!node) {
        node = corporateDisclosure(turn.dataset.row + '/' + key, true);
        node.firstElementChild.append(make('span', 'wtitle'));
      }
      updateCorporateCard(node, part);
    }
    node.dataset.part = key;
    if (turn.children[index] !== node) { turn.insertBefore(node, turn.children[index] || null); }
  });
  for (const stale of old.values()) { stale.remove(); }
}

// Live turns use the time seen by this page. Older saved turns have no total duration field, so
// their available thinking intervals provide the best duration the chat record can report.
function workingDuration(key, active, messages) {
  const now = performance.now();
  if (active) {
    if (!workStarted.has(key)) { workStarted.set(key, now); }
    return (workElapsed.get(key) || 0) + now - workStarted.get(key);
  }
  if (workStarted.has(key)) {
    workElapsed.set(key, (workElapsed.get(key) || 0) + now - workStarted.get(key));
    workStarted.delete(key);
  }
  if (workElapsed.has(key)) { return workElapsed.get(key); }
  return (messages || []).reduce((total, message) =>
    total + (Number(message.reasoning_ms) || 0), 0);
}

function workingLabel(key, active, messages) {
  return (active ? 'Working for ' : 'Worked for ')
    + spanWords(workingDuration(key, active, messages));
}

function updateWorkingClock() {
  let active = false;
  for (const node of document.querySelectorAll('.work-turn[data-active="true"]')) {
    const key = node.dataset.workKey;
    if (!workStarted.has(key)) { continue; }
    active = true;
    const status = node.querySelector('.work-turn-state');
    const label = workingLabel(key, true, []);
    if (status.textContent !== label) {
      status.textContent = label;
      const head = node.querySelector('.work-turn-head');
      head.setAttribute('aria-label', label + (node.classList.contains('shut')
        ? ', expand work' : ', collapse work'));
    }
  }
  if (!active && workClock) {
    clearInterval(workClock);
    workClock = 0;
  }
}

function startWorkingClock() {
  if (!workClock) { workClock = setInterval(updateWorkingClock, 1000); }
}

function workingDisclosure(key) {
  const node = make('div', 'turn work-turn');
  node.dataset.viewKey = 'work:' + key;
  node.dataset.workKey = key;
  const header = make('button', 'work-turn-head');
  header.type = 'button';
  const fold = make('span', 'work-turn-fold');
  fold.textContent = '▾';
  const dots = make('span', 'dots work-turn-dots');
  dots.append(make('i'), make('i'), make('i'));
  header.append(fold, dots, processingMark());
  const status = make('span', 'work-turn-state');
  header.append(status);
  const body = make('div', 'work-turn-body');
  node.dataset.active = 'false';
  node.append(header, body);
  function apply() {
    const shut = workFolded.has(key) ? Boolean(workFolded.get(key)) : true;
    node.classList.toggle('shut', shut);
    header.setAttribute('aria-expanded', String(!shut));
    header.setAttribute('aria-label', status.textContent
      + (shut ? ', expand work' : ', collapse work'));
  }
  header.addEventListener('click', function () {
    workFolded.set(key, !node.classList.contains('shut'));
    apply();
  });
  apply();
  return node;
}

function workingPlan(items, state) {
  const plan = [];
  let at = 0;
  const inFlight = Boolean(state.generating || state.running || state.summarising);
  while (at < items.length) {
    const message = items[at];
    if (message.role !== 'You') {
      if (message.role !== 'Assistant' && message.role !== 'Tool'
          && message.role !== 'Summary' && message.role !== 'Note') {
        plan.push({ kind: 'message', message, index: at++ });
        continue;
      }
      const from = at;
      while (at < items.length && items[at].role !== 'You') { at++; }
      plan.push(...workingEntry(items.slice(from, at), 'orphan:' + message.id,
        from, at, inFlight && at === items.length,
        workResultProcessing(items.slice(from, at), state, inFlight && at === items.length), state));
      continue;
    }
    plan.push({ kind: 'message', message, index: at++ });
    const from = at;
    while (at < items.length && items[at].role !== 'You') { at++; }
    plan.push(...workingEntry(items.slice(from, at), 'turn:' + message.id,
      from, at, inFlight && at === items.length,
      workResultProcessing(items.slice(from, at), state, inFlight && at === items.length), state));
  }
  return plan;
}

function workResultProcessing(messages, state, active) {
  if (!active || state.running || (state.tool_previews || []).length) { return false; }
  if (messages.some(message => message.id === 'live' && (message.tools || []).length)) { return false; }
  if (messages.some(message => message.role === 'Tool' && message.activity
      && /^Running /.test(message.activity.head || ''))) { return false; }
  return messages.some(message => message.role === 'Tool' && message.activity && message.activity.done);
}

function hasRecordedActivity(work) {
  return (work || []).some(one => one.group
    ? hasRecordedActivity(one.sub) : one.tool !== 'finish_answer');
}

function hasWorkActivity(messages, state) {
  if (state.running || state.summarising || (state.tool_previews || []).length) { return true; }
  return messages.some(message => message.role === 'Tool' || message.role === 'Summary'
    || String(message.reasoning || '').trim()
    || (message.tools || []).length > 0
    || hasRecordedActivity(message.work)
    || (String(message.work_head || '').trim()
      && message.work_head !== 'Done' && message.work_head !== 'Thinking'));
}

function workingEntry(messages, key, from, to, active, processing, state) {
  if (!hasWorkActivity(messages, state)) {
    const direct = [];
    for (let index = 0; index < messages.length; index++) {
      const message = messages[index];
      if (message.id !== 'live' || String(message.body || '').length > 0
          || String(message.reasoning || '').trim()) {
        direct.push({ kind: 'message', message, index: from + index });
      }
    }
    return direct;
  }
  let finalAt = -1;
  if (!active) {
    for (let n = messages.length - 1; n >= 0; n--) {
      const one = messages[n];
      if (one.role !== 'Assistant' && one.role !== 'Tool') { continue; }
      if (one.role === 'Assistant' && one.id !== 'live' && !one.stopped
          && String(one.body || '').trim()) { finalAt = n; }
      break;
    }
  }
  let final = null;
  let split = null;
  if (finalAt >= 0) {
    const original = messages[finalAt];
    const body = String(original.body || '');
    const offsets = Array.isArray(original.thought_offsets) ? original.thought_offsets : [];
    const rawStart = offsets.length ? Number(offsets[offsets.length - 1]) || 0 : 0;
    const finalStart = Math.max(0, Math.min(body.length, rawStart));
    if (!body.slice(finalStart).trim()) { finalAt = -1; }
    else { split = { body, offsets, finalStart }; }
  }
  const work = messages.slice(0, finalAt >= 0 ? finalAt + 1 : messages.length);
  if (finalAt >= 0) {
    const original = messages[finalAt];
    work[finalAt] = Object.assign({}, original, {
      id: 'work:' + original.id,
      body: split.body.slice(0, split.finalStart),
      thought_offsets: split.offsets.filter(offset => Number(offset) <= split.finalStart),
      can_continue: false,
      downloads: [],
      displayed: [],
      suggestions: [],
      steps: null
    });
    final = Object.assign({}, original, {
      body: split.body.slice(split.finalStart),
      reasoning: '',
      reasoning_ms: 0,
      thought_offsets: [],
      work: [],
      work_head: ''
    });
  }
  return [
    { kind: 'working', key, active, processing, messages: work, from,
      to: finalAt >= 0 ? from + finalAt : to },
    ...(final ? [{ kind: 'final', key: 'final:' + final.id, message: final,
      source: messages[finalAt], index: from + finalAt }] : []),
    ...(finalAt >= 0 ? messages.slice(finalAt + 1).map((message, index) => ({
      kind: 'message', message, index: from + finalAt + index + 1
    })) : [])
  ];
}

function moveChildren(parent, wanted, keepPlan) {
  const retained = new Set(wanted);
  for (const child of Array.from(parent.children)) {
    if (keepPlan && child.id === 'planask') { continue; }
    if (!retained.has(child)) { child.remove(); }
  }
  for (let n = 0; n < wanted.length; n++) {
    const node = wanted[n];
    const others = Array.from(parent.children).filter(child => !(keepPlan && child.id === 'planask'));
    const anchor = others[n] || (keepPlan && parent.querySelector(':scope > #planask')) || null;
    if (anchor !== node) { parent.insertBefore(node, anchor || null); }
  }
}

function workingSingleNode(parent, key, message, index) {
  let node = Array.from(parent.children).find(child => child.dataset.viewKey === key);
  if (!node) {
    node = make('div', 'turn');
    node.dataset.viewKey = key;
    if (message.id === 'live') { fillLive(node, message); }
    else { fill(node, message); }
    claimFlyTurn(node, message.role === 'You' ? message : null);
    node._workingStamp = JSON.stringify([roleStamp(message), message.role === 'Summary'
      && handoffOf(message.id)]);
  } else {
    if (message.id === 'live') {
      feedInk(message);
      node.dataset.from = String(index);
      node.dataset.to = String(index + 1);
      return node;
    }
    const stamp = JSON.stringify([roleStamp(message), message.role === 'Summary'
      && handoffOf(message.id)]);
    if (node._workingStamp !== stamp) {
      fill(node, message);
      node._workingStamp = stamp;
    }
  }
  node.dataset.from = String(index);
  node.dataset.to = String(index + 1);
  return node;
}

function plainWorkRows(body, messages, state) {
  const spans = thoughtSpans(messages);
  const rows = threadRows(messages, spans);
  const old = new Map(Array.from(body.children, node => [node.dataset.workRow, node]));
  const oldRows = Array.from(body.children);
  const keep = [];
  for (let n = 0; n < rows.length; n++) {
    const row = rows[n];
    const key = rowStamp(messages, row);
    let node = old.get(key);
    old.delete(key);
    if (!node) {
      node = oldRows.find(candidate => old.has(candidate.dataset.workRow)
        && workingRowsMatch(candidate.dataset.workRow, key));
      if (node) { old.delete(node.dataset.workRow); }
    }
    if (!node) {
      node = make('div', 'turn');
      drawRow(node, messages, row);
    } else {
      syncTurn(node, messages, row);
    }
    node.dataset.workRow = key;
    keep.push(node);
    const end = endPlan(messages, rows, Boolean(state.generating))[n];
    placeEnd(node, end);
  }
  for (const stale of old.values()) { stale.remove(); }
  for (let n = 0; n < keep.length; n++) {
    if (body.children[n] !== keep[n]) { body.insertBefore(keep[n], body.children[n] || null); }
  }
  const tail = messages[messages.length - 1];
  if (tail && tail.id === 'live') { feedInk(tail); }
}

function workingRowsMatch(before, after) {
  if (sameTalk(before, after)) { return true; }
  const oldIds = before.split('+');
  const newIds = after.split('+');
  return oldIds.includes('live') && newIds.some(id => id.startsWith('work:'));
}

function corporateWorkRows(body, messages, state) {
  const old = new Map(Array.from(body.children, node => [node.dataset.workPart, node]));
  const keep = [];
  let answerIndex = 0;
  for (let at = 0; at < messages.length;) {
    const message = messages[at];
    if (message.role === 'Assistant' || message.role === 'Tool') {
      const from = at++;
      while (at < messages.length
          && (messages[at].role === 'Assistant' || messages[at].role === 'Tool')) { at++; }
      const run = messages.slice(from, at);
      const key = 'answer:' + answerIndex++;
      let node = old.get(key);
      old.delete(key);
      if (!node) { node = make('div', 'corp-work-run'); }
      node.dataset.workPart = key;
      node.dataset.row = key;
      updateCorporateAnswer(node, run, state);
      keep.push(node);
      continue;
    }
    const key = 'message:' + message.id;
    let node = old.get(key);
    old.delete(key);
    if (!node) { node = make('div', 'turn'); }
    const stamp = JSON.stringify([roleStamp(message), message.role === 'Summary'
      && handoffOf(message.id)]);
    if (node._workingStamp !== stamp) {
      fill(node, message);
      node._workingStamp = stamp;
    }
    node.dataset.workPart = key;
    keep.push(node);
    at++;
  }
  for (const stale of old.values()) { stale.remove(); }
  for (let n = 0; n < keep.length; n++) {
    if (body.children[n] !== keep[n]) { body.insertBefore(keep[n], body.children[n] || null); }
  }
}

function workingNode(box, item, state) {
  const key = item.key + ':' + (cur && cur.chat && cur.chat.id || '');
  const label = workingLabel(key, item.active, item.messages);
  if (item.active) { startWorkingClock(); }
  let node = Array.from(box.children).find(child => child.dataset.viewKey === 'work:' + key);
  if (!node) { node = workingDisclosure(key); }
  const status = node.querySelector('.work-turn-state');
  node.dataset.active = String(Boolean(item.active));
  node.classList.toggle('corp-work-turn', Boolean(chatting.corporate));
  node.classList.toggle('processing', Boolean(chatting.corporate && item.processing));
  status.textContent = label;
  node.firstElementChild.setAttribute('aria-label', status.textContent +
    (node.classList.contains('shut') ? ', expand work' : ', collapse work'));
  node.dataset.from = String(item.from);
  node.dataset.to = String(item.to);
  const body = node.lastElementChild;
  if (chatting.corporate) { corporateWorkRows(body, item.messages, state); }
  else { plainWorkRows(body, item.messages, state); }
  workingMainMark(node.firstElementChild, body, Boolean(chatting.corporate));
  return node;
}

function corporateFinalNode(box, item) {
  let node = Array.from(box.children).find(child => child.dataset.viewKey === item.key);
  if (!node) {
    node = make('div', 'turn t-ai');
    node.dataset.viewKey = item.key;
    const prose = make('div', 'corporate-message');
    prose.append(make('div', 'plain answer-words'));
    node.append(prose);
  }
  node.className = 'turn t-ai';
  node.dataset.from = String(item.index);
  node.dataset.to = String(item.index + 1);
  const prose = node.firstElementChild;
  updateCorporateProse(prose, {
    text: item.message.body || '', message: item.source, actions: true
  });
  placeEnd(node, endPlan([item.source], [{ from: 0, to: 1 }], false)[0]);
  return node;
}

function drawWorkingThread(state, items) {
  const box = el.col;
  const chat = state.chat && state.chat.id || '';
  const mode = chatting.corporate ? 'corporate' : 'plain';
  const fresh = box.dataset.chat !== chat || box.dataset.workingMode !== mode
    || box.dataset.key === '';
  const hasLive = items.some(message => message.id === 'live');
  if (fresh || !hasLive) { stopInk(); }
  if (fresh) {
    for (const child of Array.from(box.children)) { if (child.id !== 'planask') { child.remove(); } }
  }
  box.dataset.chat = chat;
  box.dataset.workingMode = mode;
  box.dataset.key = 'working:' + mode;
  const plan = workingPlan(items, state);
  const ordered = [];
  for (const item of plan) {
    if (item.kind === 'message') {
      ordered.push(workingSingleNode(box, 'message:' + item.message.id,
        item.message, item.index));
    } else if (item.kind === 'working') {
      ordered.push(workingNode(box, item, state));
    } else if (item.kind === 'final') {
      if (chatting.corporate) {
        ordered.push(corporateFinalNode(box, item));
      } else {
        const visible = item.message;
        ordered.push(workingSingleNode(box, item.key, visible, item.index));
        const node = ordered[ordered.length - 1];
        const sourceStamp = JSON.stringify([roleStamp(item.source), item.source.body,
          item.source.when, item.source.can_continue]);
        if (node._workSourceStamp !== sourceStamp) {
          node.querySelector('.stamp')?.remove();
          const actions = actLine(item.source);
          if (actions) { node.append(actions); }
          node._workSourceStamp = sourceStamp;
        }
        placeEnd(node, endPlan([item.source], [{ from: 0, to: 1 }], false)[0]);
      }
    }
  }
  moveChildren(box, ordered, true);
  if (fresh) { el.thread.scrollTop = 0; stick = true; }
  else if (stick) { el.thread.scrollTop = el.thread.scrollHeight; stepHeadShown(); }
}

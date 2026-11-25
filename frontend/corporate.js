// A corporate answer alternates between activity cards and assistant messages. Tool messages
// own executed calls, and drafts belong only to the active model step.
'use strict';

const corporateOpen = new Set();
const READ_MARK = 'M8 4.75C6.45 3.35 4.35 2.95 1.75 3.5V12'
  + 'c2.6-.55 4.7-.15 6.25 1.25 1.55-1.4 3.65-1.8 6.25-1.25V3.5'
  + 'c-2.6-.55-4.7-.15-6.25 1.25zM8 4.75v8.5';

function corporateDisclosure(key, main) {
  const node = make('div', main ? 'work' : 'wblock');
  node.dataset.foldKey = key;
  const header = make('div', main ? 'whead' : 'wbhead');
  const arrow = make('span', 'wfold');
  arrow.textContent = '▾';
  if (main) {
    const dots = make('span', 'dots');
    dots.append(make('i'), make('i'), make('i'));
    header.append(dots);
  }
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
  title.textContent = (one.head || 'Working').replace(/^Looked at (a picture|pictures)/,
    one.image_count > 1 || /^Looked at pictures/.test(one.head) ? 'Viewed images' : 'Viewed image');
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
      if (m.reasoning || m.reasoning_ms) { thought(m.reasoning_ms, false); }
      if (m.activity && !seen.has(m.id)) {
        seen.add(m.id);
        const one = activity();
        one.tools.push(m.activity);
        if (m.tool?.startsWith('Running ')) { one.running = m.activity.head; }
        if (m.answer_head) { one.summary = m.answer_head; }
      }
      continue;
    }
    legacy(m.work);
    if (card && m.answer_head) { card.summary = m.answer_head; }
    if (card && m.work_head && !executions && card.tools.length) { card.legacyTitle = m.work_head; }

    const body = m.body || '';
    const offsets = m.thought_offsets?.length ? m.thought_offsets : (m.reasoning || m.reasoning_ms ? [0] : []);
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
  return parts;
}

function updateCorporateCard(card, part) {
  const header = card.firstElementChild;
  header.classList.toggle('busy', Boolean(part.busy || part.running));
  const lastTool = part.tools[part.tools.length - 1];
  const elapsed = part.ms ? 'Thought for ' + spanWords(Math.max(1000, part.ms)) : 'Thought';
  const summary = part.summary === 'Done' ? '' : part.summary;
  const title = part.running || (part.drafts ? 'Writing a tool call' :
    part.busy ? 'Thinking' : summary || part.legacyTitle || lastTool?.head || elapsed);
  const label = header.lastElementChild;
  if (label.textContent !== title) { label.textContent = title; }

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

function drawCorporateThread(state, items) {
  const box = el.col;
  const chat = state.chat && state.chat.id || '';
  const fresh = box.dataset.chat !== chat;
  const reset = fresh || box.dataset.key !== 'corporate';
  stopInk();
  if (reset) { threadTurns(box).forEach(node => node.remove()); }
  box.dataset.key = 'corporate';
  box.dataset.chat = chat;
  box.dataset.count = items.length;
  const turns = threadTurns(box);
  const ranges = [];
  let anchor = 'first';
  function isAnswer(m) { return m.role === 'Assistant' || m.role === 'Tool'; }
  for (let from = 0; from < items.length;) {
    const m = items[from];
    const answer = isAnswer(m);
    let to = from + 1;
    if (answer) { while (to < items.length && isAnswer(items[to])) { to++; } }
    else { anchor = m.id; }
    const key = JSON.stringify([chat, anchor, answer]);
    const index = ranges.length;
    let turn = turns[index];
    if (!turn || turn.dataset.row !== key) {
      const next = make('div', answer ? 'turn t-ai' : 'turn');
      if (turn) { turn.replaceWith(next); } else { addTurn(box, next); }
      turn = turns[index] = next;
      turn.dataset.row = key;
    }
    turn.dataset.from = from;
    turn.dataset.to = to;
    turn.dataset.mid = m.id;
    if (answer) {
      updateCorporateAnswer(turn, items.slice(from, to), state);
    } else {
      const stamp = JSON.stringify([m, m.role === 'Summary' && handoffOf(m.id)]);
      if (turn._content !== stamp) {
        fill(turn, m);
        turn._content = stamp;
        claimFlyTurn(turn, m);
      }
    }
    ranges.push({ from, to });
    from = to;
  }
  while (turns.length > ranges.length) { turns.pop().remove(); }
  endPlan(items, ranges, state.generating).forEach((plan, i) => placeEnd(turns[i], plan));
  if (fresh) { el.thread.scrollTop = 0; stick = true; }
  else if (stick) { el.thread.scrollTop = el.thread.scrollHeight; stepHeadShown(); }
}

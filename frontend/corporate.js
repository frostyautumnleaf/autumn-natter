// Corporate display has one thread row per answer. Tool messages own executed calls.
// Drafts belong only to the active model step and are never restored from history.
'use strict';

const corporateOpen = new Set();

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

function corporateEntry(node, item) {
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
  if (item.words !== undefined) {
    node.classList.add('wwords');
    const lines = item.words.trim().split('\n');
    inlineInto(title, inlineParts(lines.shift()), false);
    header.append(title);
    for (const part of blocks(lines.join('\n').trim())) { body.append(blockNode(part)); }
  } else {
    const one = item.work;
    if (one.tool && one.tool !== 'work_log' && one.tool !== 'finish_answer') {
      const book = 'M2 3h4.2A2.8 2.8 0 0 1 9 5.8V13a2.2 2.2 0 0 0-2.2-2H2zM14 3H9.8A2.8 2.8 0 0 0 7 5.8V13a2.2 2.2 0 0 1 2.2-2H14zM9 5.8V13';
      header.append(mark(one.reading ? book : toolShape(one.tool).mark, 'wtoolmark'));
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
      corporateEntry(nested, { work: child });
      body.append(nested);
    }
  }
  arrow.style.visibility = body.childElementCount ? '' : 'hidden';
  header.tabIndex = body.childElementCount ? 0 : -1;
  header.style.cursor = body.childElementCount ? 'pointer' : 'default';
}

// Ordinals are local to this answer. A draft keeps its place when the engine gives
// the execution a permanent message id, even if the model supplied no call id.
function corporateAnswer(run, state, beforeSummary) {
  const entries = [];
  const tail = run[run.length - 1];
  const live = tail.id === 'live' || tail.id === 'live-work';
  const final = !live && !beforeSummary && tail.role === 'Assistant' && tail.body.trim() ? tail : null;
  const seen = new Set();
  const executions = run.some(m => m.role === 'Tool' && m.activity);
  let tools = 0, words = 0, title = 'Done';
  function tool(work) {
    entries.push({ key: 'tool/' + tools++, work: work });
  }
  function legacyHeading(one) {
    return { ...one, sub: (one.sub || []).filter(child => child.tool === 'work_log').map(legacyHeading) };
  }
  function legacy(work) {
    for (const one of work || []) {
      if (one.group) { legacy(one.sub); continue; }
      if (String(one.id || '').startsWith('stream:')) { continue; }
      if (executions && one.tool !== 'work_log') { continue; }
      if (one.id && seen.has(one.id)) { continue; }
      if (one.id) { seen.add(one.id); }
      // Older chats can include the same executed calls inside work_log headings.
      tool(executions ? legacyHeading(one) : one);
    }
  }
  for (const m of run) {
    if (m.role === 'Tool') {
      if (m.activity && !seen.has(m.id)) {
        seen.add(m.id);
        tool(m.activity);
        title = m.activity.head;
      }
      continue;
    }
    legacy(m.work);
    if (m !== final && m.body.trim()) {
      entries.push({ key: 'words/' + words++, words: m.body });
    }
    if (m.work_head) { title = m.work_head; }
  }
  if (tail.id === 'live' && state.generating && !state.summarising) {
    for (const [index, draft] of (state.tool_previews || []).entries()) {
      // A hidden cat preview still occupies a slot in a batch of tool calls.
      entries.push({ key: 'tool/' + (tools + (draft.slot ?? index)), work: draft });
    }
  }
  if (!live && tail.answer_head) { title = tail.answer_head; }
  return { entries, title, final, busy: tail.id === 'live' };
}

function updateCorporateAnswer(turn, run, state, beforeSummary) {
  const key = turn.dataset.row;
  const model = corporateAnswer(run, state, beforeSummary);
  let card = turn.firstElementChild;
  if (!card || !card.classList.contains('work')) {
    card = corporateDisclosure(key, true);
    card.firstElementChild.append(make('span', 'wtitle'));
    turn.prepend(card);
  }
  const header = card.firstElementChild;
  header.classList.toggle('busy', model.busy);
  const label = header.lastElementChild;
  if (label.textContent !== model.title) { label.textContent = model.title; }
  const body = card.lastElementChild;
  const old = new Map(Array.from(body.children, node => [node.dataset.foldKey, node]));
  model.entries.forEach(function (entry, index) {
    const id = key + '/' + entry.key;
    const node = old.get(id) || corporateDisclosure(id, false);
    old.delete(id);
    const stamp = JSON.stringify(entry);
    if (node._activity !== stamp) {
      corporateEntry(node, entry);
      node._activity = stamp;
    }
    if (body.children[index] !== node) { body.insertBefore(node, body.children[index] || null); }
  });
  for (const stale of old.values()) { stale.remove(); }

  const final = model.final;
  const stamp = final && JSON.stringify([final.id, final.body, final.when, final.can_continue]);
  if (turn._final !== stamp) {
    turn.querySelectorAll(':scope > .answer-words, :scope > .stamp').forEach(node => node.remove());
    if (final) {
      const answer = make('div', 'plain answer-words');
      answer.dataset.mid = final.id;
      for (const part of blocks(final.body)) { answer.append(blockNode(part)); }
      turn.append(answer);
      const actions = actLine(final);
      if (actions) { turn.append(actions); }
    }
    turn._final = stamp;
  }
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
      updateCorporateAnswer(turn, items.slice(from, to), state, items[to]?.role === 'Summary');
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

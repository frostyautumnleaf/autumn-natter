// The tooltips of the page are drawn by the page. The tooltip of the computer is a grey box
// that answers to none of the colours of the chat, and in the window of the program it is not
// even themed. The page therefore takes the text a part carries in its `title` once: they move
// to `data-tip`, the `title` is removed for good, and `aria-label` keeps them for a screen
// reader. Nothing is put back - the text remaining in a `title` again let the grey box come up
// over the part, and the `mouseover` that would take them away does not come again until the
// pointer leaves the part and enters it, so a part used to be served its own tooltip and the
// box of the computer on alternate hovers.
//
// node tip-check.js AN_PAGE=web/page.html AN_FETCH=1 node tip-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

function stateOf() {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A picture the model can see' },
    chats: [{ id: 'c1', title: 'A picture the model can see', repo: '', when: 'Today 09:04',
      active: true, preview: 'What colour is this?' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'What colour is this?', reasoning: '', reasoning_ms: 0,
        attachments: [{ id: 'a1', name: 'swatch.png', mime: 'image/png', image: true,
          data: 'data:image/png;base64,iVBORw0KGgo=' }],
        tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Assistant', body: 'This image is orange.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
        seen: 'The model saw this image', when: '09:05' },
      { id: '3', role: 'Tool', body: 'The swatch, shown to the model.',
        tool: 'shown to the model', tool_name: 'view_image',
        attachments: [{ id: 'a2', name: 'swatch.png', mime: 'image/png', image: true }],
        mono: false, failed: false, when: '09:06' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

async function main() {
  let shell = process.env.AN_SHELL_BIN || '';
  if (!shell) {
    try {
      shell = require('child_process').execSync('find "' + (process.env.PUPPETEER_CACHE_DIR
        || process.env.HOME + '/.cache/puppeteer') + '" '
        + '-name chrome-headless-shell -type f | head -1').toString().trim();
    } catch (e) { shell = ''; }
  }
  const browser = await puppeteer.launch({
    headless: 'shell', executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', function (e) { errors.push(String(e)); });
  const state = stateOf();
  if (USE_FETCH) {
    await page.evaluateOnNewDocument(function (state) {
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
    }, state);
  } else {
    await page.evaluateOnNewDocument(function (state) {
      window.__TAURI__ = {
        core: {
          invoke: function (command) {
            if (command === 'get_state') { return Promise.resolve(state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            if (command === 'get_attachment') {
              return Promise.resolve({ name: 'swatch.png', mime: 'image/png',
                data: 'data:image/png;base64,iVBORw0KGgo=', text: '' });
            }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, state);
  }
  await page.setViewport({ width: 1100, height: 800 });
  await page.goto('file://' + path.resolve(PAGE));
  await new Promise(function (r) { setTimeout(r, 500); });

  // Rest the pointer on a part, wait out the short wait the computer would have waited too, and
  // read what the tooltip of the page reports. Then leave the part, rest on it again, and rest
  // on it a third time after a scroll put the tooltip away under the pointer: the part is
  // served the same way every time, and the text never return to the computer.
  const tip = await page.evaluate(async function () {
    function hover(node) {
      node.dispatchEvent(new MouseEvent('mouseover', { bubbles: true, cancelable: true }));
    }
    function rest(ms) { return new Promise(function (r) { setTimeout(r, ms || 500); }); }
    var send = document.getElementById('send');
    var out = { titles: [], empty: 0 };

    hover(send);
    await rest();
    var box = document.querySelector('.tip');
    out.holds = box ? box.textContent : '';
    out.shown = !!box && box.classList.contains('on');
    out.taken = !send.hasAttribute('title');
    out.kept = send.getAttribute('data-tip') || '';
    out.themed = box ? getComputedStyle(box).backgroundColor : '';
    out.insideWindow = box ? (box.getBoundingClientRect().top >= 0
      && box.getBoundingClientRect().right <= window.innerWidth) : false;

    // The pointer leaves, and returns: the text was taken for good, so the tooltip of the page
    // appears again rather than the grey box of the computer.
    send.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, cancelable: true }));
    await rest(60);
    out.hidden = !document.querySelector('.tip.on');
    hover(send);
    await rest();
    var again = document.querySelector('.tip.on');
    out.shownAgain = !!again && again.textContent === out.holds;

    // A scroll puts the tooltip away while the pointer still rests on the part. Nothing may be
    // in the `title` afterwards, for the `mouseover` that took the text away does not come
    // again until the pointer leaves and enters.
    el.thread.scrollTop += 1;
    window.dispatchEvent(new Event('scroll'));
    await rest(60);
    out.hiddenAfterScroll = !document.querySelector('.tip.on');
    out.titleAfterScroll = send.getAttribute('title');

    // The eye icon over an image the model saw is a part of the page like any other, and its
    // text is the reason the eye icon is there at all. It must not carry a word the computer
    // would read, for the box of the computer hides the very image it speaks of.
    var eye = document.querySelector('.seen');
    out.eyeFound = !!eye;
    if (eye) {
      hover(eye);
      await rest();
      var second = document.querySelector('.tip.on');
      out.eyeWords = second ? second.textContent : '';
      out.eyeShown = !!second && second.textContent.length > 0;
      for (var n = eye; n && n !== document.body; n = n.parentNode) {
        if (n.getAttribute && String(n.getAttribute('title') || '').trim()) { out.eyeTitled = true; }
      }
    }

    // Nothing anywhere carries text the computer could read. An empty `title` is skipped, for
    // it is how a part reports it wants no tooltip at all.
    var left = document.querySelectorAll('[title]');
    out.titles = Array.prototype.slice.call(left).map(function (one) {
      return (one.className || one.tagName) + '="' + one.getAttribute('title') + '"';
    });
    return out;
  });

  await browser.close();
  if (errors.length) { throw new Error('page faults: ' + errors.join(' | ')); }
  if (!tip.holds) { throw new Error('the send button carries no text to say'); }
  if (!tip.shown) { throw new Error('no tooltip of the page appeared'); }
  if (!tip.taken) { throw new Error('the title was left on the part under the tooltip'); }
  if (tip.kept !== tip.holds) {
    throw new Error('the text of the part was not where they are kept: ' +
      JSON.stringify(tip.kept) + ' against ' + JSON.stringify(tip.holds));
  }
  if (!tip.hidden) { throw new Error('the tooltip was on after the pointer left'); }
  if (!tip.shownAgain) {
    throw new Error('the part was hovered again and no tooltip of the page appeared, '
      + 'which is the error in the grey box taking over');
  }
  if (!tip.hiddenAfterScroll) { throw new Error('the tooltip was on after the conversation was scrolled'); }
  if (tip.titleAfterScroll !== null) {
    throw new Error('a scroll gave the text back to the part: ' + JSON.stringify(tip.titleAfterScroll));
  }
  if (tip.themed === 'rgba(0, 0, 0, 0)' || tip.themed === 'transparent') {
    throw new Error('the tooltip has no ground of its own');
  }
  if (!tip.insideWindow) { throw new Error('the tooltip was put outside the window'); }
  if (!tip.eyeFound) { throw new Error('the eye icon over an image the model saw is removed'); }
  if (!tip.eyeShown) { throw new Error('the eye icon over an image the model saw brought up no tooltip'); }
  if (tip.eyeTitled) { throw new Error('a word is where the computer reads it over the eye icon'); }
  if (tip.titles.length) {
    throw new Error('the computer has text to show over ' + tip.titles.length + ' part(s): '
      + tip.titles.slice(0, 3).join(', '));
  }
  console.log('the page draws its own tooltip out of the text of a part and takes those '
    + 'text for good, so the part is served the same tooltip the first time it is '
    + 'hovered and every time after, a scroll puts the tooltip away without giving the '
    + 'text back to the computer, nothing in the page carries text the computer could '
    + 'read, and the eye icon over an image the model saw speaks its reason with no grey box '
    + 'over the picture');
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });

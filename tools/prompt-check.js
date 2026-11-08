// A look at the Custom system prompt section of the Program tab, in both faces.
//
// The section holds the prompt of the user and the table of the pre-written
// prompts. The words of a prompt do not stand in the state of the page: the
// table is asked for, and the words of one prompt are asked for when the user
// opens it to read. This tool answers those two asks itself, so the section can
// be looked at with no engine behind it: the bridge of the page is not used.
//
//   node prompt-check.js            # both faces
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const FACES = [
  { name: 'window', file: path.resolve(__dirname, '..', 'frontend', 'index.html') },
  { name: 'browser', file: path.resolve(__dirname, '..', 'web', 'page.html') }
];

// What the check asks the page, with the two answers of the program standing in
// for the bridge. The marks stand on the second prompt alone.
const LOOK = function () {
  const TABLE = [
    { id: 'engineer', name: 'Software Engineer Writing Style', summary: 'Plain terms.' },
    { id: 'adhd', name: 'ADHD Rules', summary: 'The answer first.' }
  ];
  const WORDS = 'RULES\n1. Put the answer first.\n2. Cap every list at 5 items.';
  window.readPrompts = function (id) {
    if (!id) { return Promise.resolve({ lines: TABLE }); }
    const hit = TABLE.filter(function (one) { return one.id === id; })[0];
    return Promise.resolve(hit ? { id: hit.id, name: hit.name, words: WORDS } : { refused: 'no' });
  };
  const box = document.getElementById('fPromptLines');
  const field = document.getElementById('fSystemPrompt');
  if (!box || !field) { return { error: 'the section is not in the page' }; }
  field.value = 'Answer in short sentences.';
  buildPromptLines(['adhd']);
  return new Promise(function (done) {
    setTimeout(function () {
      const rows = box.querySelectorAll('.prompt-line');
      const marks = box.querySelectorAll('input[data-prompt]');
      const names = [];
      for (let i = 0; i < rows.length; i += 1) { names.push(rows[i].querySelector('.p-name').textContent); }
      const shown = {
        rows: rows.length,
        names: names,
        marked: Array.prototype.map.call(marks, function (m) { return m.checked ? m.dataset.prompt : ''; })
          .filter(function (one) { return one !== ''; }),
        userWords: field.value
      };
      // The marks are read back in the order of the table: the mark put on here
      // stands first, and the mark the settings gave stands after it.
      const first = box.querySelector('input[data-prompt="engineer"]');
      first.checked = true;
      shown.ids = checkedPromptIds();
      first.checked = false;
      const button = box.querySelector('button[data-prompt-view="adhd"]');
      if (!button) { shown.error = 'no button to view a prompt'; done(shown); return; }
      button.click();
      setTimeout(function () {
        const over = document.getElementById('promptOverlay');
        const body = document.getElementById('promptWords');
        shown.opened = over ? !over.classList.contains('hidden') : false;
        shown.title = (document.getElementById('promptTitle') || {}).textContent || '';
        shown.words = body ? body.textContent : '';
        shown.linesKept = body ? getComputedStyle(body).whiteSpace === 'pre-wrap' : false;
        document.getElementById('promptClose').click();
        shown.closed = over.classList.contains('hidden');
        done(shown);
      }, 120);
    }, 200);
  });
};

(async () => {
  const browser = await puppeteer.launch({ args: ['--no-sandbox'] });
  let bad = 0;
  for (const face of FACES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 900 });
    await page.goto('file://' + face.file);
    const out = await page.evaluate(LOOK);
    const well = out.rows === 2
      && out.names.join('|') === 'Software Engineer Writing Style|ADHD Rules'
      && out.marked.join(',') === 'adhd'
      && out.ids.join(',') === 'engineer,adhd'      && out.userWords === 'Answer in short sentences.'
      && out.opened === true
      && out.title === 'ADHD Rules'
      && out.words.indexOf('Put the answer first.') >= 0
      && out.linesKept === true
      && out.closed === true;
    if (!well) { bad += 1; }
    console.log(face.name + ': ' + (well ? 'well' : 'WRONG') + ' ' + JSON.stringify(out));
    await page.close();
  }
  await browser.close();
  process.exit(bad === 0 ? 0 : 1);
})();

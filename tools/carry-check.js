// Check the carry block of the Model tab: the whole of a model's settings as one
// piece of words, and the shapes of them the user keeps under names of their own.
// The clipboard is stood in for, first shut and then open, because it is shut to a
// page over the network and open to no two machines alike; the order the two ways in
// are read in is what is looked for. The program is stood in for by a list of presets
// kept in the check, which makes the name of a preset even as the program makes it.
// The page is loaded as it stands, with no bridge and no engine.
//
//   node carry-check.js
//   AN_PAGE=../web/page.html node carry-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE ? path.resolve(process.env.AN_PAGE) : PAGE;

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1180, height: 1100 });
  await page.goto('file://' + REMOTE);
  const out = await page.evaluate(async () => {
    var faults = [];
    var wait = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };
    var by = function (id) { return document.getElementById(id); };
    var noteBad = function (id) { return by(id).classList.contains('bad'); };
    var move = function (id) {
      by(id).dispatchEvent(new Event('input', { bubbles: true }));
      by(id).dispatchEvent(new Event('change', { bubbles: true }));
    };

    // The settings of the model that stands in the bar, and the two choices of the
    // tab that are read from the program rather than from the settings.
    var settings = {
      ctx_size: 8192, kv_index: 1, load_index: 0, spec_index: 0, ctx_checkpoints: 32,
      cache_ram: 8192, cache_prompt: true, no_context_shift: false, temperature: 0.8,
      top_p: 0.95, top_k: 40, min_p: 0.05, repeat_penalty: 1, presence_penalty: 0,
      max_tokens: -1, seed: -1, draft_model: '', draft_tokens: 4, parallel: 1,
      batch_size: 2048, ubatch_size: 512, vision: false, image_min_tokens: 0,
      image_max_tokens: 0, mmproj_cpu: false, reasoning_budget: -1,
      reasoning_message: '', tensor_parallel: false, chat_template: '',
      extra_args: '', send_default_args: true, vram_budget: 90, gpu_enabled: [false]
    };
    var gpus = [{ name: 'A card', memory_text: '12 GiB', memory_mb: 12288, is_offload: true }];
    var presets = [];      // the presets the program holds, as the file holds them
    var calls = [];        // every call of the carry block the page made
    var clipboard = null;  // the words the clipboard holds, or nothing when it is shut

    // The program, stood in for. The name of a preset is made even here, for the
    // field of the sheet is not the last word about a name.
    var answer = function (name, body) {
      if (name === 'model_settings') {
        return {
          name: 'The Long Answers', kind: 'q8_0', settings: JSON.parse(JSON.stringify(settings)),
          kinds: ['f16', 'q8_0', 'q4_k'], loads: ['auto', 'mmap'], specs: ['none', 'draft'],
          gpus: gpus, presets: JSON.parse(JSON.stringify(presets))
        };
      }
      if (name === 'preview_command') { return 'llama-server --model one.gguf'; }
      if (name === 'state') { return { rev: 0, unchanged: true, generating: false }; }
      if (name === 'model_preset') {
        calls.push(JSON.parse(JSON.stringify(body)));
        var made = String(body.name || '').trim().replace(/\s+/g, ' ');
        if (!made) { return { ok: false, status: 'A preset needs a name before it can be kept' }; }
        if (body.op === 'save') {
          var over = false;
          for (var i = 0; i < presets.length; i++) {
            if (presets[i].name === made) { presets[i].settings = body.settings; over = true; }
          }
          if (!over) { presets.push({ name: made, settings: body.settings }); }
          presets.sort(function (a, b) { return a.name < b.name ? -1 : (a.name > b.name ? 1 : 0); });
          return { ok: true, name: made, written_over: over };
        }
        if (body.op === 'remove') {
          var kept = [];
          for (var j = 0; j < presets.length; j++) {
            if (presets[j].name !== made) { kept.push(presets[j]); }
          }
          if (kept.length === presets.length) { return { ok: false, status: 'No preset stands under that name' }; }
          presets = kept;
          return { ok: true };
        }
        return { ok: false, status: 'Nothing is done by "' + body.op + '"' };
      }
      return { ok: true };
    };
    var route = function (where, body) {
      var clean = String(where).split('?')[0].replace(/^\/api\//, '');
      // The window asks for the settings of the model by the name of its bridge,
      // the page of the remote mode by the name of its small server.
      if (clean.indexOf('get_') === 0) { clean = clean.slice(4); }
      return answer(clean, body || {});
    };
    // The two bridges of the two faces. Both are stood in for, so the same check
    // runs on the window page and on the page of the remote mode. The window puts a
    // call of the carry block inside the envelope of the calls of the sheet.
    invoke = function (name, args) {
      if (name === 'action' && args && args.path) {
        return Promise.resolve(route(args.path, args.body || {}));
      }
      return Promise.resolve(route(name, args || {}));
    };
    ask = function (name, body) { return Promise.resolve(route(name, body || {})); };

    // The clipboard, stood in for: shut at the first, then open.
    clipRead = function () { return Promise.resolve(clipboard === null ? '' : clipboard); };
    clipWrite = function (text) { clipboard = (clipboard === null) ? null : text; return Promise.resolve(clipboard !== null); };

    // The Model tab comes forward with its model.
    cur = { model: { id: '/models/one.gguf', name: 'The Long Answers' }, dark: false };
    openSettings('model');
    await wait(80);
    if (by('mForm').style.display === 'none') { faults.push('the form of the model was put away'); }
    if (by('mCtx').value !== '8192') { faults.push('the context of the model is ' + by('mCtx').value); }
    if (by('mCards').querySelectorAll('input').length !== 1) {
      faults.push('the card of the machine was not drawn into the form');
    }

    // The block stands where the fields of the model stand, and its buttons are
    // known apart from the button that saves the sheet.
    ['mCopy', 'mPaste', 'mCarryBox', 'mPreset', 'mPresetName', 'mPresetSave',
      'mPresetLoad', 'mPresetRemove'].forEach(function (id) {
      if (!by(id)) { faults.push('the block holds no ' + id); }
      else if (!by(id).closest('#mCarry')) { faults.push(id + ' stands outside the block'); }
    });
    if (!by('mCarry').closest('#mForm')) { faults.push('the block stands outside the form of the model'); }

    // Nothing is kept yet: the menu says so, and the two buttons that work on a
    // chosen preset stand greyed.
    var lines = by('mPreset').options.length;
    if (lines !== 1) { faults.push('the empty menu holds ' + lines + ' lines and not its first line alone'); }
    if (by('mPreset').value !== '') { faults.push('the menu stands chosen with nothing kept'); }
    if (!by('mPresetLoad').disabled || !by('mPresetRemove').disabled) {
      faults.push('a button that works on a chosen preset stands free with nothing chosen');
    }

    // Copy: the words of the fields go into the box, and a line is written under
    // the buttons. Which of the two ways out took the words is the business of the
    // machine, so only the box and the line are looked for here.
    by('mCopy').click();
    await wait(40);
    if (by('mCarryBox').value.indexOf('"ctx_size": 8192') < 0) {
      faults.push('the box does not hold the words of the fields: ' + by('mCarryBox').value.slice(0, 60));
    }
    if (!by('mCarryNote').textContent) { faults.push('the copy wrote no line under the buttons'); }

    // Paste with the clipboard shut: the words of the box are read.
    by('mCtx').value = '4096';
    by('mExtra').value = '';
    by('mCarryBox').value = JSON.stringify({ ctx_size: 32768, extra_args: '--flash-attn' }, null, 2);
    by('mPaste').click();
    await wait(40);
    if (by('mCtx').value !== '32768') { faults.push('the words of the box were not pasted: ' + by('mCtx').value); }
    if (by('mExtra').value !== '--flash-attn') { faults.push('a field of the pasted words did not move'); }
    if (by('mTopP').value !== '0.95') { faults.push('a setting the words said nothing about was moved'); }
    if (mdlDirty !== true) { faults.push('the paste left the tab holding no words never written'); }
    if (noteBad('mCarryNote')) { faults.push('a paste that worked is told in the colour of a fault'); }

    // The wrong words are refused, and the fields stand where they stood.
    var before = by('mCtx').value;
    by('mCarryBox').value = 'the model of a friend of mine';
    by('mPaste').click();
    await wait(40);
    if (by('mCtx').value !== before) { faults.push('words that are no settings moved the fields'); }
    if (!noteBad('mCarryNote')) { faults.push('a paste of nothing is not told in the colour of a fault'); }

    // Paste with the clipboard open: what the clipboard holds stands before the
    // words of the box, which are of another model.
    clipboard = JSON.stringify({ ctx_size: 131072, temperature: 0.2 }, null, 2);
    clipRead = function () { return Promise.resolve(clipboard); };
    by('mPresetName').value = '   ';
    by('mPaste').click();
    await wait(40);
    if (by('mCtx').value !== '131072') { faults.push('the clipboard did not stand before the box: ' + by('mCtx').value); }
    if (by('mTemp').value !== '0.2') { faults.push('a field of the clipboard did not move'); }

    // Keep: the settings of the fields go to the program under the name of the
    // field, and the name the program kept with is the name of the menu.
    by('mPresetName').value = '   the   long   answers  ';
    by('mPresetSave').click();
    await wait(40);
    if (calls.length !== 1 || calls[0].op !== 'save') {
      faults.push('keeping asked for ' + JSON.stringify(calls[calls.length - 1] || null));
    }
    if (by('mPreset').value !== 'the long answers') {
      faults.push('the menu does not stand at ' + by('mPreset').value);
    }
    if (by('mPresetName').value !== 'the long answers') {
      faults.push('the field of the name does not hold the name the program kept with');
    }
    if (String(by('mPreset').options[1].text) !== 'the long answers') {
      faults.push('the menu does not show the name the program kept with');
    }
    if (noteBad('mPresetNote')) { faults.push('a preset that was kept is told as a fault'); }
    var keptAt = JSON.parse(JSON.stringify(presets[0].settings));
    if (String(keptAt.ctx_size) !== '131072') {
      faults.push('the shape that was kept is not the shape of the fields: ' + keptAt.ctx_size);
    }
    if (by('mPresetLoad').disabled || by('mPresetRemove').disabled) {
      faults.push('the two buttons that work on a chosen preset stand greyed with one chosen');
    }

    // A second name, and the list of the menu is in the order of the names.
    by('mPresetName').value = 'a short chat';
    by('mCtx').value = '4096';
    by('mPresetSave').click();
    await wait(40);
    var shown = [];
    for (var k = 1; k < by('mPreset').options.length; k++) { shown.push(by('mPreset').options[k].text); }
    if (shown.join(',') !== 'a short chat,the long answers') {
      faults.push('the menu stands in the order ' + shown.join(','));
    }

    // Keeping the same name again writes the shape over, and brings no second name
    // in beside it.
    by('mPresetName').value = 'a short chat';
    by('mCtx').value = '2048';
    by('mPresetSave').click();
    await wait(40);
    if (presets.length !== 2) { faults.push('keeping the same name again brought in another preset'); }
    for (var p = 0; p < presets.length; p++) {
      if (presets[p].name === 'a short chat' && String(presets[p].settings.ctx_size) !== '2048') {
        faults.push('the written over shape is ' + presets[p].settings.ctx_size);
      }
    }
    if (by('mPresetNote').textContent.indexOf('written over') < 0) {
      faults.push('a preset written over does not say so: ' + by('mPresetNote').textContent);
    }

    // A name of nothing at all is refused in the page, and asks nothing of the
    // program.
    var asks = calls.length;
    by('mPresetName').value = '   ';
    by('mPresetSave').click();
    await wait(40);
    if (calls.length !== asks) { faults.push('a name of nothing asked something of the program'); }
    if (!noteBad('mPresetNote')) { faults.push('a preset without a name is not told in the colour of a fault'); }

    // Load puts a shape into the fields, and the name of it into the field of the
    // name. The model is left alone until the sheet is saved.
    by('mPreset').value = 'the long answers';
    by('mPreset').dispatchEvent(new Event('change', { bubbles: true }));
    if (by('mPresetName').value !== 'the long answers') {
      faults.push('choosing a preset did not write its name into the field');
    }
    by('mCtx').value = '1';
    by('mPresetLoad').click();
    await wait(40);
    if (by('mCtx').value !== '131072') { faults.push('the loaded shape is not in the fields: ' + by('mCtx').value); }
    if (mdlDirty !== true) { faults.push('a load left the tab holding no words never written'); }

    // Throw away: the preset is out of the menu, and the two buttons that work on a
    // chosen preset stand greyed again.
    by('mPreset').value = 'the long answers';
    by('mPreset').dispatchEvent(new Event('change', { bubbles: true }));
    by('mPresetRemove').click();
    await wait(40);
    if (presets.length !== 1 || presets[0].name !== 'a short chat') {
      faults.push('the preset that was thrown away stands in the program still');
    }
    if (by('mPresetName').value !== '') { faults.push('the name of the thrown away preset stands in its field'); }
    if (!by('mPresetRemove').disabled) { faults.push('Throw away stands free with nothing chosen'); }
    if (noteBad('mPresetNote')) { faults.push('a preset that was thrown away is told as a fault'); }

    // The words of the box and the name of a preset are no settings of the model:
    // words moved in them leave the tab as it stood, and a field of the model does
    // not.
    mdlDirty = false;
    by('mCarryBox').value = 'words of their own';
    move('mCarryBox');
    by('mPresetName').value = 'a name of a preset';
    move('mPresetName');
    if (mdlDirty !== false) { faults.push('words of the block made the tab hold words never written'); }
    by('mCtx').value = '6144';
    move('mCtx');
    if (mdlDirty !== true) { faults.push('a field of the model left the tab holding nothing'); }

    // The block is put away with the form of the model, which is what happens when
    // no model stands in the bar.
    cur = { model: null, dark: false };
    mdlDirty = false;
    loadModel();
    await wait(40);
    if (by('mForm').style.display !== 'none') { faults.push('the block stands with no model in the bar'); }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('the carry block of the Model tab holds the whole of a model in one piece of words,');
  console.log('copies it into the box and on to the clipboard, pastes the clipboard or the box,');
  console.log('refuses words that are no settings, keeps a shape of the settings under a name of');
  console.log('the user\'s own, writes that shape over when the name is asked for again, loads it');
  console.log('into the fields, throws it away again, greys the two buttons that work on a chosen');
  console.log('preset while nothing is chosen, and leaves the tab unmarked while only its own');
  console.log('words are moved; nothing of it reaches a model until the sheet is saved');
})().catch(function (e) { console.error(e); process.exit(1); });

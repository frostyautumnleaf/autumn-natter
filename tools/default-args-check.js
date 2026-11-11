// Check the switch of the default arguments of the Model tab: where it stands,
// how it is filled from the settings of a model, what it does to the words the
// tab sends when it asks for the command line, and how it is carried about with
// the rest of the settings. The switch lets a server of another build, which
// knows none of the arguments of this program, be started with a command line of
// the user's own. The page is loaded as it stands, with no bridge and no engine,
// so the two faces are checked the same way and neither engine is run.
//
//   node default-args-check.js
//   AN_PAGE=../web/page.html node default-args-check.js
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
  const faults = await page.evaluate(async () => {
    var faults = [];
    var wait = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };
    var by = function (id) { return document.getElementById(id); };
    var move = function (id) {
      by(id).dispatchEvent(new Event('input', { bubbles: true }));
      by(id).dispatchEvent(new Event('change', { bubbles: true }));
    };

    // The settings of the model that stands in the bar. They hold no word for
    // the switch, as the settings file of a model written before the switch
    // existed holds none either.
    var settings = {
      ctx_size: 8192, kv_index: 1, load_index: 0, spec_index: 0, ctx_checkpoints: 32,
      cache_ram: 8192, cache_prompt: true, no_context_shift: false, temperature: 0.7,
      top_p: 0.95, top_k: 20, min_p: 0, repeat_penalty: 1, presence_penalty: 0,
      max_tokens: -1, seed: -1, draft_model: '', draft_tokens: 4, parallel: 1,
      batch_size: 512, ubatch_size: 128, vision: false, image_min_tokens: 0,
      image_max_tokens: 0, mmproj_cpu: false, reasoning_budget: -1,
      reasoning_message: '', tensor_parallel: false, chat_template: '',
      extra_args: '', vram_budget: 90, gpu_enabled: [true]
    };
    var gpus = [{ name: 'A card', memory_text: '12 GiB', memory_mb: 12288, is_offload: true }];
    var previews = [];   // every settings object the tab sent for the command line

    // The program, stood in for. The line the program would build is of no
    // interest here; what the tab sends with the ask is.
    var answer = function (name, body) {
      if (name === 'model_settings') {
        return {
          name: 'One', kind: 'q8_0', settings: JSON.parse(JSON.stringify(settings)),
          kinds: ['f16', 'q8_0'], loads: ['auto', 'mmap'], specs: ['none', 'draft'],
          gpus: gpus, presets: []
        };
      }
      if (name === 'preview_command') {
        previews.push((body && body.settings) ? body.settings : {});
        return 'llama-server --model one.gguf';
      }
      if (name === 'state') { return { rev: 0, unchanged: true, generating: false }; }
      return { ok: true };
    };
    var route = function (where, body) {
      var clean = String(where).split('?')[0].replace(/^\/api\//, '');
      // The window asks for the settings of a model by the name of its bridge,
      // the page of the remote mode by the name of its small server.
      if (clean.indexOf('get_') === 0) { clean = clean.slice(4); }
      return answer(clean, body || {});
    };
    invoke = function (name, args) {
      if (name === 'action' && args && args.path) {
        return Promise.resolve(route(args.path, args.body || {}));
      }
      return Promise.resolve(route(name, args || {}));
    };
    ask = function (name, body) { return Promise.resolve(route(name, body || {})); };

    // The Model tab comes forward with its model.
    cur = { model: { id: '/models/one.gguf', name: 'One' }, dark: false };
    openSettings('model');
    await wait(120);
    if (by('mForm').style.display === 'none') { faults.push('the form of the model was put away'); }

    // The switch stands in the form of the model, under the field of the extra
    // arguments, as a box of the sheet.
    if (!by('mDefaultArgs')) {
      faults.push('the tab holds no switch of the default arguments');
      return faults;
    }
    if (!by('mDefaultArgs').closest('#mForm')) {
      faults.push('the switch stands outside the form of the model');
    }
    if (by('mDefaultArgs').type !== 'checkbox') { faults.push('the switch is not a box'); }
    var under = by('mExtra').compareDocumentPosition(by('mDefaultArgs'));
    if (!(under & Node.DOCUMENT_POSITION_FOLLOWING)) {
      faults.push('the switch does not stand under the field of the extra arguments');
    }

    // A model that says nothing about the switch stands with it on, which is
    // where a new set of settings stands, and the fields read it as on.
    if (by('mDefaultArgs').checked !== true) {
      faults.push('the switch is off with a model that says nothing about it');
    }
    if (readModel().send_default_args !== true) {
      faults.push('the fields read the switch off with nothing said');
    }

    // The box is read back into the settings, and the ask for the command line
    // carries what the box stands at. That is the whole of the way the switch
    // reaches the engine: the engine builds the line from these words.
    by('mDefaultArgs').checked = false;
    move('mDefaultArgs');
    await wait(300);
    if (readModel().send_default_args !== false) {
      faults.push('the fields do not read the switch off');
    }
    var last = previews[previews.length - 1] || {};
    if (last.send_default_args !== false) {
      faults.push('the ask for the command line did not carry the switch off: '
        + JSON.stringify(last.send_default_args));
    }

    // The words of the tab hold the switch, so a copy of a whole model carries
    // it, and words put in by hand move the box again.
    if (modelWords().indexOf('send_default_args') < 0) {
      faults.push('the words of the tab do not carry the switch');
    }
    putInFields({ send_default_args: true });
    await wait(40);
    if (by('mDefaultArgs').checked !== true) {
      faults.push('words that set the switch on left the box off');
    }
    // Words that say nothing about the switch leave the box where it stands, for
    // they are read over the settings of the model and not in the place of them.
    putInFields({ ctx_size: 4096 });
    await wait(40);
    if (by('mDefaultArgs').checked !== true) {
      faults.push('words that said nothing about the switch moved the box');
    }
    if (by('mCtx').value !== '4096') { faults.push('the words of a paste did not move the context'); }

    // The box is filled again from the settings of the model, and a model that
    // says off stands off.
    mdl.settings.send_default_args = false;
    fillModelForm({});
    if (by('mDefaultArgs').checked !== false) {
      faults.push('a model whose settings say off is filled with the box on');
    }

    return faults;
  });
  await browser.close();
  const face = process.env.AN_PAGE ? 'the page of the remote mode' : 'the window page';
  if (!faults.length) {
    console.log(`ok: the switch of the default arguments stands right on ${face}`);
    return;
  }
  console.log(`faults on ${face}:`);
  faults.forEach(function (f) { console.log(' - ' + f); });
  process.exitCode = 1;
})();

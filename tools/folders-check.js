// Check the two folders of the Program tab that a user may point elsewhere: the
// directory for the files of the program, and the folder for the talks the model
// worked its way through. Both stand in the Folders section beside the places that
// are read out, both are filled from the settings, and both are read back into the
// words the tab sends when it asks for the settings to be written. Empty words mean
// the usual place, which is what a settings file written before the two fields
// existed holds. The page is loaded as it stands, with no bridge and no engine, so
// the two faces are checked the same way and neither engine is run.
//
//   node folders-check.js
//   AN_PAGE=../web/page.html node folders-check.js
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

    // The settings as the engine holds them, and the two places in use. The
    // settings name other folders than the ones in use, so that what is filled
    // into a field can be told from what is read out beside it.
    var settings = {
      llama_dir: '/llamacpp/build/bin', models_dir: '/models',
      repositories_dir: '', data_dir: '/spindle/program', contexts_dir: '/spindle/talks',
      remote_port: 8723, upnp_enabled: false, theme: 'light', dark_mode: false,
      model_id: '', model_settings: {}
    };
    var saved = [];   // every settings object the tab asked to be written

    // The two faces ask in their own words: the window names the command of the
    // bridge, the page of the remote mode names the road of its small server.
    var answer = function (name) {
      if (name === 'get_config' || name === 'config') {
        return JSON.parse(JSON.stringify(settings));
      }
      if (name === 'list_repositories' || name === 'repositories') { return []; }
      return { ok: true };
    };
    invoke = function (name, args) {
      if (name === 'save_config' && args && args.config) {
        saved.push(JSON.parse(JSON.stringify(args.config)));
      }
      return Promise.resolve(answer(name));
    };
    ask = function (name, body) {
      if (name === 'config' && body && body.config) {
        saved.push(JSON.parse(JSON.stringify(body.config)));
      }
      return Promise.resolve(answer(name));
    };

    // The state of the program: the places it is reading from this run.
    cur = { rev: 0, unchanged: true, generating: false, dark: false,
      data_dir: '/home/user/.config/autumn_natter',
      contexts_dir: '/home/user/.config/autumn_natter/contexts' };

    openSettings('program');
    await wait(200);

    // Both fields stand in the section of the folders, and the two places in use
    // are read out above them, so the difference between the two words is plain
    // to the user before anything is written.
    var pane = by('paneProgram');
    ['fDataDir', 'fContexts'].forEach(function (id) {
      if (!by(id)) { faults.push('the Program tab holds no field ' + id); return; }
      if (pane && !by(id).closest('#paneProgram')) { faults.push(id + ' stands outside the section of the program'); }
      if (by(id).type !== 'text') { faults.push(id + ' is not a field of words'); }
    });
    if (!by('fData') || !by('fContextsInUse')) {
      faults.push('the places in use are not read out in the section of the folders');
      return faults;
    }
    if (by('fData').textContent.indexOf('autumn_natter') < 0) {
      faults.push('the directory in use is not read out: ' + by('fData').textContent);
    }
    if (by('fContextsInUse').textContent.indexOf('contexts') < 0) {
      faults.push('the context folder in use is not read out: '
        + by('fContextsInUse').textContent);
    }
    // The fields are filled from the settings, which name other folders than the
    // ones in use.
    if (by('fDataDir').value !== '/spindle/program') {
      faults.push('the field of the data directory holds ' + JSON.stringify(by('fDataDir').value));
    }
    if (by('fContexts').value !== '/spindle/talks') {
      faults.push('the field of the context folder holds ' + JSON.stringify(by('fContexts').value));
    }

    // The fields are read back into the settings, and the ask that has the
    // settings written carries both words.
    by('fDataDir').value = '/disk2/program';
    move('fDataDir');
    by('fContexts').value = '/disk2/talks';
    move('fContexts');
    var back = readProgram();
    if (back.data_dir !== '/disk2/program') {
      faults.push('the fields do not read the data directory back: ' + JSON.stringify(back.data_dir));
    }
    if (back.contexts_dir !== '/disk2/talks') {
      faults.push('the fields do not read the context folder back: ' + JSON.stringify(back.contexts_dir));
    }
    // Saving the tab asks for the whole of the settings, both folders inside it.
    saveSheet();
    await wait(200);
    var last = saved[saved.length - 1] || {};
    if (last.data_dir !== '/disk2/program') {
      faults.push('the ask for the settings did not carry the data directory: '
        + JSON.stringify(last.data_dir));
    }
    if (last.contexts_dir !== '/disk2/talks') {
      faults.push('the ask for the settings did not carry the context folder: '
        + JSON.stringify(last.contexts_dir));
    }

    // Empty words are kept as they stand, for they are how the usual place is
    // asked for: nothing of the settings is filled in for the user.
    openSettings('program');
    await wait(200);
    by('fDataDir').value = '';
    move('fDataDir');
    by('fContexts').value = '   ';
    move('fContexts');
    var empty = readProgram();
    if (empty.data_dir !== '') {
      faults.push('empty words were filled in with something: ' + JSON.stringify(empty.data_dir));
    }
    if (empty.contexts_dir !== '   ') {
      faults.push('the words of the context folder were changed: ' + JSON.stringify(empty.contexts_dir));
    }
    return faults;
  });
  await browser.close();
  const face = process.env.AN_PAGE ? 'the page of the remote mode' : 'the window page';
  if (!faults.length) {
    console.log(`ok: the two folders of the Program tab stand and are read back on ${face}`);
    return;
  }
  console.log(`faults on ${face}:`);
  faults.forEach(function (f) { console.log(' - ' + f); });
  process.exitCode = 1;
})();

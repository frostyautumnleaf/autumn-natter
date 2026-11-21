// Look at the blocks of the markdown, as the chat cuts them and as the page puts them down. The
// page is loaded as it is now, with no bridge and no engine: the block cutter, the maker of the
// nodes, and the writer of an answer that is arriving are driven manually.
//
// Four items are checked. A table, a quote and a rule are cut into blocks of their own, and
// into the right ones. The nodes they make say the same text, in the same order, with the
// header where the markers stated it was, and no marker of the table, of the quote or of the
// rule left for the user. A link is created a link, with its text on the page and its address
// in it, and an address that is not one to follow is left as text. And an answer that grows a
// few characters at a time keeps every word of a table, of a quote and of a link on the page,
// so the writer that puts the text of the last block on again never loses one.
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE
  ? path.resolve(process.env.AN_PAGE)
  : path.join(__dirname, '..', 'frontend', 'index.html');

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto('file://' + PAGE);
  const faults = await page.evaluate(() => {
    var found = [];
    function say(fault) { found.push(fault); }

    // One message of the markdown, what the blocks of it should be, and what the page should
    // say of it.
    var TABLE = 'The columns remain as the markers say:\n\n'
      + '| file | lines | left |\n'
      + '| :--- | ---: | :-: |\n'
      + '| web.rs | 12 | 3 |\n'
      + '| page.html | 304 | 41 |\n\n'
      + 'The last row is the longest.';

    var QUOTE = 'The model answers:\n\n'
      + '> A quote of another voice.\n'
      + '> It goes on over lines.\n>\n'
      + '> - with a list inside\n'
      + '> - and one more thing\n'
      + '>\n'
      + '> ```js\n> var a = 1;\n> ```\n\n'
      + 'And text of its own after it.';

    var kindsOf = function (made) {
      return made.map(function (one) {
        return one.kind + (one.level || '') + (one.blocks ? '(' + kindsOf(one.blocks) + ')' : '');
      }).join(' ');
    };

    // 1. The blocks the markers are cut into.
    var table = blocks(TABLE);
    if (kindsOf(table) !== 'text table text') {
      say('a table did not come between two blocks of text: ' + kindsOf(table));
    }
    var one = table[1] || {};
    if (one.head.join(',') !== 'file,lines,left') {
      say('the head of the table is ' + JSON.stringify(one.head));
    }
    if ((one.align || []).join(',') !== 'l,r,c') {
      say('the columns are not as the colons stated: ' + JSON.stringify(one.align));
    }
    if ((one.rows || []).length !== 2 || ((one.rows || [])[1] || []).join(',') !== 'page.html,304,41') {
      say('the rows of the table are wrong: ' + JSON.stringify(one.rows));
    }

    var quote = blocks(QUOTE);
    if (kindsOf(quote) !== 'text quote(text list code) text') {
      say('a quote did not come between two blocks of text: ' + kindsOf(quote));
    }
    var inner = (quote[1] || {}).blocks || [];
    if (inner.length !== 3) {
      say('the quote holds ' + inner.length + ' blocks, not three');
    } else {
      if (inner[0].kind !== 'text' || inner[0].text.indexOf('\n') < 0) {
        say('the first text of the quote is not one block over two lines');
      }
      if (inner[1].kind !== 'list' || inner[1].items.length !== 2) {
        say('a list was not inside the quote');
      }
      if (inner[2].kind !== 'code' || inner[2].text.indexOf('var a = 1;') < 0) {
        say('a block of code was not inside the quote, or its markers came with it');
      }
    }

    // 2. The markers that must not be taken for a table.
    var RULE = 'Words above.\n\n---\n\nWords below.';
    if (kindsOf(blocks(RULE)).indexOf('table') >= 0) {
      say('three markers on their own were taken for a table: ' + kindsOf(blocks(RULE)));
    }
    var NOHEAD = 'a | b\nnot a delimiter\nc | d';
    if (kindsOf(blocks(NOHEAD)) !== 'text') {
      say('a line of walls with no line of dashes under it was taken for a table: '
        + kindsOf(blocks(NOHEAD)));
    }
    var NARROW = '| a | b |\n| - | - |\n| 1 |';
    var narrow = blocks(NARROW)[0] || {};
    if ((narrow.rows || []).join(',') !== '1,' || (narrow.width || 0) !== 2) {
      say('a row with a cell missing did not end up as wide as the table: '
        + JSON.stringify(narrow.rows) + ' of width ' + narrow.width);
    }
    var ESCAPED = '| a \\| b | c |\n| --- | --- |\n| x | y |';
    var esc = blocks(ESCAPED)[0] || {};
    if ((esc.head || []).join(',') !== 'a | b,c') {
      say('a pipe with a line before it was taken for a wall: ' + JSON.stringify(esc.head));
    }

    // 3. The nodes, and the text they say. The nodes are in the page, for a look at the style
    //    of an item that is not on the page reports nothing of the style it would take there.
    var stage = document.createElement('div');
    stage.className = 'turn t-ai';
    var words = document.createElement('div');
    words.className = 'plain';
    stage.appendChild(words);
    document.body.appendChild(stage);
    function said(made) {
      var box = document.createElement('div');
      for (var n = 0; n < made.length; n++) { box.appendChild(blockNode(made[n])); }
      words.appendChild(box);
      return box;
    }
    var host = said(table);
    var cellText = function (box) {
      var rows = box.querySelectorAll('table.mdtable tr'), out = [];
      for (var r = 0; r < rows.length; r++) {
        var cells = rows[r].children, line = [];
        for (var c = 0; c < cells.length; c++) { line.push(cells[c].textContent); }
        out.push(line.join(','));
      }
      return out.join(' / ');
    };
    if (cellText(host) !== 'file,lines,left / web.rs,12,3 / page.html,304,41') {
      say('the table on the page reports something else: ' + cellText(host));
    }
    var marks = host.querySelector('table.mdtable');
    if (!marks) { say('no table is on the page'); }
    var heads = host.querySelectorAll('table.mdtable thead th');
    if (heads.length !== 3 || heads[0].tagName !== 'TH') {
      say('the head of the table is not made of header cells: ' + heads.length);
    }
    var aligned = host.querySelector('table.mdtable tbody tr td:nth-child(2)');
    if (!aligned || window.getComputedStyle(aligned).textAlign !== 'right') {
      say('the second column is not to the right as its colon stated');
    }
    var centred = host.querySelector('table.mdtable tbody tr td:nth-child(3)');
    if (!centred || window.getComputedStyle(centred).textAlign !== 'center') {
      say('the third column is not in the middle as its colons stated');
    }
    if (host.textContent.indexOf('|') >= 0) {
      say('a wall of the table reached the user');
    }
    var quoted = said(blocks(QUOTE));
    if (quoted.querySelector('.quote .marks li').textContent !== 'with a list inside') {
      say('the list inside the quote lost its text');
    }
    if (/(^|\n)\s*>/.test(quoted.querySelector('.quote').textContent)) {
      say('the marker of the quote reached the user');
    }
    if (quoted.querySelectorAll('.quote').length !== 1) {
      say('the quote was created more than once');
    }
    // The text of the quote is dim, and the top bar of it is drawn: a quote that reads exactly
    // like the text about it is a quote nobody sees.
    var bar = window.getComputedStyle(quoted.querySelector('.quote')).borderLeftWidth;
    if (parseFloat(bar) < 2) {
      say('the quote remains without a rule of colour beside it');
    }
    var ground = window.getComputedStyle(host.querySelector('.tablewrap')).backgroundColor;
    if (ground === 'rgba(0, 0, 0, 0)' || ground === 'transparent') {
      say('the table has no ground of its own');
    }

    // A table of more columns than the chat is wide is moved sideways inside its frame, and
    // does not push the chat wider.
    var WIDE = '| one | two | three | four | five | six | seven | eight |\n'
      + '| --- | --- | --- | --- | --- | --- | --- | --- |\n'
      + '| a long word in every cell | a long word in every cell | a long word | '
      + 'a long word | a long word | a long word | a long word | a long word |';
    var chat = document.createElement('div');
    chat.className = 'plain';
    chat.style.width = '420px';
    stage.appendChild(chat);
    chat.appendChild(said(blocks(WIDE)));
    var frame = chat.querySelector('.tablewrap');
    if (!frame) { say('a wide table made no frame of its own'); }
    else {
      if (frame.getBoundingClientRect().width > 421) {
        say('a wide table pushed the chat wider than it is: '
          + Math.round(frame.getBoundingClientRect().width) + ' of 420');
      }
      // The text of a cell break to fit the chat, as the text of a block of code does, so a
      // table of many columns is read without moving sideways.
      if (frame.scrollWidth > frame.clientWidth) {
        say('a wide table remains wider than the frame it is in');
      }
    }
    // A table may be inside a quote, as anything else of the markdown may.
    var INQUOTE = '> | a | b |\n> | --- | --- |\n> | 1 | 2 |';
    if (kindsOf(blocks(INQUOTE)) !== 'quote(table)') {
      say('a table was not inside a quote: ' + kindsOf(blocks(INQUOTE)));
    }

    // 4. The rule across the page: three of one marker, with space between them.
    var RULED = 'Words above.\n\n---\n\nWords below.';
    if (kindsOf(blocks(RULED)) !== 'text rule text') {
      say('a rule did not come between two blocks of text: ' + kindsOf(blocks(RULED)));
    }
    var forms = ['---', '***', '___', '- - -', '* * *', '_ _ _', '----',
      ' - - - ', '***  '];
    for (var f = 0; f < forms.length; f++) {
      if (kindsOf(blocks(forms[f])) !== 'rule') {
        say('a rule of "' + forms[f] + '" was not read as one: ' + kindsOf(blocks(forms[f])));
      }
    }
    // Two markers are not a rule, and neither are markers with text about them.
    var NOT = ['--', '- -', '**', '__', '--- free', '***bold***', '- an item'];
    for (var g = 0; g < NOT.length; g++) {
      if (kindsOf(blocks(NOT[g])).indexOf('rule') >= 0) {
        say('"' + NOT[g] + '" was taken for a rule');
      }
    }
    // A line of dashes under a line of text is a rule, and not one block of text with the
    // dashes in it.
    var TIGHT = 'Words above.\n---\nWords below.';
    if (kindsOf(blocks(TIGHT)) !== 'text rule text') {
      say('a rule under a line of text was not read as one: ' + kindsOf(blocks(TIGHT)));
    }
    if (kindsOf(blocks('> ---\n> \n> Words')) !== 'quote(rule text)') {
      say('a rule was not inside a quote: ' + kindsOf(blocks('> ---\n> \n> Words')));
    }
    var ruleBox = said(blocks(RULED));
    var hr = ruleBox.querySelector('hr.mdrule');
    if (!hr) { say('no rule is on the page'); }
    else {
      if (hr.textContent !== '') { say('the rule on the page reports text'); }
      if (parseFloat(window.getComputedStyle(hr).borderTopWidth) < 1) {
        say('the rule on the page has no hairline of its own');
      }
      if (parseFloat(window.getComputedStyle(hr).marginTop) < 1) {
        say('the rule on the page remains hard against the text about it');
      }
      // The rule goes across the chat. It is not the thin marker that is between two parts of
      // the top bar, and the two must not share a name.
      if (hr.getBoundingClientRect().width < 200) {
        say('the rule on the page is ' + Math.round(hr.getBoundingClientRect().width)
          + ' wide, and not across the chat');
      }
    }

    // 5. A link is a link: its text on the page, and its address in it.
    var LINK = 'Read [Autumn Natter](https://example.com/autumn) and go on.';
    var linkBox = said(blocks(LINK));
    var link = linkBox.querySelector('a.link');
    if (!link) {
      say('a link made nothing the user may follow');
    } else {
      if (link.getAttribute('href') !== 'https://example.com/autumn') {
        say('the address of the link is ' + link.getAttribute('href'));
      }
      if (link.textContent !== 'Autumn Natter') {
        say('the words of the link are ' + JSON.stringify(link.textContent));
      }
      if (link.getAttribute('target') !== '_blank') {
        say('the link is not opened away from this page');
      }
      if ((link.getAttribute('rel') || '').indexOf('noopener') < 0) {
        say('the link does not hold the page it opens off');
      }
      if (!link.getAttribute('title')) {
        say('the address of the link is in no tip');
      }
    }
    if (linkBox.textContent !== 'Read Autumn Natter and go on.') {
      say('the text about the link are ' + JSON.stringify(linkBox.textContent));
    }
    if (/[[\]()]/.test(linkBox.textContent)) {
      say('a marker of the link reached the user');
    }
    if (link && window.getComputedStyle(link).color === window.getComputedStyle(words).color) {
      say('the link reads in the colour of the text about it');
    }
    if (link && window.getComputedStyle(link).textDecorationLine.indexOf('underline') < 0) {
      say('no rule is under the text of the link');
    }
    // A title between the address and the marker that ends it has expired there, and a pair of
    // markers of rounding belongs to the address.
    var titledBox = said(blocks('[words](https://example.com/x "A title")'));
    var titledLink = titledBox.querySelector('a.link');
    if (!titledLink || titledLink.getAttribute('href') !== 'https://example.com/x') {
      say('a title between the address and its end was not spent: '
        + (titledLink ? titledLink.getAttribute('href') : 'no link'));
    }
    var roundBox = said(blocks('[book](https://example.com/a_(b)) after'));
    var roundLink = roundBox.querySelector('a.link');
    if (!roundLink || roundLink.getAttribute('href') !== 'https://example.com/a_(b)') {
      say('markers of rounding inside an address were not kept: '
        + (roundLink ? roundLink.getAttribute('href') : 'no link'));
    }
    if (roundBox.textContent !== 'book after') {
      say('the text of the address came onto the page: ' + roundBox.textContent);
    }
    // An address that is not one to follow is left as text, and made nothing.
    var BAD = ['[run](javascript:alert(1))', '[next](./other.html)',
      '[up](../secret/one)', '[file](file:///etc/passwd)', '[data](data:text/html,x)',
      '[go](javascript\u0009:alert(1))'];
    for (var b2 = 0; b2 < BAD.length; b2++) {
      var badBox = said(blocks(BAD[b2]));
      if (badBox.querySelector('a')) {
        say('an address that is not one to follow was created a link: ' + BAD[b2]);
      }
    }
    var mailBox = said(blocks('Write to [the leaf](mailto:leaf@example.com).'));
    var mail = mailBox.querySelector('a.link');
    if (!mail || mail.getAttribute('href') !== 'mailto:leaf@example.com') {
      say('a mail was not made a link');
    }
    // An address between markers of less-than is a link of its own.
    var straightBox = said(blocks('See <https://example.com/x_(y)> and a < b.'));
    var straight = straightBox.querySelectorAll('a.link');
    if (straight.length !== 1) {
      say('an address between markers of less-than gave ' + straight.length + ' links');
    } else if (straight[0].textContent !== 'https://example.com/x_(y)') {
      say('the words of the address are ' + straight[0].textContent);
    }
    if (straightBox.textContent.indexOf('< b') < 0) {
      say('a marker of less-than that is no address was taken for a link');
    }
    // A link with no text of its own is reads by its address, and an escaped bracket starts
    // nothing. An image is read as text here as it always was.
    var emptyBox = said(blocks('[](https://example.com/none)'));
    if (!emptyBox.querySelector('a.link') || emptyBox.textContent !== 'https://example.com/none') {
      say('a link with no text of its own is not reads by its address: '
        + JSON.stringify(emptyBox.textContent));
    }
    var escapedBox = said(blocks('\\[not](https://example.com)'));
    if (escapedBox.querySelector('a')) { say('an escaped bracket was created a link'); }
    var pictureBox = said(blocks('A ![leaf](https://example.com/leaf.png) here'));
    if (pictureBox.querySelector('a')) { say('an image was created a link'); }
    if (pictureBox.textContent.indexOf('leaf (https://example.com/leaf.png)') < 0) {
      say('an image is not read as it was: ' + JSON.stringify(pictureBox.textContent));
    }
    // A link is in a heading, in an item of a list, and in a cell of a table, as it is now
    // anywhere else in the writing.
    var every = blocks('# A [heading](https://example.com/h)\n\n'
      + '- an [item](https://example.com/i)\n'
      + '2. a [number](https://example.com/n)\n\n'
      + '| file | where |\n| --- | --- |\n'
      + '| one | [a cell](https://example.com/c) |\n');
    var everyBox = said(every);
    var inHead = everyBox.querySelector('.h a.link');
    var inItems = everyBox.querySelectorAll('ul.marks a.link, ol.marks a.link');
    var inCell = everyBox.querySelector('table.mdtable a.link');
    if (!inHead || inHead.textContent !== 'heading') {
      say('no link is in a heading');
    }
    if (inItems.length !== 2) {
      say('a list holds ' + inItems.length + ' links, and not two');
    }
    if (!inCell || inCell.textContent !== 'a cell') {
      say('no link is in a cell of a table');
    }
    if (everyBox.textContent.indexOf('[') >= 0) {
      say('a marker of a link reached the user');
    }

    // 6. An answer that grows a few characters at a time. Whatever the markers say, the page
    //    must say the same text.
    var RULES = 'Words before.\n\n---\n\nWords after, and a [link](https://example.com/on) too.';
    var LINKS = 'First [Autumn Natter](https://example.com/autumn), then words that go on '
      + 'after the link, and then <https://example.com/straight> and a '
      + '[second](https://example.com/two "two") link.\n\n'
      + '- an item with a [link](https://example.com/item) in it\n'
      + '- an item that goes on and on\n\n'
      + '# A heading with a [link](https://example.com/head)\n\n'
      + '| file | where |\n| --- | --- |\n'
      + '| [one](https://example.com/cell) | a cell |\n\n'
      + '> A quote with a [link](https://example.com/quote) and a rule.\n>\n> ---\n'
      + '> Words after the rule inside the quote.';
    var samples = [TABLE, QUOTE,
      '| one | two |\n| --- | --- |\n| a',
      '> only a quote, and nothing after it',
      '> | a | b |\n> | --- | --- |\n> | 1 | 2 |',
      'Words.\n\n| a | b |\n| :--- | --- |\n| 1 | 2 |\n\n- a list after the table\n',
      '| file | where |\n| --- | --- |\n| [page.html](https://example.com/page) | the page |',
      'A list with a link in the first item:\n\n- [one](https://example.com/one)\n- two',
      '> ## A heading inside a quote\n>\n> Words after the heading.',
      RULES, LINKS];
    for (var s = 0; s < samples.length; s++) {
      var text = samples[s];
      var state = { shape: '' };
      var box = words.appendChild(document.createElement('div'));
      for (var n = 1; n <= text.length; n += 3) {
        var part = text.slice(0, n);
        writeBlocks(box, state, part);
        var want = said(blocks(part));
        if (box.textContent !== want.textContent) {
          say('at ' + n + ' characters the page lost text of the answer ' + s + '\n  page: '
            + JSON.stringify(box.textContent) + '\n  want: ' + JSON.stringify(want.textContent));
          break;
        }
        // A link that arrived must stay a link while the text after it arrive, and a rule must
        // stay a rule.
        var pageLinks = box.querySelectorAll('a.link').length;
        var modelLinks = 0;
        var countLinks = function (made) {
          made.forEach(function (one) {
            var words = one.text || '';
            if (one.items) {
              words = one.items.map(function (i) { return i.text; }).join('\n');
            }
            if (one.kind === 'table') {
              words = one.head.join('\n') + '\n' + one.rows.map(function (r) { return r.join('\n'); }).join('\n');
            }
            inlineParts(words).forEach(function (part) { if (part.href) { modelLinks += 1; } });
            if (one.blocks) { countLinks(one.blocks); }
          });
        };
        countLinks(blocks(part));
        if (pageLinks !== modelLinks) {
          say('at ' + n + ' characters the page holds ' + pageLinks + ' links and the marks say '
            + modelLinks + ' (sample ' + s + ')');
          break;
        }
        var pageRules = box.querySelectorAll('hr.mdrule').length;
        var modelRules = 0;
        var countRules = function (made) {
          made.forEach(function (one) {
            if (one.kind === 'rule') { modelRules += 1; }
            if (one.blocks) { countRules(one.blocks); }
          });
        };
        countRules(blocks(part));
        if (pageRules !== modelRules) {
          say('at ' + n + ' characters the page holds ' + pageRules + ' rules and the marks say '
            + modelRules + ' (sample ' + s + ')');
          break;
        }
        var pageRows = box.querySelectorAll('table.mdtable tr').length;
        // The header of a table is a row of its own, whether or not the rows under it have come
        // yet; and a table may be inside a quote.
        var modelRows = 0;
        var countRows = function (made) {
          made.forEach(function (one) {
            if (one.kind === 'table') { modelRows += 1 + one.rows.length; }
            if (one.blocks) { countRows(one.blocks); }
          });
        };
        countRows(blocks(part));
        if (pageRows !== modelRows) {
          say('at ' + n + ' characters the table on the page has ' + pageRows
            + ' rows and the marks say ' + modelRows + ' (sample ' + s + ')');
          break;
        }
      }
      // All of it at the last, as the answer ends.
      writeBlocks(box, state, text);
      var whole = said(blocks(text));
      if (box.textContent !== whole.textContent) {
        say('the answer ' + s + ' ended with other text than the markers say');
      }
      // Every link the markers name is on the page, with its address, and no marker of the
      // writing is left for the user.
      var ends = [];
      var collect = function (made) {
        made.forEach(function (one) {
          var words = one.text || '';
          if (one.items) { words = one.items.map(function (i) { return i.text; }).join('\n'); }
          if (one.kind === 'table') {
            words = one.head.join('\n') + one.rows.map(function (r) { return r.join('\n'); }).join('\n');
          }
          inlineParts(words).forEach(function (part) { if (part.href) { ends.push(part.href); } });
          if (one.blocks) { collect(one.blocks); }
        });
      };
      collect(blocks(text));
      var stood = [];
      var links = box.querySelectorAll('a.link');
      for (var L = 0; L < links.length; L++) { stood.push(links[L].getAttribute('href')); }
      if (stood.join(' ') !== ends.join(' ')) {
        say('the answer ' + s + ' ended with the links ' + stood.join(' ') + ' and not '
          + ends.join(' '));
      }
      if (box.textContent.indexOf('](') >= 0) {
        say('the answer ' + s + ' ended with the markers of a link on the page');
      }
    }
    return found;
  });
  await browser.close();
  if (faults.length) { console.log('FAULTS\n' + faults.slice(0, 10).join('\n')); process.exit(1); }
  console.log('a table, a quote and a rule are cut into blocks of their own, a link is created a '
    + 'link with its address in it, and every word of them is kept while the answer is written');
})();

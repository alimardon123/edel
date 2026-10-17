// Renders each .board in review.html to NAME.jpg next to it, at twice the
// size: `node render.js` for all, `node render.js desk-dark` for some.
const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 }, deviceScaleFactor: 2 });
  await page.goto('file://' + __dirname + '/review.html');
  await page.waitForTimeout(600);
  const ids = process.argv.slice(2);
  const all = await page.$$eval('.board', els => els.map(e => e.id));
  for (const id of (ids.length ? ids : all)) {
    const el = await page.$('#' + id);
    await el.screenshot({ path: __dirname + '/' + id + '.jpg', type: 'jpeg', quality: 88 });
    console.log('wrote', id + '.jpg');
  }
  await browser.close();
})();

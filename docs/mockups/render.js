// Renders each mockup in edel-mockups.html to NAME.jpg next to it:
// `node render.js` for all, `node render.js classic tablet` for some.
// Needs Playwright with Chromium, and Inter installed for the text.
const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 }, deviceScaleFactor: 1 });
  await page.goto('file://' + __dirname + '/edel-mockups.html');
  await page.waitForTimeout(400);
  const ids = process.argv.slice(2);
  const all = await page.$$eval('.screen', els => els.map(e => e.id));
  for (const id of (ids.length ? ids : all)) {
    const el = await page.$('#' + id);
    await el.screenshot({ path: __dirname + '/' + id + '.jpg', type: 'jpeg', quality: 90 });
    console.log('wrote', id + '.jpg');
  }
  await browser.close();
})();

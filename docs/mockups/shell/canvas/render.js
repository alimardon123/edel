// Renders the boards of a page here to ../ID.jpg, one picture per element
// with an id at the top level of the body: `node render.js lock.html`.
// Needs Playwright with Chromium, and Inter installed for the text.
const { chromium } = require('playwright');
(async () => {
  const file = process.argv[2];
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
  await page.goto('file://' + __dirname + '/' + file);
  await page.waitForTimeout(400);
  const ids = await page.$$eval('body > [id]', els => els.map(e => e.id));
  for (const id of ids) {
    const el = await page.$('#' + id);
    await el.screenshot({ path: __dirname + '/../' + id + '.jpg', type: 'jpeg', quality: 85 });
    console.log('wrote', id + '.jpg');
  }
  await browser.close();
})();

import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const site = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(new URL('../../frontend/package.json', import.meta.url));
const puppeteer = require('puppeteer');

test('导航在中英文、窄屏和桌面上可访问', async () => {
  const server = createServer(async (req, res) => {
    const name = req.url === '/' ? 'index.html' : req.url.slice(1);
    if (name !== 'index.html') {
      res.writeHead(404).end();
      return;
    }
    res.setHeader('Content-Type', 'text/html');
    res.end(await readFile(join(site, name)));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await puppeteer.launch({ headless: true, executablePath: process.env.CHROME_BIN || await puppeteer.executablePath(), args: ['--no-sandbox'] });
    const url = `http://127.0.0.1:${server.address().port}/`;
    for (const lang of ['en', 'zh']) for (const width of [320, 390, 761, 1440]) {
      const page = await browser.newPage();
      await page.setViewport({ width, height: 900 });
      await page.goto(url);
      await page.evaluate(value => applyLang(value), lang);
      const state = await page.evaluate(() => ({
        overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
        main: document.querySelectorAll('main').length,
        nav: document.querySelector('nav').offsetHeight,
        desktopLinks: [...document.querySelectorAll('nav>.wrap>.links a')].filter(link => link.getClientRects().length).length,
        menu: !!document.querySelector('.mobile-menu summary').getClientRects().length,
        binarySize: document.querySelector('.facts div:last-child b').textContent.replace(/\s/g, ' ').trim(),
        languageHeight: document.querySelector('.lang').offsetHeight,
      }));
      assert.equal(state.overflow, false, `${lang} ${width}: 横向溢出`);
      assert.equal(state.main, 1);
      assert.equal(state.binarySize, '50 MB', 'v1.0.1 静态二进制大小');
      assert.equal(state.languageHeight, width <= 760 ? 44 : 34, `${lang} ${width}: 语言按钮文字换行`);
      assert.equal(state.desktopLinks, width <= 760 ? 0 : 5);
      assert.equal(state.menu, width <= 760);
      if (width <= 760) {
        await page.click('.mobile-menu summary');
        assert.equal(await page.$$eval('.mobile-menu .links a', links => links.filter(link => link.getClientRects().length).length), 5);
        await page.keyboard.press('Escape');
        assert.equal(await page.$eval('.mobile-menu', menu => menu.open), false);
        await page.click('.mobile-menu summary');
        await page.click('.mobile-menu .links a[href="#how"]');
        assert.equal(await page.$eval('.mobile-menu', menu => menu.open), false);
        await page.waitForFunction(() => Math.abs(document.querySelector('#how').getBoundingClientRect().top - 80) < 20);
      }
      await page.close();
    }
    const page = await browser.newPage();
    await page.goto(url);
    await page.keyboard.press('Tab');
    assert.equal(await page.$eval(':focus', element => element.className), 'skip-link');
    await page.keyboard.press('Enter');
    assert.equal(await page.$eval(':focus', element => element.id), 'main');
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    assert.equal(await page.$eval('html', element => getComputedStyle(element).scrollBehavior), 'auto');
    await page.evaluate(() => { applyLang('en'); Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async () => { throw Error('denied'); } } }); });
    await page.click('.copy');
    await page.waitForFunction(() => document.querySelector('.copy-status').textContent.includes('Select the command'));
    assert.equal(await page.$eval('.copy', element => element.textContent), 'Copy');
    await page.click('#langBtn');
    assert.match(await page.$eval('.copy-status', element => element.textContent), /手动复制/);
    await page.close();
  } finally {
    if (browser) await browser.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});

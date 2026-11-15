#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..');
const browserDistDir = path.join(repoRoot, 'dist', 'psalm-stage', 'browser');
const websiteOutDir = path.join(repoRoot, 'website', 'src', 'assets', 'screenshots');
const docsOutDir = path.join(repoRoot, 'docs', 'assets', 'screenshots');

const MIME_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.json': 'application/json; charset=utf-8',
  '.woff2': 'font/woff2',
};

function ensureFrontendBuilt() {
  if (!fs.existsSync(path.join(browserDistDir, 'index.html'))) {
    console.log('==> Building Angular frontend (dist/psalm-stage/browser)...');
    execFileSync('npm', ['run', 'build'], { cwd: repoRoot, stdio: 'inherit' });
  }
}

function startStaticServer(rootDir) {
  return new Promise((resolve, reject) => {
    const server = http.createServer((req, res) => {
      const reqPath = (req.url || '/').split('?')[0];
      const relPath = reqPath === '/' ? 'index.html' : reqPath.replace(/^\/+/, '');
      let filePath = path.join(rootDir, relPath);
      if (!filePath.startsWith(rootDir) || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
        filePath = path.join(rootDir, 'index.html');
      }
      const ext = path.extname(filePath).toLowerCase();
      res.writeHead(200, { 'Content-Type': MIME_TYPES[ext] || 'application/octet-stream' });
      fs.createReadStream(filePath).pipe(res);
    });
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      resolve({ server, port: addr.port });
    });
  });
}

async function loadPlaywright() {
  const pwEntry = path.join(repoRoot, 'website', 'node_modules', 'playwright', 'index.mjs');
  if (!fs.existsSync(pwEntry)) {
    throw new Error('Playwright is not installed in website/node_modules. Run `cd website && npm install` first.');
  }
  return import(pathToFileURL(pwEntry).href);
}

async function launchBrowser(chromium) {
  try {
    return await chromium.launch({ headless: true });
  } catch (err) {
    const macChrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
    if (fs.existsSync(macChrome)) {
      console.log('==> Using system Google Chrome for headless screenshot capture...');
      return chromium.launch({ executablePath: macChrome, headless: true });
    }
    throw err;
  }
}

async function saveShot(page, name) {
  const destWebsite = path.join(websiteOutDir, name);
  const destDocs = path.join(docsOutDir, name);
  await page.waitForTimeout(350);
  await page.screenshot({ path: destWebsite, fullPage: false });
  fs.copyFileSync(destWebsite, destDocs);
  console.log(`==> Captured ${name}`);
}

async function main() {
  ensureFrontendBuilt();
  fs.mkdirSync(websiteOutDir, { recursive: true });
  fs.mkdirSync(docsOutDir, { recursive: true });

  const { chromium } = await loadPlaywright();
  const { server, port } = await startStaticServer(browserDistDir);
  const browser = await launchBrowser(chromium);

  try {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 900 },
      deviceScaleFactor: 2,
      colorScheme: 'dark',
    });
    const page = await context.newPage();

    // Suppress update check network call variance in screenshots
    await page.route('https://api.github.com/**', (route) => route.abort());

    await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: 'networkidle' });
    await page.getByText('PSALM STAGE').first().waitFor({ state: 'visible', timeout: 15000 });

    // 1. Live Stage
    await saveShot(page, 'live-stage.png');

    // 2. Tonic Pad Studio modal
    const tonicBtn = page.getByTestId('open-tonic-studio-btn');
    await tonicBtn.click();
    const tonicModal = page.getByTestId('tonic-pad-studio-modal');
    await tonicModal.waitFor({ state: 'visible', timeout: 5000 });
    await saveShot(page, 'tonic-pad-studio.png');
    await tonicModal.getByRole('button', { name: 'Done', exact: true }).click();
    await tonicModal.waitFor({ state: 'hidden', timeout: 5000 });

    // 3. Setlist Studio
    await page.getByRole('button', { name: 'Setlist Studio', exact: true }).click();
    await saveShot(page, 'setlist-studio.png');

    // 4. Loop Studio
    await page.getByTestId('workspace-tab-loops').click();
    await saveShot(page, 'loop-studio.png');

    // 5. Import Workbench
    await page.getByRole('button', { name: 'Import Workbench', exact: true }).click();
    await saveShot(page, 'import-workbench.png');
  } finally {
    await browser.close();
    server.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

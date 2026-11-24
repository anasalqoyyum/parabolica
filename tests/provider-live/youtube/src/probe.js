import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';
import { probeConfig, waitFor } from './config.js';
import { serveProbePage } from './server.js';

export async function runProbe({
  page,
  screenshotDirectory,
  videoId,
  wait = waitFor,
  playbackObservationMs = 4000,
  sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
}) {
  await page.goto(`/?video=${encodeURIComponent(videoId)}`, { waitUntil: 'load' });
  await page.bringToFront();
  try {
    await wait(() => page.evaluate(() => window.spike?.ready === true), { timeout: 45_000 });
  } catch (error) {
    throw new Error('The IFrame player did not become ready', { cause: error });
  }
  const command = async (name, ...args) => page.evaluate(
    (commandName, commandArgs) => window.spike.player[commandName](...commandArgs),
    name,
    args,
  );
  await command('playVideo');
  try {
    await wait(() => page.evaluate(() => window.spike.playing));
  } catch (error) {
    throw new Error('The player did not start playing', { cause: error });
  }
  await sleep(playbackObservationMs);
  const playback = await page.evaluate(() => ({
    durationSeconds: window.spike.player.getDuration(),
    positionSeconds: window.spike.player.getCurrentTime(),
  }));
  if (playback.positionSeconds < 3) {
    throw new Error('The player reported playing but its position did not advance');
  }
  await command('pauseVideo');
  try {
    await wait(() => page.evaluate(() => window.spike.paused));
  } catch (error) {
    throw new Error('The player did not pause', { cause: error });
  }
  await command('seekTo', 10, true);
  try {
    await wait(() => page.evaluate(
      () => Math.abs(window.spike.player.getCurrentTime() - 10) <= 1,
    ));
  } catch (error) {
    throw new Error('The player did not seek to 10 seconds', { cause: error });
  }
  const result = await page.evaluate(() => ({
    errors: window.spike.errors,
    positionAfterSeekSeconds: window.spike.player.getCurrentTime(),
    states: window.spike.states,
  }));
  if (result.errors.length > 0) {
    throw new Error(`The player reported error codes: ${result.errors.join(', ')}`);
  }
  if (screenshotDirectory) {
    await mkdir(screenshotDirectory, { recursive: true });
    const timestamp = new Date().toISOString().replaceAll(/[:.]/g, '-');
    await page.screenshot({
      path: path.join(screenshotDirectory, `parabolica-youtube-iframe-${timestamp}.png`),
    });
  }
  return { ...result, playback };
}

const ALLOWED_PROTOCOLS = new Set(['data:', 'https:']);

export function allowRequest(origin, localOrigin) {
  return origin === localOrigin || ALLOWED_PROTOCOLS.has(new URL(origin).protocol);
}

async function main() {
  const config = probeConfig();
  const profile = await mkdtemp(path.join(tmpdir(), 'parabolica-youtube-'));
  const server = await serveProbePage();
  let browser;
  try {
    browser = await puppeteer.launch({
      executablePath: config.chromePath,
      headless: false,
      userDataDir: profile,
      defaultViewport: { width: 900, height: 800 },
      args: ['--no-first-run', '--no-default-browser-check'],
    });
    const page = await browser.newPage();
    await page.setRequestInterception(true);
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (allowRequest(url.origin, server.origin)) {
        request.continue();
      } else {
        request.abort('blockedbyclient');
      }
    });
    const originalGoto = page.goto.bind(page);
    page.goto = (target, options) => originalGoto(`${server.origin}${target}`, options);
    const result = await runProbe({ browser, page, ...config });
    const evidence = {
      schemaVersion: 1,
      command: 'pnpm --dir tests/provider-live/youtube probe',
      source: {
        base: '754984d2f8dad70dc664e6a8a4c85ce85a91faa2',
        change: 'Issue #15 browser-route spike; the PR identifies the final source commit.',
      },
      environment: {
        browser: execFileSync(config.chromePath, ['--version'], { encoding: 'utf8' }).trim(),
        platform: `${process.platform} ${process.arch}`,
      },
      limitations: [
        'Public embed playback only; no signed-in YouTube Music account was used.',
        'Player state and position do not prove audible output.',
        'This does not verify Windows WebView2, native PCM or output-device control.',
      ],
      route: 'YouTube IFrame Player API in visible Chrome',
      testItem: { videoId: config.videoId },
      ...result,
    };
    const output = fileURLToPath(new URL('../evidence/local-public-iframe.json', import.meta.url));
    await mkdir(path.dirname(output), { recursive: true });
    await writeFile(output, `${JSON.stringify(evidence, null, 2)}\n`, { mode: 0o600 });
    console.log(JSON.stringify(evidence, null, 2));
    console.log(`Evidence: ${output}`);
  } finally {
    await browser?.close();
    await server.close();
    await rm(profile, { force: true, recursive: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}

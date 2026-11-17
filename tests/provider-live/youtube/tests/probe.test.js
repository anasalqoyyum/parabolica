import assert from 'node:assert/strict';
import { get } from 'node:http';
import test from 'node:test';
import { DEFAULT_VIDEO_ID } from '../src/config.js';
import { allowRequest, runProbe } from '../src/probe.js';
import { serveProbePage } from '../src/server.js';

const getPage = (url) => new Promise((resolve, reject) => {
  get(url, (response) => {
    let body = '';
    response.on('data', (chunk) => body += chunk);
    response.on('end', () => resolve({ body, status: response.statusCode }));
  }).on('error', reject);
});

test('loopback server serves only the probe document and closes cleanly', async () => {
  const server = await serveProbePage();
  try {
    const page = await getPage(`${server.origin}/`);
    assert.equal(page.status, 200);
    assert.match(page.body, /onYouTubeIframeAPIReady/);
    assert.equal((await getPage(`${server.origin}/src/probe.js`)).status, 404);
  } finally {
    await server.close();
  }
});

test('request filter permits HTTPS dependencies and the loopback document only', () => {
  const local = 'http://127.0.0.1:3000';
  assert.equal(allowRequest(local, local), true);
  assert.equal(allowRequest('https://www.youtube.com', local), true);
  assert.equal(allowRequest('https://i.ytimg.com', local), true);
  assert.equal(allowRequest('http://169.254.169.254', local), false);
  assert.equal(allowRequest('file:///home/user/.config/google-chrome', local), false);
});

function fakePage({ position = 4.1, errors = [], pauseWorks = true } = {}) {
  const state = {
    errors,
    player: {
      getCurrentTime: () => state.position,
      getDuration: () => 1343.661,
      pauseVideo: () => { if (pauseWorks) state.paused = true; },
      playVideo: () => { state.playing = true; },
      seekTo: (seconds) => { state.position = seconds; },
    },
    position,
    ready: true,
    states: [{ state: 1 }, { state: 2 }],
  };
  return {
    async bringToFront() {},
    evaluate: async (fn, ...args) => {
      globalThis.window = { spike: state };
      return fn(...args);
    },
    async goto() {},
    state,
  };
}

test('structured command sequence checks play, pause and seek', async () => {
  const page = fakePage();
  const result = await runProbe({ page, sleep: async () => {}, videoId: DEFAULT_VIDEO_ID });
  assert.equal(result.playback.positionSeconds, 4.1);
  assert.equal(result.positionAfterSeekSeconds, 10);
  assert.equal(page.state.playing, true);
  assert.equal(page.state.paused, true);
});

test('probe rejects a playing state whose position does not advance', async () => {
  await assert.rejects(
    runProbe({ page: fakePage({ position: 0 }), sleep: async () => {}, videoId: DEFAULT_VIDEO_ID }),
    /position did not advance/,
  );
});

test('probe rejects reported player errors', async () => {
  await assert.rejects(
    runProbe({ page: fakePage({ errors: [101] }), sleep: async () => {}, videoId: DEFAULT_VIDEO_ID }),
    /error codes: 101/,
  );
});

test('probe reports which player operation failed', async () => {
  const page = fakePage({ pauseWorks: false });
  await assert.rejects(
    runProbe({
      page,
      sleep: async () => {},
      videoId: DEFAULT_VIDEO_ID,
      wait: async (read) => {
        if (!(await read())) throw new Error('test timeout');
      },
    }),
    /did not pause/,
  );
});

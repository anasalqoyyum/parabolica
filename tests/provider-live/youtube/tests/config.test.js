import assert from 'node:assert/strict';
import test from 'node:test';
import { DEFAULT_VIDEO_ID, probeConfig, validateVideoId, waitFor } from '../src/config.js';

test('video ID accepts the documented default and explicit 11-character IDs', () => {
  assert.equal(probeConfig({}).videoId, DEFAULT_VIDEO_ID);
  assert.equal(probeConfig({ YOUTUBE_VIDEO_ID: 'aqz-KE-bpKQ' }).videoId, 'aqz-KE-bpKQ');
});

test('video ID rejects URLs, query parameters and arbitrary strings', () => {
  for (const value of [
    'https://www.youtube.com/watch?v=M7lc1UVf-VE',
    'M7lc1UVf-VE?secret=value',
    'short',
    'M7lc1UVf-VE extra',
    '',
  ]) {
    assert.throws(() => validateVideoId(value), /11-character YouTube video ID/);
  }
});

test('configuration does not require or return credentials', () => {
  assert.deepEqual(probeConfig({ YOUTUBE_TOKEN: 'do-not-read-this' }), {
    chromePath: '/usr/local/bin/google-chrome',
    screenshotDirectory: null,
    videoId: DEFAULT_VIDEO_ID,
  });
});

test('waitFor returns when the condition succeeds', async () => {
  let calls = 0;
  await waitFor(() => ++calls === 2, { delay: 0 });
  assert.equal(calls, 2);
});

test('waitFor has a bounded timeout', async () => {
  let now = 0;
  await assert.rejects(
    waitFor(() => false, {
      delay: 10,
      now: () => now,
      sleep: async () => { now += 10; },
      timeout: 25,
    }),
    /Timed out/,
  );
});

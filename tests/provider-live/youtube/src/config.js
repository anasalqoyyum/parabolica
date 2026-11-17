export const DEFAULT_VIDEO_ID = 'M7lc1UVf-VE';

export function validateVideoId(value) {
  if (typeof value !== 'string' || !/^[\w-]{11}$/.test(value)) {
    throw new Error('YOUTUBE_VIDEO_ID must be an 11-character YouTube video ID, not a URL.');
  }
  return value;
}

export function probeConfig(environment = process.env) {
  return {
    chromePath: environment.CHROME_PATH || '/usr/local/bin/google-chrome',
    screenshotDirectory: environment.PARABOLICA_SCREENSHOT_DIR || null,
    videoId: validateVideoId(environment.YOUTUBE_VIDEO_ID || DEFAULT_VIDEO_ID),
  };
}

export async function waitFor(read, {
  delay = 100,
  now = Date.now,
  sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
  timeout = 15_000,
} = {}) {
  const deadline = now() + timeout;
  for (;;) {
    if (await read()) return;
    if (now() >= deadline) throw new Error('Timed out waiting for the player state');
    await sleep(delay);
  }
}

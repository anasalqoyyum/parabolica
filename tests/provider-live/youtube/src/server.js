import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';

export async function serveProbePage() {
  const html = await readFile(new URL('./page.html', import.meta.url), 'utf8');
  const server = createServer((request, response) => {
    if (request.url === '/favicon.ico') {
      response.writeHead(204).end();
    } else if (request.url === '/' || request.url?.startsWith('/?')) {
      response.writeHead(200, {
        'Content-Type': 'text/html; charset=utf-8',
        'Content-Security-Policy': "default-src 'self' https: data:; script-src 'self' 'unsafe-inline' https:; frame-src https:; img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; connect-src 'self' https:;",
      }).end(html);
    } else {
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address();
  return {
    close: () => new Promise((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve());
      server.closeAllConnections();
    }),
    origin: `http://127.0.0.1:${port}`,
  };
}

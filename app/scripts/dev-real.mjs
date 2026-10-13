// Runs the UI in a browser against the real core: builds and starts vistaforge-devserver, then
// Vite, which forwards /api to it. Open http://localhost:1420/?backend=http
//
//   npm run dev:real -- --catalog /path/to/catalog.json
import { spawn } from 'node:child_process';

const shell = process.platform === 'win32';
const server = spawn(
  'cargo',
  ['run', '--release', '-p', 'vistaforge-bridge', '--features', 'devserver', '--bin', 'vistaforge-devserver', '--', ...process.argv.slice(2)],
  { stdio: 'inherit', shell },
);
const vite = spawn('npx', ['vite'], { stdio: 'inherit', shell });
console.log('UI with the real core: http://localhost:1420/?backend=http');

const stop = () => {
  server.kill();
  vite.kill();
};
process.on('SIGINT', stop);
server.on('exit', stop);
vite.on('exit', stop);

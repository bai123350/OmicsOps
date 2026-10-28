const { spawn } = require('node:child_process');
const os = require('node:os');
const path = require('node:path');

function buildEnvironment(env, root, home) {
  const clean = { ...env };
  for (const name of Object.keys(clean)) {
    if (/^(VITE_|TAURI_)/i.test(name) || /api_?key|token|secret|password|credential|private_?key/i.test(name)) delete clean[name];
  }
  delete clean.RUSTFLAGS;
  delete clean.CARGO_BUILD_TARGET;
  const mappings = [
    [home, '/build-home'],
    [env.CARGO_HOME || path.join(home, '.cargo'), '/cargo'],
    [env.RUSTUP_HOME || path.join(home, '.rustup'), '/rustup'],
    [root, '/omicsops'],
  ];
  const flags = [];
  for (const [source, destination] of mappings) {
    for (const variant of new Set([source, source.replaceAll('\\', '/')])) {
      flags.push(`--remap-path-prefix=${variant}=${destination}`);
    }
  }
  // Replace developer flags: release builds must not re-enable debug paths.
  clean.CARGO_ENCODED_RUSTFLAGS = flags.join('\x1f');
  clean.CARGO_PROFILE_RELEASE_DEBUG = '0';
  clean.CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS = 'false';
  clean.CARGO_PROFILE_RELEASE_STRIP = 'symbols';
  clean.CARGO_TARGET_DIR = path.join(root, 'target');
  return clean;
}

function nativeTarget(args) {
  let target;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--target' && !target && args[i + 1]) target = args[++i];
    else if (args[i] === '--' && args[i + 1] === '--locked' && i + 2 === args.length) break;
    else throw new Error('Supported release options: --target <native Rust target> and -- --locked');
  }
  if (target !== undefined && !['x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-apple-darwin'].includes(target)) {
    throw new Error('Unsupported desktop release target');
  }
  return target;
}

async function runBuild({ root, home, env, args, run = runCommand }) {
  const target = nativeTarget(args);
  const clean = buildEnvironment(env, root, home);
  const python = env.OMICSOPS_BUILD_PYTHON || 'python';
  const audit = ['-B', path.join(root, 'scripts/check_release_privacy.py'), '--root', root];
  const tauri = path.join(root, 'node_modules/@tauri-apps/cli/tauri.js');
  const targetArgs = target ? ['--target', target] : [];
  const options = { cwd: root, env: clean };
  await run(python, audit, options);
  await run(process.execPath, [tauri, 'build', '--no-bundle', ...targetArgs, '--', '--locked'], options);
  const windows = target ? target.endsWith('-windows-msvc') : process.platform === 'win32';
  const binary = path.join(root, 'target', ...(target ? [target] : []), 'release', `omicsops-desktop${windows ? '.exe' : ''}`);
  await run(python, [...audit, '--dist', path.join(root, 'dist'), '--binary', binary], options);
  await run(process.execPath, [tauri, 'bundle', ...targetArgs], options);
}

function runCommand(command, args, options) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { ...options, stdio: 'inherit', shell: false });
    child.on('error', () => reject(new Error('Cannot start build tool; install Python 3.12+ and dependencies, or set OMICSOPS_BUILD_PYTHON')));
    child.on('close', code => code === 0 ? resolve() : reject(new Error(`Desktop release step failed (exit ${code})`)));
  });
}

if (require.main === module) {
  runBuild({ root: path.resolve(__dirname, '..'), home: os.homedir(), env: process.env, args: process.argv.slice(2) })
    .catch(error => { console.error(error.message); process.exitCode = 1; });
}

module.exports = { buildEnvironment, runBuild };

const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const desktop = require('./build_desktop.cjs');
const { buildEnvironment } = desktop;

test('release environment removes frontend injections and remaps compiler paths with spaces', () => {
  const input = { PATH: 'toolchain', VITE_API_KEY: 'example-test-only', TAURI_SIGNING_PRIVATE_KEY: 'example-test-only',
    RUSTFLAGS: '-C debug-assertions=yes', CARGO_HOME: '/home/test user/.cargo', RUSTUP_HOME: '/home/test user/.rustup' };
  const output = buildEnvironment(input, '/work/private project', '/home/test user');
  assert.equal(output.PATH, 'toolchain');
  assert.equal(output.VITE_API_KEY, undefined);
  assert.equal(output.TAURI_SIGNING_PRIVATE_KEY, undefined);
  assert.equal(output.RUSTFLAGS, undefined);
  assert.equal(output.CARGO_PROFILE_RELEASE_DEBUG, '0');
  assert.equal(output.CARGO_PROFILE_RELEASE_STRIP, 'symbols');
  const args = output.CARGO_ENCODED_RUSTFLAGS.split('\x1f');
  assert.ok(args.includes('--remap-path-prefix=/work/private project=/omicsops'));
  assert.ok(args.includes('--remap-path-prefix=/home/test user=/build-home'));
  assert.ok(args.includes('--remap-path-prefix=/home/test user/.cargo=/cargo'));
  assert.ok(!args.includes('debug-assertions=yes'));
  assert.equal(input.VITE_API_KEY, 'example-test-only');
});

test('an audit failure prevents installer creation', async () => {
  const phases = [];
  await assert.rejects(() => desktop.runBuild({ root: '/workspace', home: '/home/builder', env: {}, args: [],
    run: async (_command, args) => {
      phases.push(args);
      if (args.includes('--binary')) throw new Error('privacy rejection');
    }
  }), /privacy rejection/);
  assert.equal(phases.length, 3);
  assert.ok(phases[1].includes('--no-bundle'));
  assert.ok(!phases.some(args => args.includes('bundle')));
});

test('a successful audit permits packaging the same native target', async () => {
  const phases = [];
  await desktop.runBuild({ root: '/workspace', home: '/home/builder', env: {},
    args: ['--target', 'aarch64-apple-darwin', '--', '--locked'],
    run: async (_command, args) => { phases.push(args); }
  });
  assert.equal(phases.length, 4);
  assert.ok(phases[2].includes(path.join('/workspace', 'target/aarch64-apple-darwin/release/omicsops-desktop')));
  assert.deepEqual(phases[3].slice(1), ['bundle', '--target', 'aarch64-apple-darwin']);
});

test('debug or custom configuration cannot bypass release checks', async () => {
  for (const args of [['--debug'], ['--config', 'personal.json'], ['--target'], ['--target', 'invalid']]) {
    let invoked = false;
    await assert.rejects(() => desktop.runBuild({ root: '/workspace', home: '/home/builder', env: {}, args,
      run: async () => { invoked = true; }
    }));
    assert.equal(invoked, false);
  }
});

test('frontend builds cannot embed dotenv or prefixed process secrets', async () => {
  const { build, loadConfigFromFile } = await import('vite');
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'omicsops-env-test-'));
  const names = ['VITE_BUILD_TEST_SECRET', 'TAURI_BUILD_TEST_SECRET'];
  const previous = names.map(name => process.env[name]);
  try {
    names.forEach(name => { process.env[name] = 'private-process-sentinel-for-test'; });
    await fs.writeFile(path.join(root, '.env.production'), 'VITE_FILE_SECRET=private-dotenv-sentinel-for-test\n');
    await fs.writeFile(path.join(root, 'index.html'), '<script type="module" src="/main.js"></script>');
    await fs.writeFile(path.join(root, 'main.js'), 'window.values = [import.meta.env.VITE_BUILD_TEST_SECRET, import.meta.env.TAURI_BUILD_TEST_SECRET, import.meta.env.VITE_FILE_SECRET];');
    const loaded = await loadConfigFromFile({ command: 'build', mode: 'production' }, path.resolve(__dirname, '../vite.config.ts'));
    const result = await build({ ...loaded.config, configFile: false, root, logLevel: 'silent', build: { write: false } });
    const code = result.output.filter(item => item.type === 'chunk').map(item => item.code).join('\n');
    assert.ok(!code.includes('private-process-sentinel-for-test'), 'process secret leaked into frontend');
    assert.ok(!code.includes('private-dotenv-sentinel-for-test'), 'dotenv secret leaked into frontend');
  } finally {
    names.forEach((name, i) => { if (previous[i] === undefined) delete process.env[name]; else process.env[name] = previous[i]; });
    await fs.rm(root, { recursive: true, force: true });
  }
});

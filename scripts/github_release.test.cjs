const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const publishDraft = require('./github_release.cjs');

const VERSION = '0.1.0';
const TAG = 'v0.1.0';
const COMMIT = 'a'.repeat(40);
const OTHER_COMMIT = 'b'.repeat(40);
const REPO = { owner: 'research-lab', repo: 'OmicsOps' };
const INSTALLERS = [
  'OmicsOps_0.1.0_x64-setup.exe',
  'OmicsOps_0.1.0_x64_en-US.msi',
  'OmicsOps_0.1.0_amd64.deb',
  'OmicsOps_0.1.0_amd64.AppImage',
  'OmicsOps_0.1.0_aarch64.dmg',
  'OmicsOps_0.1.0_x64.dmg',
];
const ALL_ASSETS = [...INSTALLERS, 'release-manifest.json', 'SHA256SUMS.txt'].sort();
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const apiError = (status) => Object.assign(new Error(`GitHub ${status}`), { status });

async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'omicsops-publisher-'));
  t.after(async () => {
    assert.equal(path.dirname(root), path.resolve(os.tmpdir()));
    await fs.rm(root, { recursive: true, force: true });
  });
  const assetDir = path.join(root, 'assets');
  await fs.mkdir(assetDir);
  const files = new Map(INSTALLERS.map((name) => [name, Buffer.from(`installer bytes: ${name}`)]));
  const targets = [
    ['windows-x64', INSTALLERS.slice(0, 2)],
    ['linux-x64', INSTALLERS.slice(2, 4)],
    ['macos-arm64', INSTALLERS.slice(4, 5)],
    ['macos-x64', INSTALLERS.slice(5, 6)],
  ].map(([target, names]) => ({
    schema_version: 1, version: VERSION, commit: COMMIT, target,
    files: names.map((name) => ({ name, size: files.get(name).length, sha256: digest(files.get(name)) })),
  }));
  files.set('release-manifest.json', Buffer.from(JSON.stringify({
    schema_version: 1, version: VERSION, commit: COMMIT, targets,
  })));
  files.set('SHA256SUMS.txt', Buffer.from([...files.keys()].sort()
    .map((name) => `${digest(files.get(name))}  ${name}\n`).join('')));
  for (const [name, bytes] of files) await fs.writeFile(path.join(assetDir, name), bytes);
  return { assetDir, files, root };
}

function mockGithub(options = {}) {
  const state = {
    release: options.absent ? null : {
      id: 73, tag_name: TAG, draft: true, prerelease: true, target_commitish: COMMIT,
      name: 'Manually edited title', body: 'Reviewed notes',
      html_url: 'https://github.com/research-lab/OmicsOps/releases/tag/v0.1.0',
      upload_url: 'https://uploads.github.com/repos/research-lab/OmicsOps/releases/73/assets{?name,label}',
      ...options.release,
    },
    assets: (options.assets ?? []).map((name, index) => ({ id: index + 100, name, size: 10, state: 'uploaded' })),
    tag: { type: 'commit', sha: COMMIT },
    tagObjects: {},
    requests: [], mutations: [], uploaded: [], refReads: 0, releaseReads: 0,
  };
  function request(method, args) {
    assert.equal(args.owner, REPO.owner);
    assert.equal(args.repo, REPO.repo);
    state.requests.push({ method, args });
  }
  const git = {
    async getRef(args) {
      request('getRef', args);
      assert.equal(args.ref, `tags/${TAG}`);
      state.refReads += 1;
      await options.onRef?.(state);
      if (options.refError) throw options.refError;
      return { data: { ref: `refs/tags/${TAG}`, object: { ...state.tag } } };
    },
    async getTag(args) {
      request('getTag', args);
      const object = state.tagObjects[args.tag_sha];
      assert.ok(object, 'Must request an observed tag object');
      return { data: { sha: args.tag_sha, tag: TAG, object: { ...object } } };
    },
  };
  const repos = {
    async getReleaseByTag(args) {
      request('getReleaseByTag', args);
      assert.equal(args.tag, TAG);
      if (options.lookupError) throw options.lookupError;
      if (!state.release) throw apiError(404);
      return { data: { ...state.release } };
    },
    async listReleases(args) {
      request('listReleases', args);
      assert.equal(args.per_page, 100);
      if (options.listError) throw options.listError;
      return { data: options.listedReleases ?? (state.release ? [{ ...state.release }] : []) };
    },
    async getRelease(args) {
      request('getRelease', args);
      assert.equal(args.release_id, 73);
      state.releaseReads += 1;
      await options.onRelease?.(state);
      if (options.releaseError) throw options.releaseError;
      return { data: { ...state.release } };
    },
    async createRelease(args) {
      request('createRelease', args);
      state.mutations.push({ method: 'createRelease', args });
      state.release = {
        id: 73, html_url: 'https://github.com/research-lab/OmicsOps/releases/73',
        upload_url: 'https://uploads.github.com/repos/research-lab/OmicsOps/releases/73/assets{?name,label}',
        ...args,
      };
      return { data: { ...state.release } };
    },
    async listReleaseAssets(args) {
      request('listReleaseAssets', args);
      assert.equal(args.release_id, 73);
      assert.equal(args.per_page, 100);
      return { data: state.assets.map((asset) => ({ ...asset })) };
    },
    async deleteReleaseAsset(args) {
      request('deleteReleaseAsset', args);
      assert.ok(state.assets.some((asset) => asset.id === args.asset_id));
      state.mutations.push({ method: 'deleteReleaseAsset', args });
      state.assets = state.assets.filter((asset) => asset.id !== args.asset_id);
      await options.afterDelete?.(state);
      return { status: 204 };
    },
    async uploadReleaseAsset(args) {
      request('uploadReleaseAsset', args);
      assert.equal(args.release_id, 73);
      assert.ok(Buffer.isBuffer(args.data));
      assert.equal(args.headers['content-length'], args.data.length);
      state.mutations.push({ method: 'uploadReleaseAsset', args });
      state.uploaded.push({ name: args.name, data: Buffer.from(args.data) });
      state.assets.push({ id: 1000 + state.uploaded.length, name: args.name, size: args.data.length, state: 'uploaded' });
      await options.afterUpload?.(state);
      return { data: state.assets.at(-1) };
    },
  };
  const github = {
    rest: { git, repos },
    async paginate(method, args) {
      assert.ok(method === repos.listReleaseAssets || method === repos.listReleases);
      state.requests.push({ method: 'paginate', args });
      return (await method(args)).data;
    },
  };
  return { github, state };
}

function run(assetDir, github, overrides = {}) {
  return publishDraft({
    github, context: { repo: REPO }, core: { info() {} }, tag: TAG, version: VERSION,
    commit: COMMIT, assetDir, body: 'Unsigned Windows packages. Experimental Linux/macOS.', ...overrides,
  });
}

test('refuses a published release before changing any assets', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ release: { draft: false }, assets: ALL_ASSETS });
  await assert.rejects(run(assetDir, github), /draft|published/i);
  assert.deepEqual(state.mutations, []);
});

test('refuses a draft targeting a different commit or a branch name', async (t) => {
  const { assetDir } = await fixture(t);
  for (const target_commitish of [OTHER_COMMIT, 'main']) {
    const { github, state } = mockGithub({ release: { target_commitish } });
    await assert.rejects(run(assetDir, github), /commit/i);
    assert.deepEqual(state.mutations, []);
  }
});

test('missing and moved tags cannot create a release', async (t) => {
  const { assetDir } = await fixture(t);
  for (const options of [{ refError: apiError(404) }, { onRef: (state) => { state.tag.sha = OTHER_COMMIT; } }]) {
    const { github, state } = mockGithub({ absent: true, ...options });
    await assert.rejects(run(assetDir, github), /404|tag|commit/i);
    assert.deepEqual(state.mutations, []);
  }
});

test('tag movement after release lookup prevents draft creation', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ absent: true, onRef: (state) => {
    if (state.refReads === 2) state.tag.sha = OTHER_COMMIT;
  } });
  await assert.rejects(run(assetDir, github), /tag|commit/i);
  assert.deepEqual(state.mutations, []);
});

test('release lookup authentication and server failures are not treated as absent', async (t) => {
  const { assetDir } = await fixture(t);
  for (const status of [401, 403, 429, 500]) {
    const failure = apiError(status);
    const { github, state } = mockGithub({ absent: true, lookupError: failure });
    await assert.rejects(run(assetDir, github), (error) => error === failure);
    assert.deepEqual(state.mutations, []);
  }
});

test('creates a draft prerelease pinned to a lightweight tag commit and uploads exactly validated bytes', async (t) => {
  const { assetDir, files } = await fixture(t);
  const { github, state } = mockGithub({ absent: true });
  const result = await run(assetDir, github);
  const creation = state.mutations.find((entry) => entry.method === 'createRelease').args;
  assert.equal(creation.tag_name, TAG);
  assert.equal(creation.name, TAG);
  assert.equal(creation.target_commitish, COMMIT);
  assert.equal(creation.draft, true);
  assert.equal(creation.prerelease, true);
  assert.equal(creation.make_latest, 'false');
  assert.match(creation.body, /Unsigned Windows/);
  assert.equal(result.id, 73);
  assert.equal(result.draft, true);
  assert.equal(state.mutations.filter((entry) => entry.method === 'createRelease').length, 1);
  assert.deepEqual(state.uploaded.map((asset) => asset.name).sort(), ALL_ASSETS);
  for (const asset of state.uploaded) assert.deepEqual(asset.data, files.get(asset.name));
  assert.ok(state.requests.some((entry) => entry.method === 'paginate'));
});

test('new draft uses the versioned release notes title', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ absent: true });
  const title = 'v0.1.0: First preview · 初始预览';
  await run(assetDir, github, { title });
  const creation = state.mutations.find(entry => entry.method === 'createRelease').args;
  assert.equal(creation.name, title);
  assert.equal(creation.draft, true);
});

test('invalid or misleading release titles fail before GitHub access', async (t) => {
  const { assetDir } = await fixture(t);
  for (const title of ['', 'Preview', 'v0.1.00: Wrong version', 'v0.1.0-beta.1: Wrong version',
    'v0.1.0\nInjected title', 'v0.1.0: line\u2028break', null, 123]) {
    const { github, state } = mockGithub({ absent: true });
    await assert.rejects(run(assetDir, github, { title }), /title/i);
    assert.deepEqual(state.requests, []);
    assert.deepEqual(state.mutations, []);
  }
});

test('peels nested annotated tags to the expected commit', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub();
  state.tag = { type: 'tag', sha: 'c'.repeat(40) };
  state.tagObjects['c'.repeat(40)] = { type: 'tag', sha: 'd'.repeat(40) };
  state.tagObjects['d'.repeat(40)] = { type: 'commit', sha: COMMIT };
  await run(assetDir, github);
  assert.equal(state.uploaded.length, 8);
  assert.ok(state.requests.some((entry) => entry.method === 'getTag'));
});

test('rejects annotated tags pointing at another commit or a tree and bounds tag cycles', async (t) => {
  const { assetDir } = await fixture(t);
  for (const object of [
    { type: 'commit', sha: OTHER_COMMIT },
    { type: 'tree', sha: COMMIT },
    { type: 'tag', sha: 'c'.repeat(40) },
  ]) {
    const { github, state } = mockGithub();
    state.tag = { type: 'tag', sha: 'c'.repeat(40) };
    state.tagObjects['c'.repeat(40)] = object;
    await assert.rejects(run(assetDir, github), /tag|commit/i);
    assert.deepEqual(state.mutations, []);
    assert.ok(state.requests.filter((entry) => entry.method === 'getTag').length <= 10);
  }
});

test('rerun replaces only known assets while preserving manually edited draft title and notes', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ assets: ALL_ASSETS });
  await run(assetDir, github, { title: 'v0.1.0: Updated source title', body: 'Updated source notes' });
  await run(assetDir, github);
  assert.equal(state.release.name, 'Manually edited title');
  assert.equal(state.release.body, 'Reviewed notes');
  assert.equal(state.mutations.filter((entry) => entry.method === 'createRelease').length, 0);
  assert.equal(state.mutations.filter((entry) => entry.method === 'deleteReleaseAsset').length, 16);
  assert.deepEqual(state.assets.map((asset) => asset.name).sort(), ALL_ASSETS);
});

test('finds an existing draft through paginated releases when tag lookup only exposes published releases', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ assets: ALL_ASSETS, lookupError: apiError(404) });
  await run(assetDir, github);
  assert.equal(state.mutations.filter((entry) => entry.method === 'createRelease').length, 0);
  assert.equal(state.release.name, 'Manually edited title');
  assert.equal(state.release.body, 'Reviewed notes');
  assert.equal(state.uploaded.length, 8);
  assert.ok(state.requests.some((entry) => entry.method === 'listReleases'));
});

test('draft fallback lookup failures and ambiguous matches cannot create releases', async (t) => {
  const { assetDir } = await fixture(t);
  const failure = apiError(403);
  const failed = mockGithub({ lookupError: apiError(404), listError: failure });
  await assert.rejects(run(assetDir, failed.github), (error) => error === failure);
  assert.deepEqual(failed.state.mutations, []);

  const ambiguous = mockGithub({ lookupError: apiError(404), listedReleases: [
    { id: 73, tag_name: TAG, draft: true, target_commitish: COMMIT },
    { id: 74, tag_name: TAG, draft: true, target_commitish: COMMIT },
  ] });
  await assert.rejects(run(assetDir, ambiguous.github), /multiple|ambiguous|duplicate/i);
  assert.deepEqual(ambiguous.state.mutations, []);
});

test('unexpected remote assets cause failure without deleting user files', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ assets: [...ALL_ASSETS, 'manual-review.pdf'] });
  await assert.rejects(run(assetDir, github), /unexpected|unknown|unrecognized/i);
  assert.deepEqual(state.mutations, []);
});

test('publication during uploads stops before the next mutation', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ afterUpload: (state) => { state.release.draft = false; } });
  await assert.rejects(run(assetDir, github), /draft|published/i);
  assert.equal(state.uploaded.length, 1);
  assert.equal(state.mutations.length, 1);
});

test('publication after deleting an old asset stops its replacement upload', async (t) => {
  const { assetDir } = await fixture(t);
  const { github, state } = mockGithub({ assets: ALL_ASSETS, afterDelete: (state) => { state.release.draft = false; } });
  await assert.rejects(run(assetDir, github), /draft|published/i);
  assert.equal(state.uploaded.length, 0);
  assert.equal(state.mutations.length, 1);
});

test('tag movement and release retargeting during upload stop subsequent mutations', async (t) => {
  const { assetDir } = await fixture(t);
  for (const afterUpload of [
    (state) => { state.tag.sha = OTHER_COMMIT; },
    (state) => { state.release.target_commitish = OTHER_COMMIT; },
  ]) {
    const { github, state } = mockGithub({ afterUpload });
    await assert.rejects(run(assetDir, github), /tag|commit/i);
    assert.equal(state.uploaded.length, 1);
    assert.equal(state.mutations.length, 1);
  }
});

test('release recheck failures propagate before asset deletion', async (t) => {
  const { assetDir } = await fixture(t);
  const failure = apiError(503);
  const { github, state } = mockGithub({ assets: ALL_ASSETS, releaseError: failure });
  await assert.rejects(run(assetDir, github), (error) => error === failure);
  assert.deepEqual(state.mutations, []);
});

test('rejects invalid tag, version and commit inputs before API calls', async (t) => {
  const { assetDir } = await fixture(t);
  for (const overrides of [
    { tag: 'v0.2.0' }, { version: '0.1.0-beta.1', tag: 'v0.1.0-beta.1' },
    { version: '01.1.0', tag: 'v01.1.0' }, { commit: 'main' }, { commit: '../secrets' },
  ]) {
    const { github, state } = mockGithub();
    await assert.rejects(run(assetDir, github, overrides), /tag|version|commit/i);
    assert.deepEqual(state.requests, []);
  }
});

test('missing and unrelated local assets fail before GitHub access', async (t) => {
  for (const alter of [
    (dir) => fs.unlink(path.join(dir, INSTALLERS[0])),
    (dir) => fs.writeFile(path.join(dir, 'application.db'), 'private'),
    (dir) => fs.mkdir(path.join(dir, 'nested')),
  ]) {
    const { assetDir } = await fixture(t);
    await alter(assetDir);
    const { github, state } = mockGithub();
    await assert.rejects(run(assetDir, github), /asset|file|directory/i);
    assert.deepEqual(state.requests, []);
  }
});

test('a required installer that is a directory is never uploaded', async (t) => {
  const { assetDir } = await fixture(t);
  const installer = path.join(assetDir, INSTALLERS[0]);
  await fs.unlink(installer);
  await fs.mkdir(installer);
  const { github, state } = mockGithub();
  await assert.rejects(run(assetDir, github), /regular|file/i);
  assert.deepEqual(state.requests, []);
});

test('symbolic link file metadata is rejected without requiring Windows link privileges', async (t) => {
  const { assetDir } = await fixture(t);
  const installer = path.join(assetDir, INSTALLERS[0]);
  const lstat = fs.lstat;
  t.mock.method(fs, 'lstat', async (filename, ...args) => {
    const stat = await lstat(filename, ...args);
    if (filename === installer) {
      stat.isFile = () => false;
      stat.isSymbolicLink = () => true;
    }
    return stat;
  });
  const { github, state } = mockGithub();
  await assert.rejects(run(assetDir, github), /regular|symlink|symbolic|file/i);
  assert.deepEqual(state.requests, []);
});

test('tampered installer or manifest bytes fail their checksums before GitHub access', async (t) => {
  for (const name of [INSTALLERS[0], 'release-manifest.json']) {
    const { assetDir } = await fixture(t);
    await fs.appendFile(path.join(assetDir, name), 'changed');
    const { github, state } = mockGithub();
    await assert.rejects(run(assetDir, github), /checksum|hash/i);
    assert.deepEqual(state.requests, []);
  }
});

test('checksums require exactly seven safe distinct file names', async (t) => {
  for (const alter of [
    (text) => text.split('\n').slice(1).join('\n'),
    (text) => text + text.split('\n')[0] + '\n',
    (text) => text + `${'0'.repeat(64)}  ../../outside\n`,
    (text) => text.replace(/^[a-f0-9]{64}/, 'not-a-checksum'),
  ]) {
    const { assetDir, files } = await fixture(t);
    await fs.writeFile(path.join(assetDir, 'SHA256SUMS.txt'), alter(files.get('SHA256SUMS.txt').toString()));
    const { github, state } = mockGithub();
    await assert.rejects(run(assetDir, github), /checksum|hash|file/i);
    assert.deepEqual(state.requests, []);
  }
});

test('manifest must describe the requested version and commit even with matching checksum', async (t) => {
  for (const changes of [{ version: '0.2.0' }, { commit: OTHER_COMMIT }, { schema_version: 2 }]) {
    const { assetDir, files } = await fixture(t);
    const manifest = JSON.parse(files.get('release-manifest.json'));
    const updated = Buffer.from(JSON.stringify({ ...manifest, ...changes }));
    await fs.writeFile(path.join(assetDir, 'release-manifest.json'), updated);
    await fs.writeFile(path.join(assetDir, 'SHA256SUMS.txt'), files.get('SHA256SUMS.txt').toString()
      .replace(digest(files.get('release-manifest.json')), digest(updated)));
    const { github, state } = mockGithub();
    await assert.rejects(run(assetDir, github), /manifest/i);
    assert.deepEqual(state.requests, []);
  }
});

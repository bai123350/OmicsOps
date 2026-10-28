const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const path = require('node:path');
const assetFormats = require('./release-assets.json');

async function readAssets(assetDir, version, commit) {
  const names = [
    ...Object.values(assetFormats).flat().map(item => `OmicsOps_${version}_${item.suffix}`),
    'release-manifest.json',
    'SHA256SUMS.txt',
  ].sort();
  if (names.length !== 8 || new Set(names).size !== 8) {
    throw new Error('Release asset contract must describe exactly six distinct installers');
  }
  const directory = await fs.lstat(assetDir);
  if (!directory.isDirectory() || directory.isSymbolicLink()) {
    throw new Error('Asset directory must be a real directory, not a symbolic link');
  }
  const entries = (await fs.readdir(assetDir)).sort();
  if (entries.length !== names.length || entries.some((name, i) => name !== names[i])) {
    throw new Error('Asset directory must contain exactly the six installers and two metadata files');
  }

  // Upload these same buffers so local changes after validation cannot alter the bytes sent.
  const assets = new Map();
  for (const name of names) {
    const filename = path.join(assetDir, name);
    const stat = await fs.lstat(filename);
    if (!stat.isFile() || stat.isSymbolicLink()) {
      throw new Error(`Asset must be a regular file, not a symbolic link: ${name}`);
    }
    const data = await fs.readFile(filename);
    if (!data.length) throw new Error(`Asset file is empty: ${name}`);
    assets.set(name, data);
  }

  const checksums = new Map();
  for (const line of assets.get('SHA256SUMS.txt').toString('utf8').trimEnd().split(/\r?\n/)) {
    const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
    if (!match || !assets.has(match[2]) || match[2] === 'SHA256SUMS.txt' || checksums.has(match[2])) {
      throw new Error('Checksum list contains a malformed, unexpected or duplicate file');
    }
    checksums.set(match[2], match[1]);
  }
  if (checksums.size !== 7) throw new Error('Checksum list must cover six installers and release-manifest.json');
  for (const [name, expected] of checksums) {
    if (createHash('sha256').update(assets.get(name)).digest('hex') !== expected) {
      throw new Error(`Checksum mismatch for ${name}`);
    }
  }
  const manifest = JSON.parse(assets.get('release-manifest.json').toString('utf8'));
  if (manifest.schema_version !== 1 || manifest.version !== version || manifest.commit !== commit) {
    throw new Error('Release manifest does not match the requested version and commit');
  }
  return assets;
}

module.exports = async function publishDraft({ github, context, core, tag, version, commit, assetDir, body, title = tag }) {
  if (typeof version !== 'string' || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    throw new Error('Release version must be numeric X.Y.Z without leading zeroes');
  }
  if (tag !== `v${version}`) throw new Error('Release tag must equal v<version>');
  if (typeof title !== 'string' || /[\x00-\x1f\x7f\u2028\u2029]/.test(title)
    || !title.startsWith(tag) || (title.length > tag.length && !/^[: ]/.test(title.slice(tag.length)))) {
    throw new Error('Release title must be a single line beginning with the exact version tag');
  }
  if (typeof commit !== 'string' || !/^[a-f0-9]{40}$/.test(commit)) {
    throw new Error('Release commit must be a full lowercase 40-character hexadecimal SHA');
  }
  const assets = await readAssets(assetDir, version, commit);
  const { owner, repo } = context.repo;
  const repository = { owner, repo };
  const { git, repos } = github.rest;

  async function assertTag() {
    let object = (await git.getRef({ ...repository, ref: `tags/${tag}` })).data.object;
    const visited = new Set();
    for (let depth = 0; object.type === 'tag'; depth += 1) {
      if (depth >= 10 || visited.has(object.sha)) throw new Error('Release tag chain is cyclic or too deep');
      visited.add(object.sha);
      object = (await git.getTag({ ...repository, tag_sha: object.sha })).data.object;
    }
    if (object.type !== 'commit' || object.sha !== commit) {
      throw new Error('Release tag no longer points to the expected commit');
    }
  }

  function assertDraft(release) {
    if (release.draft !== true) throw new Error('Refusing to modify a published release; release must remain a draft');
    if (release.target_commitish !== commit) throw new Error('Draft release targets a different commit');
    if (release.tag_name !== tag) throw new Error('Draft release has a different tag');
  }

  await assertTag();
  let release;
  try {
    release = (await repos.getReleaseByTag({ ...repository, tag })).data;
  } catch (error) {
    if (error.status !== 404) throw error;
    // The tag endpoint only promises published releases. Authenticated listings include drafts.
    const releases = await github.paginate(repos.listReleases, { ...repository, per_page: 100 });
    const matching = releases.filter((candidate) => candidate.tag_name === tag);
    if (matching.length > 1) throw new Error('Multiple releases have the requested tag; refusing ambiguous recovery');
    release = matching[0];
  }
  if (!release) {
    // GitHub can create a missing tag through createRelease. Never intentionally permit that path.
    await assertTag();
    release = (await repos.createRelease({
      ...repository, tag_name: tag, target_commitish: commit, name: title, body,
      draft: true, prerelease: true, make_latest: 'false',
    })).data;
  }
  assertDraft(release);
  const releaseId = release.id;
  async function recheck() {
    const current = (await repos.getRelease({ ...repository, release_id: releaseId })).data;
    assertDraft(current);
    await assertTag();
    return current;
  }

  const existing = await github.paginate(repos.listReleaseAssets, { ...repository, release_id: releaseId, per_page: 100 });
  const existingByName = new Map();
  for (const asset of existing) {
    if (!assets.has(asset.name) || existingByName.has(asset.name)) {
      throw new Error(`Unexpected or duplicate existing release asset: ${asset.name}`);
    }
    existingByName.set(asset.name, asset);
  }
  for (const [name, data] of assets) {
    const previous = existingByName.get(name);
    if (previous) {
      await recheck();
      await repos.deleteReleaseAsset({ ...repository, asset_id: previous.id });
    }
    await recheck();
    await repos.uploadReleaseAsset({
      ...repository, release_id: releaseId, name, data,
      headers: {
        'content-type': name.endsWith('.json') ? 'application/json' : name.endsWith('.txt') ? 'text/plain' : 'application/octet-stream',
        'content-length': data.length,
      },
    });
  }
  release = await recheck();
  core?.info(`Prepared draft release ${tag} with ${assets.size} verified assets.`);
  return release;
};

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { spawnSync } = require('node:child_process');
const { loadReleaseNotes } = require('./release_notes.cjs');

function notesFixture(t, content) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'omicsops-notes-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const folder = path.join(root, '.github/release-notes');
  fs.mkdirSync(folder, { recursive: true });
  if (content !== null) fs.writeFileSync(path.join(folder, 'v1.2.3.md'), content);
  return { root, tag: 'v1.2.3', repository: 'research-lab/OmicsOps' };
}

test('versioned notes produce a custom title and links to all six native installers', t => {
  const options = notesFixture(t, '<!-- release-title: v1.2.3: Desktop preview · 桌面预览 -->\r\n# Changes\r\n\r\n{{DOWNLOADS}}\r\n');
  const result = loadReleaseNotes(options);
  assert.equal(result.title, 'v1.2.3: Desktop preview · 桌面预览');
  for (const suffix of ['x64-setup.exe', 'x64_en-US.msi', 'amd64.deb', 'amd64.AppImage', 'aarch64.dmg', 'x64.dmg']) {
    assert.ok(result.body.includes(`https://github.com/research-lab/OmicsOps/releases/download/v1.2.3/OmicsOps_1.2.3_${suffix}`));
  }
  assert.ok(!result.body.includes('xuzhougeng'));
  assert.ok(!result.body.includes('{{'));
});

test('notes without a title marker fall back to the tag', t => {
  const result = loadReleaseNotes(notesFixture(t, '# Release changes\nReady for review.\n'));
  assert.equal(result.title, 'v1.2.3');
});

test('missing or empty notes stop release preparation', t => {
  for (const content of [null, '', ' \n']) {
    assert.throws(() => loadReleaseNotes(notesFixture(t, content)), /notes/i);
  }
});

test('copied title from another version and unresolved placeholders are rejected', t => {
  for (const content of ['<!-- release-title: v1.2.30: Wrong -->\n# Changes', '<!-- release-title: v9.0.0: Old -->\n# Changes', '# {{VERSION}}\n{{DOWNLOADS}}']) {
    assert.throws(() => loadReleaseNotes(notesFixture(t, content)), /title|placeholder/i);
  }
});

test('unsafe tags and repository coordinates cannot affect notes paths or links', t => {
  const options = notesFixture(t, '# Changes\n{{DOWNLOADS}}');
  for (const tag of ['../secret', 'v1.2.3\n', 'v01.2.3', 'v1.2.3-beta.1']) {
    assert.throws(() => loadReleaseNotes({ ...options, tag }), /tag/i);
  }
  for (const repository of ['../OmicsOps', 'owner/repo/extra', 'owner/repo)', 'owner/repo\n']) {
    assert.throws(() => loadReleaseNotes({ ...options, repository }), /repository/i);
  }
});

test('notes cannot be a directory', t => {
  const options = notesFixture(t, null);
  fs.mkdirSync(path.join(options.root, '.github/release-notes/v1.2.3.md'));
  assert.throws(() => loadReleaseNotes(options), /notes/i);
});

test('CLI validation never echoes potentially sensitive title or note text', t => {
  const sentinel = 'sk-proj-' + 'K7'.repeat(30);
  const options = notesFixture(t, `<!-- release-title: v1.2.3: ${sentinel} -->\n# Preview\n`);
  const scripts = path.join(options.root, 'scripts');
  fs.mkdirSync(scripts);
  for (const name of ['release_notes.cjs', 'release-assets.json']) {
    fs.copyFileSync(path.join(__dirname, name), path.join(scripts, name));
  }
  const result = spawnSync(process.execPath, [path.join(scripts, 'release_notes.cjs'),
    '--tag', options.tag, '--repository', options.repository], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(!`${result.stdout}${result.stderr}`.includes(sentinel), 'CLI must not echo release text');
});

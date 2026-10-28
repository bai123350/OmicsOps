const fs = require('node:fs');
const path = require('node:path');
const { parseArgs } = require('node:util');

function downloadTable(version, base) {
  const assets = require('./release-assets.json');
  const rows = [
    ['Windows', 'x64', 'windows-x64'],
    ['Linux', 'x86_64 / amd64', 'linux-x64'],
    ['macOS', 'Apple Silicon / ARM64', 'macos-arm64'],
    ['macOS', 'Intel / x64', 'macos-x64'],
  ];
  const table = ['| 系统 / System | 芯片 / CPU | 安装包 / Installer |', '| --- | --- | --- |'];
  for (const [system, cpu, target] of rows) {
    const links = assets[target].map(asset => {
      const name = `OmicsOps_${version}_${asset.suffix}`;
      return `[\`${name}\`](${base}/${name})`;
    });
    table.push(`| ${system} | ${cpu} | ${links.join(' / ')} |`);
  }
  table.push('', `[SHA256SUMS.txt](${base}/SHA256SUMS.txt) · [release-manifest.json](${base}/release-manifest.json)`);
  return table.join('\n');
}

function loadReleaseNotes({ root = path.resolve(__dirname, '..'), tag, repository }) {
  if (typeof tag !== 'string' || /[\r\n]/.test(tag) || !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag)) {
    throw new Error('Release notes tag must be vX.Y.Z');
  }
  if (typeof repository !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9-]*\/[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(repository)) {
    throw new Error('Release notes repository must be owner/repository');
  }
  const folder = path.join(root, '.github/release-notes');
  const filename = path.join(folder, `${tag}.md`);
  let body;
  try {
    for (const directory of [path.join(root, '.github'), folder]) {
      const stat = fs.lstatSync(directory);
      if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error('Invalid notes directory');
    }
    const stat = fs.lstatSync(filename);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Invalid notes file');
    body = fs.readFileSync(filename, 'utf8').trim();
  } catch {
    throw new Error(`Release notes must be an ordinary file: .github/release-notes/${tag}.md`);
  }
  if (!body) throw new Error('Release notes must not be empty');
  const markers = [...body.matchAll(/^<!--[ \t]*release-title:[ \t]*(.*?)[ \t]*-->[ \t]*$/gm)];
  const title = markers[0]?.[1].trim() || tag;
  if (markers.length > 1 || /[\x00-\x1f\x7f]/.test(title) || title.length > 200 ||
      !(title === tag || title.startsWith(`${tag}:`) || title.startsWith(`${tag} `))) {
    throw new Error('Release notes title must be a single line starting with the exact tag');
  }
  const base = `https://github.com/${repository}/releases/download/${tag}`;
  body = body.replaceAll('{{DOWNLOADS}}', downloadTable(tag.slice(1), base))
    .replaceAll('{{DOWNLOAD_BASE}}', base)
    .replaceAll('{{REPOSITORY_URL}}', `https://github.com/${repository}`);
  if (body.includes('{{') || body.includes('}}')) throw new Error('Release notes contain an unresolved placeholder');
  return { title, body: `${body}\n` };
}

if (require.main === module) {
  try {
    const { values } = parseArgs({ options: {
      tag: { type: 'string' }, repository: { type: 'string' }, output: { type: 'string' },
    } });
    const notes = loadReleaseNotes({ tag: values.tag, repository: values.repository });
    if (values.output) fs.writeFileSync(values.output, notes.body, { encoding: 'utf8', flag: 'wx' });
    // Notes may contain an accidental credential; never echo their text to CI logs.
    console.log(`Validated release notes for ${values.tag}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

module.exports = { loadReleaseNotes };

"""Validate release versions and collect native installers using Python 3.12."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import tomllib


ASSET_FORMATS = json.loads(Path(__file__).with_name("release-assets.json").read_text(encoding="utf-8"))
TARGETS = {
    target: tuple((item["folder"], item["extension"]) for item in formats)
    for target, formats in ASSET_FORMATS.items()
}


class ReleaseError(ValueError):
    """An input violates the release contract."""


def validate_version(version):
    if not isinstance(version, str) or not re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version
    ):
        raise ReleaseError("version must be numeric X.Y.Z without leading zeros")
    major, minor, patch = map(int, version.split("."))
    if major > 255 or minor > 255 or patch > 65535:
        raise ReleaseError("version exceeds MSI limits (255.255.65535)")
    return version


def validate_commit(commit):
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ReleaseError("commit must be a full lowercase 40-character Git SHA")


def json_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ReleaseError(f"duplicate JSON field: {key}")
        value[key] = item
    return value


def read_json(path):
    with path.open(encoding="utf-8") as source:
        return json.load(source, object_pairs_hook=json_object)


def read_toml(path):
    with path.open("rb") as source:
        return tomllib.load(source)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")


def is_link(path):
    return path.is_symlink() or path.is_junction()


def require_directory(path):
    if is_link(path) or not path.is_dir():
        raise ReleaseError(f"expected an ordinary directory: {path}")


def require_empty_destination(path):
    if is_link(path) or (path.exists() and (not path.is_dir() or any(path.iterdir()))):
        raise ReleaseError(f"output directory must be empty: {path}")


def file_record(path, name=None):
    if is_link(path) or not path.is_file():
        raise ReleaseError(f"expected an ordinary file, not a link: {path}")
    size = path.stat().st_size
    if size == 0:
        raise ReleaseError(f"empty installer or manifest: {path}")
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    return {"name": name or path.name, "sha256": digest, "size": size}


def asset_name(version, target, extension):
    suffix = next(item["suffix"] for item in ASSET_FORMATS[target] if item["extension"] == extension)
    return f"OmicsOps_{version}_{suffix}"


def check_version(args):
    root = args.root
    version = validate_version(read_json(root / "package.json")["version"])
    lock = read_json(root / "package-lock.json")
    cargo = read_toml(root / "Cargo.toml")
    versions = {
        "package-lock.json version": lock["version"],
        "package-lock.json packages[''].version": lock["packages"][""]["version"],
        "Cargo.toml workspace.package.version": cargo["workspace"]["package"]["version"],
        "src-tauri/tauri.conf.json version": read_json(root / "src-tauri/tauri.conf.json")["version"],
    }

    workspace = cargo["workspace"]
    excluded = {
        path.resolve()
        for pattern in workspace.get("exclude", [])
        for path in root.glob(pattern)
    }
    members = set()
    for pattern in workspace["members"]:
        matches = [path for path in root.glob(pattern) if path.resolve() not in excluded]
        if not matches:
            raise ReleaseError(f"workspace member pattern matches no packages: {pattern}")
        members.update(matches)
    if "package" in cargo:
        members.add(root)
    names = set()
    for member in sorted(members):
        package = read_toml(member / "Cargo.toml")["package"]
        name = package["name"]
        if name in names:
            raise ReleaseError(f"duplicate workspace package: {name}")
        names.add(name)
        declared = package["version"]
        if declared != {"workspace": True}:
            versions[f"{member / 'Cargo.toml'} package.version"] = declared
    if not names:
        raise ReleaseError("workspace has no packages")
    packages = read_toml(root / "Cargo.lock")["package"]
    for name in sorted(names):
        entries = [item for item in packages if item["name"] == name and "source" not in item]
        if len(entries) != 1:
            raise ReleaseError(f"Cargo.lock must contain exactly one workspace package {name}")
        versions[f"Cargo.lock {name}"] = entries[0]["version"]
    for location, actual in versions.items():
        if actual != version:
            raise ReleaseError(f"version mismatch in {location}: expected {version}, found {actual!r}")
    if args.tag and args.tag != f"v{version}":
        raise ReleaseError(f"tag must equal v{version}, found {args.tag!r}")
    output = f"version={version}\ntag={args.tag}\ncreate_release={'true' if args.tag else 'false'}\n"
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8", newline="\n") as destination:
            destination.write(output)
    print(output, end="")


def stage(args):
    validate_version(args.version)
    validate_commit(args.commit)
    require_empty_destination(args.output_dir)
    require_directory(args.bundle_dir)
    installers = []
    for folder, extension in TARGETS[args.target]:
        source_dir = args.bundle_dir / folder
        require_directory(source_dir)
        # Tauri emits installers at the format directory's top level. AppImage
        # builds also leave an AppDir with internal links and resources beside it.
        matches = sorted(path for path in source_dir.iterdir() if path.suffix == extension)
        if len(matches) != 1:
            raise ReleaseError(f"expected exactly one {extension} installer in {source_dir}, found {len(matches)}")
        source = matches[0]
        record = file_record(source, asset_name(args.version, args.target, extension))
        installers.append((source, record))
    installers.sort(key=lambda item: item[1]["name"])
    manifest = {
        "schema_version": 1, "version": args.version, "commit": args.commit,
        "target": args.target, "files": [record for _, record in installers],
    }
    # Validate every source and the destination before creating any output.
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for source, record in installers:
        shutil.copyfile(source, args.output_dir / record["name"])
    write_json(args.output_dir / f"manifest-{args.target}.json", manifest)
    print(f"Staged {len(installers)} installers for {args.target}")


def assemble(args):
    validate_version(args.version)
    validate_commit(args.commit)
    require_empty_destination(args.output_dir)
    require_directory(args.input_dir)
    expected_manifests = {f"manifest-{target}.json" for target in TARGETS}
    expected_installers = {
        asset_name(args.version, target, extension)
        for target, formats in TARGETS.items()
        for _, extension in formats
    }
    paths = list(args.input_dir.iterdir())
    if {path.name for path in paths} != expected_manifests | expected_installers:
        raise ReleaseError("input must contain exactly four target manifests and six normalized installers")
    for path in paths:
        if is_link(path) or not path.is_file():
            raise ReleaseError(f"input must contain only ordinary files: {path}")

    manifests = []
    installers = []
    for target in sorted(TARGETS):
        manifest = read_json(args.input_dir / f"manifest-{target}.json")
        if not isinstance(manifest, dict) or set(manifest) != {
            "schema_version", "version", "commit", "target", "files"
        }:
            raise ReleaseError(f"invalid manifest fields for {target}")
        if type(manifest["schema_version"]) is not int or manifest["schema_version"] != 1:
            raise ReleaseError(f"unsupported manifest schema for {target}")
        if (manifest["version"], manifest["commit"], manifest["target"]) != (
            args.version, args.commit, target
        ):
            raise ReleaseError(f"manifest version, commit or target mismatch for {target}")
        entries = manifest["files"]
        if not isinstance(entries, list) or len(entries) != len(TARGETS[target]):
            raise ReleaseError(f"incorrect installer count for {target}")
        expected_names = {asset_name(args.version, target, ext) for _, ext in TARGETS[target]}
        seen = set()
        for entry in entries:
            if not isinstance(entry, dict) or set(entry) != {"name", "sha256", "size"}:
                raise ReleaseError(f"invalid file record for {target}")
            name = entry["name"]
            if not isinstance(name, str) or name not in expected_names or name in seen:
                raise ReleaseError(f"unsafe, unexpected or duplicate installer name: {name!r}")
            seen.add(name)
            if type(entry["size"]) is not int or entry["size"] <= 0:
                raise ReleaseError(f"invalid installer size: {name}")
            if not isinstance(entry["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
                raise ReleaseError(f"invalid SHA-256: {name}")
            source = args.input_dir / name
            if file_record(source) != entry:
                raise ReleaseError(f"installer size or SHA-256 mismatch: {name}")
            installers.append((source, entry))
        manifests.append(manifest)

    # No output exists until all four manifests and every installer pass validation.
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for source, record in installers:
        shutil.copyfile(source, args.output_dir / record["name"])
    combined = {
        "schema_version": 1, "version": args.version,
        "commit": args.commit, "targets": manifests,
    }
    manifest_path = args.output_dir / "release-manifest.json"
    write_json(manifest_path, combined)
    checksums = [record for _, record in installers] + [file_record(manifest_path)]
    (args.output_dir / "SHA256SUMS.txt").write_text(
        "".join(f"{record['sha256']}  {record['name']}\n"
                for record in sorted(checksums, key=lambda record: record["name"])),
        encoding="utf-8", newline="\n",
    )
    print("Assembled six installers, release-manifest.json and SHA256SUMS.txt")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    version_parser = commands.add_parser("check-version", help="validate repository versions and a release tag")
    version_parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    version_parser.add_argument("--tag", default="")
    version_parser.add_argument("--github-output", type=Path)
    version_parser.set_defaults(run=check_version)
    stage_parser = commands.add_parser("stage", help="validate and normalize native installers")
    stage_parser.add_argument("--target", choices=TARGETS, required=True)
    stage_parser.add_argument("--bundle-dir", type=Path, required=True)
    stage_parser.set_defaults(run=stage)
    assemble_parser = commands.add_parser("assemble", help="verify the complete release asset set")
    assemble_parser.add_argument("--input-dir", type=Path, required=True)
    assemble_parser.set_defaults(run=assemble)
    for command in (stage_parser, assemble_parser):
        command.add_argument("--output-dir", type=Path, required=True)
        command.add_argument("--version", required=True)
        command.add_argument("--commit", required=True)
    args = parser.parse_args()
    try:
        args.run(args)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"release error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

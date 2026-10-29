"""Fail closed on private desktop release inputs; never print matched values.

The gate scans configured resources, optional frontend output and uncompressed native
executables. It is a deterministic guard, not a claim that arbitrary private text can
be recognized or that compressed installers have been unpacked and audited.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


EXTENSION_FILES = (
    "asset-stager.js", "chrome-api.js", "command-router.js", "content-script.js",
    "execute-js-policy.js", "manifest.json", "protocol.js", "README.md", "search.js",
    "service-worker.js", "session.html", "session.js", "tab-ledger.js", "url-policy.js",
    "websocket-client.js",
)
RESOURCE_MAPPINGS = {
    "../skills/": "skills/",
    "../docs/third-party/skills/": "licenses/skills/",
    "../LICENSE": "licenses/AGPL-3.0.txt",
    "../crates/omicsops-bio/UPSTREAM.md": "licenses/omicsops-bio-UPSTREAM.md",
    **{f"../browser-extension/{name}": f"browser-extension/{name}" for name in EXTENSION_FILES},
}
PRIVATE_SUFFIXES = {
    ".db", ".sqlite", ".sqlite3", ".log", ".pem", ".key", ".p12", ".pfx",
    ".pyc", ".pyo", ".pdb", ".dmp", ".map",
}
PRIVATE_PARTS = {
    ".git", ".ssh", ".aws", ".codex", "__pycache__", ".pytest_cache",
    "node_modules", ".ds_store", "id_rsa", "id_ed25519", "id_ecdsa",
    "credentials.json", "credentials.toml", "cookies.sqlite", "login data",
}
TOKEN = re.compile(
    rb"\b(?:sk-(?:(?:proj|svcacct|ant-api03)-)?[A-Za-z0-9_-]{24,}"
    rb"|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}"
    rb"|AIza[A-Za-z0-9_-]{30,}|(?:AKIA|ASIA)[A-Z0-9]{16})\b"
)
PRIVATE_KEY = re.compile(
    rb"-----BEGIN ((?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY)-----"
    rb"[\r\n\t ]+[A-Za-z0-9+/=\r\n\t ]{40,}?-----END \1-----"
)
LITERAL_KEY = rb"(?:[A-Za-z0-9]+[_-])?(?:api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|password|token)"
LITERAL = re.compile(
    rb"(?i)(?:[\"'](?P<quoted>" + LITERAL_KEY + rb")[\"']|(?<![A-Za-z0-9_])(?P<bare>"
    + LITERAL_KEY + rb"))\s*[:=]\s*[\"'](?P<value>[^\"'\r\n]{1,512})[\"']"
)
AUTH_URL = re.compile(
    rb"(?i)\b(?:https?|ssh|postgres(?:ql)?|mysql|redis)://"
    rb'([^:/@\x00-\x20\x7f?#"\\]{0,128}):([^/@\x00-\x20\x7f?#"\\]{1,256})@'
)
URL_SCHEMES = ("https://", "http://", "ssh://", "postgresql://", "postgres://", "mysql://", "redis://")
UTF16_URL_STARTS = tuple((encoding, re.compile(
    b"|".join(re.escape(scheme.encode(encoding)) for scheme in URL_SCHEMES), re.IGNORECASE,
)) for encoding in ("utf-16le", "utf-16be"))
MAX_URL_CODE_UNITS = 1024
MAX_DIAGNOSTICS = 20
BUNDLE_FIELDS = {
    "active": None, "targets": None, "category": None, "shortDescription": None,
    "resources": None, "icon": None,
    "linux": {"deb": {"depends": None, "recommends": None}},
    "macOS": {"signingIdentity": None},
    "windows": {},
}


def is_link(path):
    return path.is_symlink() or path.is_junction()


def placeholder(value):
    value = value.strip().lower()
    return (
        not value
        or value in {b"redacted", b"[redacted]", b"...", b"changeme", b"change-me"}
        or (value.startswith(b"<") and value.endswith(b">"))
        or (value.startswith(b"${") and value.endswith(b"}"))
        or (value.startswith(b"{{") and value.endswith(b"}}"))
        or value.startswith((b"your_", b"your-", b"replace_with_"))
        or set(value) <= {ord("*")}
    )


def authenticated_urls(data):
    """Yield only structural metadata; never concatenate NUL-terminated strings."""
    def detail(view, offset, length, value):
        try:
            value.decode("utf-8")
            utf8_valid = True
        except UnicodeError:
            utf8_valid = False
        return {
            "view": view, "offset": offset, "length": length,
            "control_count": sum(byte < 32 or byte == 127 for byte in value),
            "query_fragment_count": value.count(b"?") + value.count(b"#"),
            "non_ascii_count": sum(byte >= 128 for byte in value),
            "utf8_valid": utf8_valid,
        }

    for match in AUTH_URL.finditer(data):
        if not placeholder(match.group(2)):
            yield detail("raw", match.start(), match.end() - match.start(), match.group())
    # Decode only bounded candidates beginning at an encoded URL scheme. Keeping
    # real UTF-16 code units preserves Unicode passwords and surrogate pairs; a
    # NUL/control terminator ends a candidate instead of joining the next string.
    for encoding, starts in UTF16_URL_STARTS:
        byteorder = "little" if encoding.endswith("le") else "big"
        for start in starts.finditer(data):
            offset = start.start()
            limit = min(len(data) - 1, offset + MAX_URL_CODE_UNITS * 2)
            for position in range(start.end(), limit, 2):
                unit = int.from_bytes(data[position:position + 2], byteorder)
                if unit < 32 or unit == 127:
                    break
                if unit != ord("@"):
                    continue
                end = position + 2
                try:
                    value = data[offset:end].decode(encoding).encode("utf-8")
                except UnicodeError:
                    break
                match = AUTH_URL.match(value)
                if match and not placeholder(match.group(2)):
                    yield detail(encoding, offset, end - offset, match.group())
                break


def scan_bytes(data, forbidden_prefixes, *, diagnostics=None):
    """Return rule IDs only, never values; also recognize UTF-16 ASCII literals."""
    findings = set()
    views = [data]
    if b"\x00" in data:
        views.append(data.replace(b"\x00", b""))
    for view in views:
        if TOKEN.search(view):
            findings.add("credential-token")
        if PRIVATE_KEY.search(view.replace(b"\\n", b"\n").replace(b"\\r", b"\r")):
            findings.add("private-key")
        for match in LITERAL.finditer(view):
            # A translated label in a JS ternary ("Password":"API key / token")
            # is not an object field. Quoted fields must start a structural entry.
            if match.group("quoted"):
                index = match.start() - 1
                while index >= 0 and view[index] in b" \t":
                    index -= 1
                if index >= 0 and view[index] not in b"{,;\n\r":
                    continue
            key = match.group("quoted") or match.group("bare")
            value = match.group("value")
            if len(value) >= (16 if key.lower() == b"token" else 6) and not placeholder(value):
                findings.add("credential-literal")
        lowered = view.lower()
        for prefix in forbidden_prefixes:
            variants = {prefix, prefix.replace("\\", "/"), prefix.replace("\\", "\\\\")}
            if any(variant.encode("utf-8").lower() in lowered for variant in variants if len(variant) > 3):
                findings.add("local-build-path")
    # The legacy zero-stripped view above is retained for the other detectors.
    # URL authority parsing must never use it: it merges unrelated binary strings.
    for detail in authenticated_urls(data):
        findings.add("authenticated-url")
        if diagnostics is not None and len(diagnostics) < MAX_DIAGNOSTICS:
            diagnostics.append(detail)
    return findings


def default_prefixes(root):
    # Only path metadata is read; credential variables/keyrings are never accessed.
    values = {str(root.resolve()), str(Path.home())}
    for name in ("USERPROFILE", "HOME", "CARGO_HOME", "RUSTUP_HOME"):
        value = os.environ.get(name)
        if value and Path(value).is_absolute():
            values.add(value)
    return sorted(values)


def private_name(path):
    parts = {part.lower() for part in path.parts}
    name = path.name.lower()
    return (
        bool(parts & PRIVATE_PARTS)
        or name == ".env" or name.startswith(".env.")
        or path.suffix.lower() in PRIVATE_SUFFIXES
        or name.endswith((".db-wal", ".db-shm", ".sqlite-wal", ".sqlite-shm"))
    )


def allowed_fields(value, schema):
    """New bundler inputs require an explicit privacy review instead of a denylist."""
    return isinstance(value, dict) and all(
        key in schema and (schema[key] is None or allowed_fields(item, schema[key]))
        for key, item in value.items()
    )


def audit_repo(root, *, dist=None, binaries=(), forbidden_prefixes=None, tracked_files=None, diagnostics=None):
    """Check only release inputs. Findings are (safe relative file label, rule ID)."""
    input_root = Path(root).absolute()
    root = input_root.resolve()
    prefixes = default_prefixes(root) if forbidden_prefixes is None else forbidden_prefixes
    findings = set()
    inspected = set()

    def label(path):
        try:
            relative = path.relative_to(root)
            return "<external-input>" if ".." in relative.parts else relative.as_posix()
        except ValueError:
            return "<external-input>"

    def build_input(value):
        # Canonicalize only the caller's root (e.g. a Windows short name or macOS
        # /var alias). Resolving the whole input would conceal descendant links.
        path = Path(value).absolute()
        if path.is_relative_to(input_root):
            path = root / path.relative_to(input_root)
        return path

    def add(path, rule):
        findings.add((label(path), rule))

    def scan_content(path, content):
        details = []
        for rule in scan_bytes(content, prefixes, diagnostics=details):
            add(path, rule)
        if diagnostics is not None:
            for detail in details:
                if len(diagnostics) >= MAX_DIAGNOSTICS:
                    break
                diagnostics.append({"path": label(path), **detail})

    if tracked_files is None:
        result = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z"],
            capture_output=True, check=False,
        )
        if result.returncode:
            return [("<repository>", "tracked-files-unavailable")]
        tracked_files = set(result.stdout.decode("utf-8", errors="replace").split("\x00"))

    def scan_file(path, *, resource=False):
        if path in inspected:
            return
        inspected.add(path)
        if is_link(path):
            add(path, "resource-link")
            return
        if not path.is_file():
            add(path, "missing-input")
            return
        if private_name(path.relative_to(root)):
            add(path, "private-resource-file")
            return
        if resource and label(path) not in tracked_files:
            add(path, "untracked-resource")
            return
        try:
            scan_content(path, path.read_bytes())
        except OSError:
            add(path, "unreadable-input")

    def walk(path, *, resource=False):
        if not path.is_relative_to(root) or ".." in path.parts:
            add(path, "external-input")
            return
        if any(is_link(parent) for parent in (path, *path.parents) if parent.is_relative_to(root)):
            add(path, "resource-link")
            return
        if not path.exists():
            add(path, "missing-input")
        elif path.is_dir():
            if private_name(path.relative_to(root)):
                add(path, "private-resource-file")
                return
            try:
                for child in sorted(path.iterdir()):
                    walk(child, resource=resource)
            except OSError:
                add(path, "unreadable-input")
        else:
            scan_file(path, resource=resource)

    config_root = root / "src-tauri"
    # Tauri reads this implicitly even without bundle.macOS.infoPlist configured.
    if (config_root / "Info.plist").exists():
        add(config_root / "Info.plist", "implicit-bundle-input")
    for name in ("Tauri.toml", "tauri.conf.json5", "tauri.windows.conf.json5", "tauri.linux.conf.json5", "tauri.macos.conf.json5", "Tauri.windows.toml", "Tauri.linux.toml", "Tauri.macos.toml"):
        path = config_root / name
        if path.exists():
            add(path, "alternate-configuration")
    configs = [config_root / "tauri.conf.json"]
    configs.extend(config_root / f"tauri.{platform}.conf.json" for platform in ("windows", "linux", "macos"))
    for config_path in configs:
        if not config_path.exists() and config_path.name != "tauri.conf.json":
            continue
        if is_link(config_path):
            add(config_path, "resource-link")
            continue
        try:
            raw_config = config_path.read_bytes()
            scan_content(config_path, raw_config)
            config = json.loads(raw_config.decode("utf-8"))
            bundle = config.get("bundle", {})
            build = config.get("build", {})
            if not isinstance(bundle, dict) or not isinstance(build, dict):
                raise ValueError("invalid configuration")
            if build.get("frontendDist", "../dist") != "../dist":
                add(config_path, "frontend-directory")
            if build.get("beforeBundleCommand") or build.get("runner"):
                add(config_path, "additional-build-input")
            if bundle.get("externalBin"):
                add(config_path, "external-binary")
            if not allowed_fields(bundle, BUNDLE_FIELDS):
                add(config_path, "additional-bundle-input")
            if bundle.get("macOS", {}).get("signingIdentity", "-") != "-":
                add(config_path, "unreviewed-signing-identity")
            resources = bundle.get("resources", {})
            if not isinstance(resources, dict):
                add(config_path, "resource-mapping")
                continue
            for source, destination in resources.items():
                if RESOURCE_MAPPINGS.get(source) != destination:
                    add(config_path, "resource-mapping")
                    continue
                walk(root / source.removeprefix("../"), resource=True)
            for icon in bundle.get("icon", []):
                if not isinstance(icon, str) or not re.fullmatch(r"icons/[A-Za-z0-9@_.-]+\.(?:png|ico|icns)", icon):
                    add(config_path, "icon-path")
                    continue
                walk(config_root / icon, resource=True)
        except (OSError, ValueError, TypeError, AttributeError):
            add(config_path, "invalid-configuration")
    if dist is not None:
        walk(build_input(dist))
    for binary in binaries:
        path = build_input(binary)
        if not path.is_relative_to(root) or ".." in path.parts:
            add(path, "external-input")
        elif path.is_dir():
            add(path, "expected-binary-file")
        else:
            walk(path)
    return sorted(findings)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--dist", type=Path)
    parser.add_argument("--binary", type=Path, action="append", default=[])
    parser.add_argument("--document", type=Path, action="append", default=[],
                        help="also scan a release notes document before publication")
    parser.add_argument("--forbid-prefix", action="append", default=[])
    args = parser.parse_args()
    diagnostics = []
    try:
        findings = audit_repo(
            args.root, dist=args.dist, binaries=args.binary + args.document,
            forbidden_prefixes=default_prefixes(args.root) + args.forbid_prefix,
            diagnostics=diagnostics,
        )
    except (OSError, ValueError):
        print("privacy error: <inputs>: inspection-failed", file=sys.stderr)
        return 1
    if findings:
        for path, rule in findings:
            print(f"privacy error: {path}: {rule}", file=sys.stderr)
        for detail in diagnostics:
            print("privacy detail: " + json.dumps(detail, sort_keys=True), file=sys.stderr)
        return 1
    print("Privacy checks passed for configured resources and requested build outputs.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Build one pinned font into an owned SD bundle, reusing verified artifacts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
STAMP = 'BUNDLE.JSON'
FORMAT = 'pulp-cjk-bundle-v1'


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def load_manifest(path):
    data = json.loads(path.read_text())
    if data.get('schema_version') != 1:
        raise ValueError('unsupported manifest schema_version')
    for key in ('name', 'version', 'upstream_url', 'license_name'):
        value = data.get(key)
        if not isinstance(value, str) or not value or value.strip() != value or any(
                ord(c) < 32 or ord(c) > 126 for c in value):
            raise ValueError(f'{key} must be non-empty printable ASCII')
    sizes = data.get('sizes')
    if not isinstance(sizes, list) or not sizes or any(
            type(s) is not int or not 1 <= s <= 255 for s in sizes) or len(set(sizes)) != len(sizes):
        raise ValueError('sizes must be distinct integers in 1..255')
    inputs = {}
    for key in ('font', 'license', 'require_chars'):
        if key == 'require_chars' and key not in data:
            continue
        item = data.get(key)
        if not isinstance(item, dict) or not isinstance(item.get('path'), str):
            raise ValueError(f'{key} requires path and sha256')
        digest = item.get('sha256')
        if not isinstance(digest, str) or len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):
            raise ValueError(f'{key} requires lowercase SHA256')
        source = (path.parent / item['path']).resolve()
        if sha256(source) != digest:
            raise ValueError(f'{key} SHA256 mismatch: {source}')
        inputs[key] = source
    return data, inputs


def converter_binary():
    host = next(line.removeprefix('host: ') for line in subprocess.check_output(
        ['rustc', '-vV'], cwd=ROOT, text=True).splitlines() if line.startswith('host: '))
    # Cargo performs dependency freshness checks even when the font cache hits.
    subprocess.run(['cargo', 'build', '--locked', '-p', 'pulp-fontconv', '--release',
                    '--target', host, '--config', 'unstable.build-std=["std","panic_unwind"]'],
                   cwd=ROOT, check=True)
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
    if not target.is_absolute():
        target = ROOT / target
    return target / host / 'release/pulp-fontconv'


def fingerprint(converter):
    paths = [ROOT / 'Cargo.lock', ROOT / 'Cargo.toml', ROOT / 'rust-toolchain.toml',
             ROOT / '.cargo/config.toml', Path(__file__)]
    for directory in ('fontconv', 'fontpack'):
        paths.extend(p for p in (ROOT / directory).rglob('*')
                     if p.is_file() and (p.suffix == '.rs' or p.name == 'Cargo.toml'))
    return dict(converter_sha256=sha256(converter),
                rustc=subprocess.check_output(['rustc', '-vV'], cwd=ROOT, text=True),
                inputs={str(p.relative_to(ROOT)): sha256(p) for p in sorted(paths)})


def owned(directory):
    try:
        return (not directory.is_symlink() and directory.is_dir()
                and not (directory / STAMP).is_symlink()
                and json.loads((directory / STAMP).read_text()).get('format') == FORMAT)
    except (OSError, ValueError, AttributeError):
        return False


def verified(directory, key):
    try:
        stamp = json.loads((directory / STAMP).read_text())
        files = stamp['files']
        return (owned(directory) and stamp['cache_key'] == key and isinstance(files, dict) and bool(files)
                and set(p.name for p in directory.iterdir()) == set(files) | {STAMP}
                and all(Path(name).name == name and not (directory / name).is_symlink()
                        and (directory / name).is_file() and sha256(directory / name) == digest
                        for name, digest in files.items()))
    except (OSError, ValueError, KeyError, TypeError):
        return False


def build(manifest, out, cache, converter=None):
    """Return True on a verified cache hit. Paths identify script-owned directories."""
    manifest, out, cache = Path(manifest).resolve(), Path(out).resolve(), Path(cache).resolve()
    data, inputs = load_manifest(manifest)
    if out == cache or out in cache.parents or cache in out.parents:
        raise ValueError('output and cache directories must be separate')
    destination = out / '_PULP/FONTS'
    if destination.parent.is_symlink():
        raise ValueError(f'refusing symlink SD parent: {destination.parent}')
    if (destination.exists() or destination.is_symlink()) and not owned(destination):
        raise ValueError(f'refusing to replace unowned font directory: {destination}')
    converter = Path(converter) if converter is not None else converter_binary()
    identity = dict(manifest=data, build=fingerprint(converter))
    key = hashlib.sha256(canonical(identity)).hexdigest()
    cache.mkdir(parents=True, exist_ok=True)
    artifact = cache / key
    if artifact.is_symlink():
        raise ValueError(f'refusing symlink cache artifact: {artifact}')
    hit = verified(artifact, key)
    if not hit:
        with tempfile.TemporaryDirectory(prefix='font-build-', dir=cache) as tmp:
            packs = Path(tmp) / 'packs'
            command = [str(converter), '--font', str(inputs['font']),
                       '--license', str(inputs['license']), '--license-name', data['license_name'],
                       '--upstream-url', data['upstream_url'], '--sizes',
                       ','.join(map(str, data['sizes'])), '--out', str(packs)]
            if 'require_chars' in inputs:
                command.extend(['--require-chars', str(inputs['require_chars'])])
            subprocess.run(command, cwd=ROOT, check=True)
            files = {p.name: sha256(p) for p in sorted(packs.iterdir())}
            (packs / STAMP).write_text(json.dumps(dict(format=FORMAT, cache_key=key, **identity, files=files),
                                                 sort_keys=True, indent=2) + '\n')
            if artifact.exists():
                shutil.rmtree(artifact)
            packs.rename(artifact)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='font-install-', dir=destination.parent) as tmp:
        staged = Path(tmp) / 'FONTS'
        shutil.copytree(artifact, staged)
        if not verified(staged, key):
            raise ValueError('artifact changed while copying')
        # Replacing the entire selected set removes packs from the previous font.
        backup = Path(tmp) / 'previous'
        if destination.exists():
            destination.rename(backup)
        try:
            staged.rename(destination)
        except OSError:
            if backup.exists():
                backup.rename(destination)
            raise
    print(f'{"cache hit" if hit else "built"}: {data["name"]} {data["version"]} -> {destination}')
    return hit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=ROOT / 'fonts/cjk.json')
    parser.add_argument('--out', type=Path, default=ROOT / 'target/cjk-sd')
    parser.add_argument('--cache', type=Path, default=ROOT / 'target/cjk-cache')
    args = parser.parse_args()
    try:
        build(args.manifest, args.out, args.cache)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'font build failed: {error}\n')


if __name__ == '__main__':
    main()

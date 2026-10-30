"""Author-only packager: ship one native binary, thin launchers and notices.

This script is never included in player kits. Requires an existing release build
and the build machine's Cargo dependency cache; no download or game input.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import zipfile


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def machine(path):
    data = Path(path).read_bytes()[:4096]
    if data[:2] == b'MZ':
        pe = int.from_bytes(data[60:64], 'little')
        if data[pe:pe+4] == b'PE\0\0' and int.from_bytes(data[pe+4:pe+6], 'little') == 0x8664:
            return 'Windows-x86_64'
    if data[:6] == b'\x7fELF\x02\x01' and int.from_bytes(data[18:20], 'little') == 62:
        return 'Linux-x86_64-musl'
    if data[:4] == b'\xcf\xfa\xed\xfe':
        cpu = int.from_bytes(data[4:8], 'little')
        return {0x1000007: 'macOS-x86_64', 0x100000C: 'macOS-arm64'}.get(cpu)
    return None


def linkage(path, target):
    """Check shipped bytes, not just the builder's static-link flags."""
    data = Path(path).read_bytes()
    if target.startswith('Linux'):
        phoff = int.from_bytes(data[32:40], 'little')
        size = int.from_bytes(data[54:56], 'little')
        count = int.from_bytes(data[56:58], 'little')
        if size < 56 or phoff+size*count > len(data):
            raise ValueError('Invalid ELF program headers')
        for i in range(count):
            at = phoff+i*size
            kind = int.from_bytes(data[at:at+4], 'little')
            if kind == 3:
                raise ValueError('Linux binary has PT_INTERP; not a standalone static kit')
            if kind == 2:
                offset = int.from_bytes(data[at+8:at+16], 'little')
                length = int.from_bytes(data[at+32:at+40], 'little')
                if offset+length > len(data):
                    raise ValueError('Invalid ELF dynamic range')
                for tag in range(offset, offset+length, 16):
                    if int.from_bytes(data[tag:tag+8], 'little') == 1:
                        raise ValueError('Linux binary has DT_NEEDED dependencies')
        return {'PT_INTERP': False, 'DT_NEEDED': False}
    if target.startswith('Windows'):
        pe = int.from_bytes(data[60:64], 'little')
        optional = pe+24
        sections = int.from_bytes(data[pe+6:pe+8], 'little')
        table = optional+int.from_bytes(data[pe+20:pe+22], 'little')
        def offset(rva):
            for i in range(sections):
                s = table+i*40
                va = int.from_bytes(data[s+12:s+16], 'little')
                length = max(int.from_bytes(data[s+8:s+12], 'little'), int.from_bytes(data[s+16:s+20], 'little'))
                if va <= rva < va+length:
                    out = int.from_bytes(data[s+20:s+24], 'little')+rva-va
                    if out >= len(data):
                        raise ValueError('Invalid PE RVA')
                    return out
            raise ValueError('PE RVA outside sections')
        rva = int.from_bytes(data[optional+120:optional+124], 'little')
        at = offset(rva)
        imports = []
        for _ in range(512):
            if at+20 > len(data):
                raise ValueError('Truncated PE imports')
            row = data[at:at+20]
            if not any(row):
                break
            name = offset(int.from_bytes(row[12:16], 'little'))
            end = data.find(b'\0', name, name+256)
            if end < 0:
                raise ValueError('Invalid PE import name')
            dll = data[name:end].decode('ascii').lower()
            if dll.startswith(('vcruntime', 'msvcp', 'ucrtbase', 'api-ms-win-crt', 'python')):
                raise ValueError('Unexpected runtime DLL: '+dll)
            imports.append(dll)
            at += 20
        else:
            raise ValueError('PE import descriptor budget')
        return {'imports': imports, 'dynamicCrtOrPythonDll': False}
    return {'kind': 'macOS system libraries; signing checked by native build'}


def package(binary, target, output, source_commit):
    crate = Path(__file__).resolve().parents[1]
    binary = Path(binary).resolve(strict=True)
    if machine(binary) != target:
        raise ValueError('Wrong native architecture or binary format')
    link = linkage(binary, target)
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    root = output/'TTE-Native-Core'
    root.mkdir()
    name = 'tte-core.exe' if target.startswith('Windows') else 'tte-core'
    shutil.copyfile(binary, root/name)
    (root/name).chmod(0o755)
    entries = ['RUN-TTE-CORE.bat','RUN-TTE-ADAPTER.bat'] if target.startswith('Windows') else ['RUN-TTE-CORE.sh','RUN-TTE-ADAPTER.sh']
    if target.startswith('macOS'):
        entries.extend(['RUN-TTE-CORE.command','RUN-TTE-ADAPTER.command'])
    # Same root installer bytes travel with every accepted native kit, so the
    # final Workshop combiner never substitutes unreviewed working-tree scripts.
    entries.extend(['INSTALL-TECH-ADAPTER.bat','INSTALL-TECH-ADAPTER.sh','INSTALL-TECH-ADAPTER.command','ADAPTER-README.txt'])
    entries.extend(['OPEN-TECH-TREE.bat','OPEN-TECH-TREE.sh','OPEN-TECH-TREE.command','WEB-ATLAS-README.txt'])
    for entry in entries:
        shutil.copyfile(crate/'launchers'/entry, root/entry)
        (root/entry).chmod(0o755)
    for doc in ('README.md', 'README.zh-CN.md'):
        shutil.copyfile(crate/doc, root/doc)
    triples={'Windows-x86_64':'x86_64-pc-windows-msvc','Linux-x86_64-musl':'x86_64-unknown-linux-musl','macOS-x86_64':'x86_64-apple-darwin','macOS-arm64':'aarch64-apple-darwin'}
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--offline', '--locked', '--format-version', '1','--filter-platform',triples[target]], cwd=crate))
    # Cargo's packages array includes target-inactive packages (e.g. UEFI).
    # Ship notices for the complete selected resolve closure, including build
    # dependencies, rather than failing on unrelated inactive crate archives.
    nodes={n['id']:n for n in metadata['resolve']['nodes']}
    required={metadata['resolve']['root']};pending=list(required)
    while pending:
        for dependency in nodes[pending.pop()]['dependencies']:
            if dependency not in required:
                required.add(dependency);pending.append(dependency)
    notices = root/'THIRD-PARTY'
    notices.mkdir()
    shutil.copyfile(crate/'assets/PYPINYIN-LICENSE.txt', notices/'PYPINYIN-LICENSE.txt')
    shutil.copyfile(crate/'assets/opencc/LICENSE.txt', notices/'OPENCC-LICENSE.txt')
    shutil.copyfile(crate/'assets/opencc/NOTICE.txt', notices/'OPENCC-NOTICE.txt')
    # The static Linux C runtime is not a Cargo crate. Keep its complete upstream
    # notice explicitly; deduplication shares this small text across platform kits.
    shutil.copyfile(crate/'assets/MUSL-COPYRIGHT.txt', notices/'MUSL-COPYRIGHT.txt')
    packages = []
    for row in sorted(metadata['packages'], key=lambda r: (r['name'], r['version'])):
        if row['name'] == 'tte-core' or row['id'] not in required:
            continue
        folder = Path(row['manifest_path']).parent
        texts = {p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE'))}
        if row.get('license_file'):
            texts.add(folder/row['license_file'])
        if not texts:
            raise ValueError('No dependency license text: '+row['name'])
        dest = notices/(row['name']+'-'+row['version'])
        dest.mkdir()
        for text in sorted(texts):
            resolved = text.resolve(strict=True)
            if not resolved.is_relative_to(folder.resolve()):
                raise ValueError('Dependency license escapes crate')
            shutil.copyfile(resolved, dest/text.name)
        packages.append({'name': row['name'], 'version': row['version'], 'license': row['license']})
    (notices/'INDEX.json').write_text(json.dumps(packages, indent=2)+'\n', encoding='utf-8')
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    rust_docs = sysroot/'share/doc/rust'
    shutil.copytree(rust_docs/'licenses', notices/'rust-standard-library/licenses')
    shutil.copyfile(rust_docs/'COPYRIGHT-library.html', notices/'rust-standard-library/COPYRIGHT-library.html')
    manifest = {'schema': 1, 'kind': 'experimental-native-adapter', 'target': target,
                'sourceCommit': source_commit, 'cargoLockSha256': sha(crate/'Cargo.lock'),
                'runtimePython': False, 'gameScriptGeneration': True, 'webAtlas': True, 'nativeGenerationAcceptance': 'PENDING', 'linkage': link,
                'files': {p.relative_to(root).as_posix(): {'bytes': p.stat().st_size, 'sha256': sha(p)}
                          for p in sorted(root.rglob('*')) if p.is_file()}}
    (root/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n', encoding='utf-8')
    archive = output/('TTE-Native-Core-'+target+'-prototype'+('.zip' if target.startswith('Windows') else '.tar.gz'))
    if target.startswith('Windows'):
        with zipfile.ZipFile(archive, 'x', zipfile.ZIP_DEFLATED, compresslevel=9) as kit:
            for path in sorted(root.rglob('*')):
                if path.is_file():
                    kit.write(path, path.relative_to(output).as_posix())
    else:
        def permissions(info):
            if info.isfile():
                info.mode = 0o755 if info.name.rsplit('/',1)[-1] in {'tte-core', *entries} else 0o644
            elif info.isdir():
                info.mode = 0o755
            return info
        with tarfile.open(archive, 'x:gz', compresslevel=9) as kit:
            kit.add(root, arcname=root.name, filter=permissions)
    result = {'archive': str(archive), 'archiveBytes': archive.stat().st_size,
              'archiveSha256': sha(archive), 'binaryBytes': (root/name).stat().st_size,
              'sourceCommit': source_commit, 'target': target, 'linkage': link}
    (output/'package.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--target', required=True, choices=['Windows-x86_64', 'Linux-x86_64-musl', 'macOS-x86_64', 'macOS-arm64'])
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--source-commit', required=True)
    a = p.parse_args()
    print(json.dumps(package(a.binary, a.target, a.output, a.source_commit), indent=2))

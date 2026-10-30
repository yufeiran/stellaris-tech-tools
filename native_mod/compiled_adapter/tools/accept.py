"""Author-side native kit acceptance, using only authored fixtures.

The child runs through the shipped launcher with a minimal PATH and invalid
Python runtime variables. It must not rely on author Python. This file is not
part of the player package.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tarfile
import zipfile
from package import machine, linkage


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def check(archive, output):
    archive = Path(archive).resolve(strict=True)
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    extracted = output/'extracted'
    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as kit:
            for name in kit.namelist():
                if not (extracted/name).resolve().is_relative_to(extracted):
                    raise ValueError('Unsafe ZIP member')
            kit.extractall(extracted)
    else:
        with tarfile.open(archive) as kit:
            kit.extractall(extracted, filter='data')
    root = extracted/'TTE-Native-Core'
    manifest = json.loads((root/'manifest.json').read_text(encoding='utf-8'))
    if manifest['runtimePython'] or not manifest['gameScriptGeneration']:
        raise ValueError('Prototype capability mismatch')
    for name, row in manifest['files'].items():
        path = (root/name).resolve(strict=True)
        if not path.is_relative_to(root) or sha(path) != row['sha256'] or path.stat().st_size != row['bytes']:
            raise ValueError('Package file mismatch: '+name)
    files = {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()}
    if files != set(manifest['files'])|{'manifest.json'}:
        raise ValueError('Unmanifested package file')
    binary = root/('tte-core.exe' if os.name == 'nt' else 'tte-core')
    if machine(binary) != manifest['target'] or linkage(binary, manifest['target']) != manifest['linkage']:
        raise ValueError('Native architecture/linkage mismatch')
    def write(path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')
    game = output/'中文 Steam/steamapps/common/Stellaris'
    source = output/'unlisted source'
    docs = output/'中文 Documents'
    write(game/'common/technology/base.txt', 'parent={area=physics tier=0 cost=10}\n')
    write(game/'interface/owned_resources.gfx','spriteTypes={spriteType={name=GFX_tiling_button_standard} spriteType={name=GFX_tile_outliner_bg} spriteType={name=GFX_tile_outliner_bg_solid} spriteType={name=GFX_ncp_scroll_slider}} bitmapfonts={bitmapfont={name=cg_16b}}')
    write(game/'interface/owned_scrolls.gui','guiTypes={extendedScrollbarType={name=bottom_horizontal_slider} extendedScrollbarType={name=right_vertical_slider} extendedScrollbarType={name=standardtext_slider} extendedScrollbarType={name=standardlistbox_slider}}')
    write(game/'interface/diplomacy_curator_event_view.gui','guiTypes={containerWindowType={name=enclave_curator_window size={width=974 height=680} background={name=background quadTextureSprite=old} containerWindowType={name=EVENT_DIPLO instantTextBoxType={name=action_title} instantTextBoxType={name=action_desc} listboxType={name=option_list}} instantTextBoxType={name=alien_message} buttonType={name=close}} containerWindowType={name=enclave_curator_option buttonType={name=option_button text=OPTION_TEXT} overlappingElementsBoxType={name=trigger_icons}}}')
    write(game/'interface/technology_view.gui','guiTypes={containerWindowType={name=technology_view_window containerWindowType={name=select_window clipping=yes}}}')
    write(game/'interface/paragon_ui_types.gui','guiTypes={containerWindowType={name=paragon_portrait_container_research size={width=10 height=20} clipping=no iconType={name=owned}}}')
    write(source/'descriptor.mod', 'name="Never listed source"')
    write(source/'common/technology/base.txt', 'parent={area=physics tier=0 cost=31}\n')
    write(source/'common/technology/new.txt',
          '@price=22\nTotallyNewNativeProbe={area=engineering tier=1 cost=@price inline_script={script=probe parent=parent}}')
    write(source/'common/inline_scripts/probe.txt', 'prerequisites={$parent$}')
    write(source/'localisation/english/probe_l_english.yml', '\ufeffl_english:\n TotallyNewNativeProbe:0 "Unlisted probe"\n')
    write(source/'localisation/simp_chinese/probe_l_simp_chinese.yml', '\ufeffl_simp_chinese:\n TotallyNewNativeProbe:0 "全新探针"\n')
    write(docs/'mod/probe.mod', 'name="Never listed source"\npath='+json.dumps(source.as_posix(), ensure_ascii=False))
    with sqlite3.connect(docs/'launcher-v2.sqlite') as db:
        db.executescript('create table playsets(id text,name text,isActive integer);'
                        'create table mods(id text,gameRegistryId text,dirPath text,archivePath text,status text);'
                        'create table playsets_mods(playsetId text,modId text,enabled integer,position integer);'
                        "insert into playsets values('fixture','中文 TEST',1);")
        db.execute('insert into mods values(?,?,?,?,?)', ('m', 'mod/probe.mod', str(source), None, 'ready_to_play'))
        db.execute("insert into playsets_mods values('fixture','m',1,0)")
    db.close()
    write(docs/'dlc_load.json', '{"enabled_mods":["mod/probe.mod"]}')
    protected = [p for folder in (game, source, docs) for p in folder.rglob('*') if p.is_file()]
    before = {p: sha(p) for p in protected}
    env = {k: os.environ[k] for k in ('HOME', 'USERPROFILE', 'TEMP', 'TMP', 'TMPDIR', 'LANG', 'LC_ALL', 'SystemRoot', 'WINDIR', 'COMSPEC') if k in os.environ}
    env.update(PYTHONHOME='/no-python', PYTHONPATH='/no-python', TTE_CORE_NO_PAUSE='1')
    if os.name == 'nt':
        env['PATH'] = str(Path(os.environ['SystemRoot'])/'System32')
        entry = [str(Path(os.environ['SystemRoot'])/'System32/cmd.exe'), '/d', '/s', '/c']
    else:
        tools = output/'no-python-path'
        tools.mkdir()
        dirname = shutil.which('dirname')
        if not dirname:
            raise ValueError('Native smoke host needs dirname')
        (tools/'dirname').symlink_to(Path(dirname).resolve())
        env['PATH'] = str(tools)
        entry = ['/bin/sh', str(root/('RUN-TTE-CORE.command' if manifest['target'].startswith('macOS') else 'RUN-TTE-CORE.sh'))]
    arguments = ['--game', str(game), '--documents', str(docs), '--output', str(output/'report.json'), '--goal', 'TotallyNewNativeProbe']
    # CreateProcess receives this command line verbatim. cmd /s /c requires an
    # extra outer quote pair around a quoted .bat path followed by arguments.
    command = (subprocess.list2cmdline(entry)+' "'+subprocess.list2cmdline([str(root/'RUN-TTE-CORE.bat'),*arguments])+'"') if os.name=='nt' else entry+arguments
    with (output/'stdout.log').open('x', encoding='utf-8') as stdout, (output/'stderr.log').open('x', encoding='utf-8') as stderr:
        result = subprocess.run(command, cwd=output, env=env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, timeout=60)
    if result.returncode:
        print((output/'stderr.log').read_text(encoding='utf-8', errors='replace'))
        raise ValueError('Native wrapper fixture failed: '+str(result.returncode))
    report = json.loads((output/'report.json').read_text(encoding='utf-8'))
    techs = report['technologies']
    checks = [len(techs) == 2, techs['parent']['cost'] == 31,
              techs['TotallyNewNativeProbe']['condition'] == ['tech', 'parent'],
              report['localisation']['simp_chinese']['TotallyNewNativeProbe']['name'] == '全新探针',
              report['capabilities']['gameScriptGeneration'] is False,
              report['route']['candidateClosure'] == ['TotallyNewNativeProbe','parent'],
              not shutil.which('python', path=env['PATH']), not shutil.which('python3', path=env['PATH']),
              all(sha(path) == digest for path, digest in before.items())]
    if not all(checks):
        raise ValueError('Native fixture contract failed: '+str(checks))
    generated=output/'local candidate'
    build_args=['--game',str(game),'--documents',str(docs),'--output',str(output/'built-report.json'),'--experimental-build',str(generated)]
    build_command=(subprocess.list2cmdline(entry)+' "'+subprocess.list2cmdline([str(root/'RUN-TTE-CORE.bat'),*build_args])+'"') if os.name=='nt' else entry+build_args
    with (output/'build-stdout.log').open('x',encoding='utf-8') as stdout,(output/'build-stderr.log').open('x',encoding='utf-8') as stderr:
        built=subprocess.run(build_command,cwd=output,env=env,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr,timeout=60)
    if built.returncode:raise ValueError('Native build fixture failed: '+(output/'build-stderr.log').read_text(encoding='utf-8',errors='replace'))
    proof=json.loads((generated/'tte-build.json').read_text(encoding='utf-8'))
    if proof['technologies']!=2 or len(proof['sourceFieldProof'])!=2 or proof['structure']['perTechnologyRouteWindows']!=0 or proof['nativeValidation']!='NOT RUN':raise ValueError('Generated runtime contract mismatch')
    if any(sha(path)!=digest for path,digest in before.items()):raise ValueError('Generation changed protected source/user files')
    # The actual one-click entry must also generate a full candidate without
    # Python in its PATH. Explicit choices prevent silent source omission.
    wizard_entry=root/('RUN-TTE-ADAPTER.bat' if os.name=='nt' else ('RUN-TTE-ADAPTER.command' if manifest['target'].startswith('macOS') else 'RUN-TTE-ADAPTER.sh'))
    wizard_defaults=['--game',str(game),'--documents',str(docs),'--output-parent',str(output/'auto output')]
    wizard_command=(subprocess.list2cmdline(entry)+' "'+subprocess.list2cmdline([str(wizard_entry),*wizard_defaults])+'"') if os.name=='nt' else ['/bin/sh',str(wizard_entry),*wizard_defaults]
    wizard_input='\n'
    with (output/'wizard-stdout.log').open('x',encoding='utf-8') as stdout,(output/'wizard-stderr.log').open('x',encoding='utf-8') as stderr:
        guided=subprocess.run(wizard_command,cwd=output,env=env,input=wizard_input,text=True,encoding='utf-8',stdout=stdout,stderr=stderr,timeout=60)
    if guided.returncode:raise ValueError('Guided native wrapper failed: '+(output/'wizard-stderr.log').read_text(encoding='utf-8',errors='replace'))
    guided_reports=list((output/'auto output').glob('TTE-Adapter-*.json'))
    if len(guided_reports)!=1:raise ValueError('Guided output report missing or duplicated')
    guided_report=json.loads(guided_reports[0].read_text(encoding='utf-8'))
    if guided_report['technologies']['TotallyNewNativeProbe']['condition']!=['tech','parent'] or not guided_report['capabilities']['gameScriptGeneration']:raise ValueError('Guided source/build contract mismatch')
    if any(sha(path)!=digest for path,digest in before.items()):raise ValueError('Guided build changed protected source/user files')
    after = {p for folder in (game, source, docs) for p in folder.rglob('*') if p.is_file()}
    registrations=list((docs/'mod').glob('tte_native_generated_*.mod'))
    if len(registrations)!=1:raise ValueError('Single-Enter wizard did not register exactly one local planner')
    if after != set(before)|set(registrations):
        raise ValueError('Native core changed source or launcher files')
    # Test the actual entry proposed for Workshop distribution. Its binary is
    # nested under the real steamapps/workshop layout; --game is deliberately
    # omitted so discovery must work from the executable's ancestors.
    embedded=game.parents[1]/'workshop/content/281990/3818261596'
    native=embedded/'technology-adapter'/manifest['target']/binary.name
    native.parent.mkdir(parents=True)
    shutil.copyfile(binary,native)
    native.chmod(0o755)
    for name in ('INSTALL-TECH-ADAPTER.bat','INSTALL-TECH-ADAPTER.sh','INSTALL-TECH-ADAPTER.command'):
        shutil.copyfile(root/name,embedded/name)
    owned_zip=embedded/'tte_runtime.zip'
    descriptor=b'name="Tech Tree & Research Planner"\nversion="1.2.0"\nremote_file_id="3818261596"\n'
    with zipfile.ZipFile(owned_zip,'x') as z:
        z.writestr('descriptor.mod',descriptor)
        z.writestr('thumbnail.png',b'authored thumbnail')
        z.writestr('events/old-removed.txt','old=yes')
    old_zip_hash=sha(owned_zip)
    write(embedded/'technology-adapter/WORKSHOP-PACKAGE.json',json.dumps({'kind':'tte-workshop-adapter','itemId':'3818261596','archive':'tte_runtime.zip','archiveSha256':old_zip_hash}))
    # Source fixture never included the planner; registering this owned ID must
    # not add it to collected source technology data or create a new Local Mod.
    write(docs/'mod/ugc_3818261596.mod','name="Planner"\nremote_file_id="3818261596"\narchive='+json.dumps(owned_zip.as_posix()))
    with sqlite3.connect(docs/'launcher-v2.sqlite') as db:
        db.execute("insert into mods values('own','mod/ugc_3818261596.mod',null,?,'ready_to_play')",(str(owned_zip),))
        db.execute("insert into playsets_mods values('fixture','own',1,1)")
    db.close()
    write(docs/'dlc_load.json','{"enabled_mods":["mod/probe.mod","mod/ugc_3818261596.mod"]}')
    same_protected={p:sha(p) for folder in (game,source,docs) for p in folder.rglob('*') if p.is_file()}
    if os.name!='nt':
        for name in ('uname','chmod'):
            tool=shutil.which(name)
            if not tool:raise ValueError('Author host missing '+name)
            (tools/name).symlink_to(Path(tool).resolve())
        # Model mode loss by Steam/Windows upload: the entry must repair only
        # its own binary mode before execution, using ordinary chmod.
        native.chmod(0o644)
    install_entry=embedded/('INSTALL-TECH-ADAPTER.bat' if os.name=='nt' else ('INSTALL-TECH-ADAPTER.command' if manifest['target'].startswith('macOS') else 'INSTALL-TECH-ADAPTER.sh'))
    install_args=['--documents',str(docs),'--output-parent',str(output/'embedded auto output')]
    install_command=(subprocess.list2cmdline(entry)+' "'+subprocess.list2cmdline([str(install_entry),*install_args])+'"') if os.name=='nt' else ['/bin/sh',str(install_entry),*install_args]
    with (output/'embedded-stdout.log').open('x',encoding='utf-8') as stdout,(output/'embedded-stderr.log').open('x',encoding='utf-8') as stderr:
        installed=subprocess.run(install_command,cwd=output,env=env,input='\n',text=True,encoding='utf-8',stdout=stdout,stderr=stderr,timeout=60)
    if installed.returncode:raise ValueError('Embedded entry failed: '+(output/'embedded-stderr.log').read_text(encoding='utf-8',errors='replace'))
    embedded_reports=list((output/'embedded auto output').glob('TTE-Adapter-*.json'))
    if len(embedded_reports)!=1:raise ValueError('Embedded entry output missing')
    embedded_report=json.loads(embedded_reports[0].read_text(encoding='utf-8'))
    if embedded_report['technologies']['TotallyNewNativeProbe']['condition']!=['tech','parent'] or not embedded_report['capabilities']['gameScriptGeneration']:raise ValueError('Embedded full build mismatch')
    if any(sha(path)!=digest for path,digest in same_protected.items()):raise ValueError('Embedded entry changed protected files')
    if len(list((docs/'mod').glob('tte_native_generated_*.mod')))!=1:raise ValueError('Embedded entry must not create another Local Mod')
    with zipfile.ZipFile(owned_zip) as z:
        if z.read('descriptor.mod')!=descriptor or 'events/old-removed.txt' in z.namelist() or 'interface/technology_view.gui' not in z.namelist():raise ValueError('Same-Mod identity or content replacement mismatch')
        if json.loads(z.read('tte-adapter-state.json'))['noNewLauncherRegistration'] is not True:raise ValueError('Same-Mod state missing')
    backups=list((output/'embedded auto output/Workshop-backups').glob('*.zip'))
    if len(backups)!=1 or sha(backups[0])!=old_zip_hash or sha(owned_zip)==old_zip_hash:raise ValueError('Same-Mod backup/replacement failed')
    first_installed_hash=sha(owned_zip)
    write(source/'common/technology/new.txt','@price=22\nFreshParent={area=engineering tier=0 cost=7}\nTotallyNewNativeProbe={area=engineering tier=1 cost=@price inline_script={script=probe parent=FreshParent}}')
    second_protected={p:sha(p) for folder in (game,source,docs) for p in folder.rglob('*') if p.is_file()}
    with (output/'embedded-rerun-stdout.log').open('x',encoding='utf-8') as stdout,(output/'embedded-rerun-stderr.log').open('x',encoding='utf-8') as stderr:
        rerun=subprocess.run(install_command,cwd=output,env=env,input='\n',text=True,encoding='utf-8',stdout=stdout,stderr=stderr,timeout=60)
    if rerun.returncode:raise ValueError('Same-Mod rerun failed: '+(output/'embedded-rerun-stderr.log').read_text(encoding='utf-8',errors='replace'))
    if any(sha(p)!=h for p,h in second_protected.items()):raise ValueError('Rerun changed source or launcher files')
    reports=sorted((output/'embedded auto output').glob('TTE-Adapter-*.json'))
    if len(reports)!=2 or json.loads(reports[-1].read_text(encoding='utf-8'))['technologies']['TotallyNewNativeProbe']['condition']!=['tech','FreshParent']:raise ValueError('Rerun did not discover changed prerequisites')
    backup_hashes={sha(p) for p in (output/'embedded auto output/Workshop-backups').glob('*.zip')}
    if backup_hashes!={old_zip_hash,first_installed_hash}:raise ValueError('Rerun backup mismatch')
    if len(list((docs/'mod').glob('tte_native_generated_*.mod')))!=1:raise ValueError('Rerun created a Local Mod')
    receipt = {'status': 'PASS', 'target': manifest['target'], 'sourceCommit': manifest['sourceCommit'],
               'archiveSha256': sha(archive), 'wrapperLaunchedNativeBinary': True,
               'externalPythonInChildPath': False, 'gameScriptGeneration': True,'generatedRuntimeAuthoredFixture':'PASS','gameRuntimeQa':'NOT RUN',
               'technologyCount': 2, 'unknownIdInlineVariablesOverrideLocalization': 'PASS',
               'sourcesAndLauncherUnchanged': True, 'singleEnterFullBuildAndRegistration':True,
               'embeddedEntrySingleEnter':True,'gameDetectedFromWorkshopLayout':True,
               'sameWorkshopArchiveUpdated':True,'sameModNoNewRegistration':True,
               'sameModRerunChangedPrerequisite':True,
               'hostPlatform':__import__('platform').platform(),'hostMachine':__import__('platform').machine(),
               'nativeGameValidation': 'NOT RUN; authored fixture only'}
    (output/'acceptance.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
    return receipt


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--archive', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    print(json.dumps(check(a.archive, a.output), indent=2))

"""Read-only verification of externally shipped tools; no program execution."""
from pathlib import Path
import argparse,hashlib
p=argparse.ArgumentParser(description=__doc__);p.add_argument('workshop_folder',type=Path)
a=p.parse_args();root=a.workshop_folder.resolve();bad=[];count=0
checks=Path(__file__).resolve().parents[1]/'workshop-1.3.1/SHA256SUMS'
for line in checks.read_text(encoding='utf-8').splitlines():
    digest,name=line.split('  ',1);path=root/name
    if not path.resolve().is_relative_to(root) or path.is_symlink():raise SystemExit('Unsafe path')
    count+=1
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:bad.append(name)
if bad:raise SystemExit('MISMATCH: '+', '.join(bad))
print('PASS: '+str(count)+' external tool/notice files match Workshop1.3.1. No program executed.')

"""Developer-only: recover exact optional display assets from our own viewer."""
from pathlib import Path
import argparse,hashlib,json
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('viewer',type=Path,help='Already installed Workshop Windows viewer executable; it is never executed')
p.add_argument('--restore',action='store_true',help='Write optional display assets into this source checkout; default is verify-only')
a=p.parse_args();data=a.viewer.read_bytes()
meta=json.loads((Path(__file__).with_name('display-assets.json')).read_text(encoding='utf-8'))
if hashlib.sha256(data).hexdigest()!=meta['sourceBinarySha256']:raise SystemExit('Not the exact published Workshop1.3.0 viewer')
for r in meta['files']:
    dest=root/r['file'];blob=data[r['offset']:r['offset']+r['bytes']]
    if not dest.resolve().is_relative_to(root) or dest.is_symlink():raise SystemExit('Unsafe destination')
    if hashlib.sha256(blob).hexdigest()!=r['sha256']:raise SystemExit('Asset hash mismatch')
    if a.restore:dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(blob)
    print(('RESTORED ' if a.restore else 'VERIFIED ')+r['file'])
print('No program was executed. No game files, registrations or saves were changed.')

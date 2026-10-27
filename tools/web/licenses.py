"""Bundle the locked dependency license notices alongside the WASM distribution."""
import html
import json
import pathlib
import subprocess
root = pathlib.Path(__file__).resolve().parents[2]
data = json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1',
    '--filter-platform','wasm32-unknown-unknown'],cwd=root))
parts = ['<!doctype html><meta charset="utf-8"><title>Third-party notices</title><h1>Third-party notices</h1>']
parts.append('<h2>Megawatt Mogul — AGPL-3.0-only</h2><pre>'+html.escape((root/'LICENSE').read_text())+'</pre>')
for package in sorted(data['packages'],key=lambda p:(p['name'],p['version'])):
    if package['id'] in data['workspace_members']:
        continue
    base = pathlib.Path(package['manifest_path']).parent
    notices = sorted(p for p in base.iterdir() if p.is_file() and p.name.lower().startswith(('license','copying','notice')))
    parts.append('<h2>'+html.escape(package['name']+' '+package['version'])+'</h2><p>'+html.escape(package.get('license') or 'See notice')+'</p>')
    for path in notices:
        parts.append('<pre>'+html.escape(path.read_text(errors='replace'))+'</pre>')
(root/'web/assets/licenses.html').write_text('\n'.join(parts))

"""Serve the production artifact beneath the actual Pages subpath (no SPA fallback)."""
import http.server
import pathlib
ROOT = pathlib.Path(__file__).resolve().parents[2] / 'dist'
class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self,*args,**kwargs): super().__init__(*args,directory=str(ROOT),**kwargs)
    def do_GET(self):
        if not self.path.startswith('/megawatt-mogul/'):
            self.send_error(404); return
        self.path = self.path[len('/megawatt-mogul'):]
        super().do_GET()
http.server.ThreadingHTTPServer(('127.0.0.1',8765), Handler).serve_forever()

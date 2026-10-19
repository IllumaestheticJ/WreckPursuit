"""Local dashboard server (stdlib only): static page + a small JSON API.

    GET  /                          dashboard (web/index.html)
    GET  /api/sweeps                finished sweeps
    GET  /api/sweep/<name>          out/sweeps/<name>/sweep.json
    GET  /api/artifact/<name>/<p>   a file under out/sweeps/<name>/ (simcli outputs)
    GET  /api/presets               standard sweeps
    GET  /api/datafiles             files the diff view can pick
    GET  /api/diff?a=..&b=..        field diff of two data files
    GET  /api/jobs                  background sweep status
    POST /api/run      {preset | spec, force}               start a sweep in the background
    POST /api/promote  {sweep, point, note, files, dry_run, force}

Binds to 127.0.0.1 by default. POSTs must be `application/json` from the page's own origin
(a cross-site form cannot promote values into your data files).
"""

from __future__ import annotations

import json
import mimetypes
import threading
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, unquote, urlparse

from . import actions, runner
from . import spec as specmod
from .cli import PRESETS, resolve_spec_path

WEB = Path(__file__).resolve().parents[1] / "web"


class Jobs:
    """At most one background sweep at a time."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.state: dict[str, Any] = {"running": False}

    def snapshot(self) -> dict[str, Any]:
        with self.lock:
            return dict(self.state)

    def start(self, spec_arg: str, force: bool, build: bool) -> dict[str, Any]:
        path = resolve_spec_path(spec_arg)
        sp = specmod.load(path)
        total = len(specmod.points(sp, sp.data or "data"))
        with self.lock:
            if self.state.get("running"):
                return {"ok": False, "error": f"sweep {self.state.get('name')} is still running"}
            self.state = {"running": True, "name": sp.name, "done": 0, "total": total, "last": "", "error": None}

        def progress(done: int, total: int, msg: str) -> None:
            with self.lock:
                self.state.update(done=done, total=total, last=msg)

        def work() -> None:
            try:
                runner.run(sp, force=force, build=build, spec_path=path, progress=progress)
                err = None
            except Exception as e:  # report to the page instead of killing the thread silently
                err = f"{e}\n{traceback.format_exc(limit=3)}"
            with self.lock:
                self.state.update(running=False, error=err)

        threading.Thread(target=work, daemon=True).start()
        return {"ok": True, "name": sp.name, "total": total}


def _sweeps() -> list[dict[str, Any]]:
    out = []
    if runner.SWEEPS.is_dir():
        for d in sorted(runner.SWEEPS.iterdir()):
            f = d / "sweep.json"
            if not f.exists():
                continue
            try:
                doc = json.loads(f.read_text())
            except json.JSONDecodeError:
                continue
            pts = doc.get("points", [])
            out.append(
                {
                    "name": doc["name"],
                    "description": doc.get("description", ""),
                    "probe_kind": doc.get("probe_kind"),
                    "points": len(pts),
                    "failing": sum(1 for p in pts if p.get("passed") is False),
                    "errors": sum(1 for p in pts if not p.get("ok")),
                    "finished_unix": doc.get("finished_unix"),
                }
            )
    return out


def _presets() -> list[dict[str, Any]]:
    out = []
    for p in sorted(PRESETS.glob("*.yaml")):
        try:
            sp = specmod.load(p)
            out.append(
                {
                    "name": p.stem,
                    "description": sp.description,
                    "kind": sp.probe["kind"],
                    "points": len(specmod.points(sp, "data")),
                }
            )
        except specmod.SpecError as e:
            out.append({"name": p.stem, "error": str(e)})
    return out


def make_handler(build: bool, jobs: Jobs) -> type[BaseHTTPRequestHandler]:
    """Request handler bound to one Jobs instance."""

    class Handler(BaseHTTPRequestHandler):
        server_version = "tuning-dash/1"

        def log_message(self, fmt: str, *args: Any) -> None:  # quieter than the default
            if not self.path.startswith("/api/jobs"):
                super().log_message(fmt, *args)

        def _send(self, code: int, body: bytes, ctype: str) -> None:
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def _json(self, obj: Any, code: int = 200) -> None:
            self._send(code, json.dumps(obj).encode(), "application/json")

        def _file(self, path: Path) -> None:
            if not path.is_file():
                self._json({"error": "not found"}, 404)
                return
            ctype = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
            if path.suffix in (".jsonl", ".csv", ".txt", ".ron", ".md"):
                ctype = "text/plain; charset=utf-8"
            self._send(200, path.read_bytes(), ctype)

        def do_GET(self) -> None:  # noqa: N802 (stdlib name)
            u = urlparse(self.path)
            q = {k: v[0] for k, v in parse_qs(u.query).items()}
            parts = [unquote(p) for p in u.path.split("/") if p]
            try:
                if not parts:
                    self._file(WEB / "index.html")
                elif parts[0] == "static" and len(parts) == 2:
                    self._file(_safe_join(WEB, parts[1]))
                elif parts[:2] == ["api", "sweeps"]:
                    self._json(_sweeps())
                elif parts[:2] == ["api", "sweep"] and len(parts) == 3:
                    self._json(actions.load_sweep(parts[2]))
                elif parts[:2] == ["api", "artifact"] and len(parts) >= 4:
                    self._file(_safe_join(runner.SWEEPS / parts[2], *parts[3:]))
                elif parts[:2] == ["api", "presets"]:
                    self._json(_presets())
                elif parts[:2] == ["api", "datafiles"]:
                    self._json(actions.data_files(q.get("data", "data")))
                elif parts[:2] == ["api", "diff"]:
                    self._json(actions.diff_files(q["a"], q["b"], data=q.get("data", "data")))
                elif parts[:2] == ["api", "jobs"]:
                    self._json(jobs.snapshot())
                else:
                    self._json({"error": "not found"}, 404)
            except (FileNotFoundError, KeyError, ValueError) as e:
                self._json({"ok": False, "error": str(e)}, 404)

        def do_POST(self) -> None:  # noqa: N802
            if not self.headers.get("Content-Type", "").startswith("application/json"):
                self._json({"ok": False, "error": "POST bodies must be application/json"}, 415)
                return
            origin = self.headers.get("Origin")
            host = self.headers.get("Host", "")
            if origin and urlparse(origin).netloc != host:
                self._json({"ok": False, "error": "cross-origin request refused"}, 403)
                return
            n = int(self.headers.get("Content-Length", "0") or 0)
            try:
                body = json.loads(self.rfile.read(n) or b"{}")
            except json.JSONDecodeError:
                self._json({"ok": False, "error": "bad JSON"}, 400)
                return
            u = urlparse(self.path)
            try:
                if u.path == "/api/promote":
                    self._json(
                        actions.promote_point(
                            str(body["sweep"]),
                            str(body["point"]),
                            note=str(body.get("note", "")),
                            files=body.get("files") or None,
                            dry_run=bool(body.get("dry_run")),
                            force=bool(body.get("force")),
                        )
                    )
                elif u.path == "/api/run":
                    arg = body.get("preset") or body.get("spec")
                    if not arg:
                        raise KeyError("preset")
                    self._json(jobs.start(str(arg), bool(body.get("force")), build))
                else:
                    self._json({"ok": False, "error": "not found"}, 404)
            except (FileNotFoundError, KeyError, specmod.SpecError, SystemExit) as e:
                self._json({"ok": False, "error": str(e)}, 400)

    return Handler


def _safe_join(root: Path, *parts: str) -> Path:
    p = root.joinpath(*parts).resolve()
    if not p.is_relative_to(root.resolve()):
        raise FileNotFoundError("outside the served folder")
    return p


def serve(host: str = "127.0.0.1", port: int = 8765, build: bool = True) -> None:
    """Serve until Ctrl-C."""
    runner.probe_binary(build)  # build once up front, so promote/diff are instant
    httpd = ThreadingHTTPServer((host, port), make_handler(False, Jobs()))
    print(f"tuning dashboard: http://{host}:{port}/  (Ctrl-C to stop)")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass

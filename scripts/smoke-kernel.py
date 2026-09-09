"""Exercise an isolated controller without opening a proxy port or changing OS settings."""
import argparse
import json
import secrets
import socket
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("binaries", nargs="+")
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
results = []
for item in args.binaries:
    binary = Path(item).resolve(strict=True)
    directory = root / ".tools" / "kernel-smoke" / secrets.token_hex(6)
    directory.mkdir(parents=True)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    secret = secrets.token_hex(24)
    config = directory / "config.yaml"
    config.write_text(f"""mixed-port: 0
allow-lan: false
bind-address: 127.0.0.1
mode: rule
log-level: error
external-controller: 127.0.0.1:{port}
secret: {secret}
tun:
  enable: false
dns:
  enable: false
proxies:
  - {{name: Alpha, type: socks5, server: 127.0.0.1, port: 9}}
  - {{name: Beta, type: socks5, server: 127.0.0.1, port: 9}}
proxy-groups:
  - name: Sufe
    type: select
    proxies: [Alpha, Beta]
rules:
  - DOMAIN-SUFFIX,example.com,Sufe
  - IP-CIDR,192.0.2.0/24,DIRECT,no-resolve
  - MATCH,Sufe
""", encoding="utf-8")
    flags = subprocess.CREATE_NO_WINDOW if hasattr(subprocess, "CREATE_NO_WINDOW") else 0
    validation = subprocess.run([str(binary), "-t", "-d", str(directory), "-f", str(config)], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=30, creationflags=flags)
    if validation.returncode:
        raise RuntimeError(f"Configuration check failed for {binary.name}: {validation.stdout} {validation.stderr}")
    with (directory / "kernel.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen([str(binary), "-d", str(directory), "-f", str(config)], stdout=log, stderr=subprocess.STDOUT, creationflags=flags)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path, body=None, authenticated=True):
            headers = {"Content-Type": "application/json"}
            if authenticated:
                headers["Authorization"] = f"Bearer {secret}"
            req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode() if body else None, headers=headers, method="PUT" if body else "GET")
            with opener.open(req, timeout=3) as response:
                data = response.read(1024 * 1024)
                return json.loads(data) if data else None
        try:
            for attempt in range(50):
                if process.poll() is not None:
                    raise RuntimeError(f"Kernel exited early: {binary.name}")
                try:
                    version = request("/version")
                    break
                except (urllib.error.URLError, TimeoutError):
                    time.sleep(0.1)
            else:
                raise RuntimeError("Controller did not become ready")
            try:
                request("/version", authenticated=False)
                raise AssertionError("Controller accepted an unauthenticated request")
            except urllib.error.HTTPError as error:
                assert error.code == 401
            settings = request("/configs")
            assert settings["mixed-port"] == 0
            assert not settings.get("tun", {}).get("enable", False)
            assert settings["mode"] == "rule"
            group = request("/proxies/Sufe")
            assert group["all"] == ["Alpha", "Beta"]
            request("/proxies/Sufe", {"name": "Beta"})
            assert request("/proxies/Sufe")["now"] == "Beta"
            rules = request("/rules")["rules"]
            assert any(rule.get("payload") == "example.com" and rule.get("proxy") == "Sufe" for rule in rules)
            result = {"binary": binary.name, "version": version, "config_validation": "passed", "controller_auth": "passed", "node_selection": "passed", "custom_rules": "passed", "system_proxy_changed": False, "tun_enabled": False}
            results.append(result)
            print(json.dumps(result, ensure_ascii=False))
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
output = root / "artifacts" / "kernel-smoke.json"
output.parent.mkdir(exist_ok=True)
output.write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")

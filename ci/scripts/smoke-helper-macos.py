#!/usr/bin/env python3
"""Real helper test, exclusively on disposable GitHub-hosted macOS runners.

No subscriptions, remote connections, automatic routes or DNS interception.
The unprivileged runner UID sends IPC; only the shell wrapper installs as root.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

SOCKET = "/Library/Application Support/com.xboard.client/ipc/helper.sock"


def guard():
    assert sys.platform == "darwin", "macOS only"
    assert os.environ.get("GITHUB_ACTIONS") == "true", "CI only"
    assert os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted", "disposable hosted runner only"
    assert os.getuid() != 0, "IPC must run as the installing ordinary user"


def ipc(op, **fields):
    request = {"id": 1, "kind": "request", "op": op, **fields}
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(30)
        client.connect(SOCKET)
        client.sendall(json.dumps(request).encode() + b"\n")
        response = bytearray()
        while not response.endswith(b"\n"):
            part = client.recv(4096)
            if not part:
                raise AssertionError("helper closed before a complete response")
            response.extend(part)
            assert len(response) <= 65536, "oversized helper response"
    decoded = json.loads(response)
    assert decoded.get("id") == 1 and decoded.get("kind") == "response", decoded
    return decoded


def eventually(action, seconds=15):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            return action()
        except (OSError, AssertionError, ValueError) as error:
            last = error
            time.sleep(0.2)
    raise AssertionError(f"readiness deadline: {last}")


def default_routes():
    result = {}
    for family in ("-inet", "-inet6"):
        output = subprocess.run(
            ["/sbin/route", "-n", "get", family, "default"],
            capture_output=True, text=True, timeout=5,
        )
        result[family] = {
            "status": output.returncode,
            "route": [line.strip() for line in output.stdout.splitlines()
                      if line.strip().startswith(("gateway:", "interface:", "destination:", "mask:"))],
        }
    return result


def interface_present():
    output = subprocess.run(["/sbin/ifconfig", "utun1989"], capture_output=True, text=True, timeout=5)
    assert output.returncode == 0 and "198.18.77.1" in output.stdout, output.stdout + output.stderr
    return output.stdout


def interface_absent():
    output = subprocess.run(["/sbin/ifconfig", "utun1989"], capture_output=True, timeout=5)
    assert output.returncode != 0, "test TUN is still present after StopKernel"


def run(report_path, version):
    report = {"runner_uid": os.getuid(), "kernel_expected": version, "checks": [], "passed": False}
    before = default_routes()
    report["default_routes_before"] = before
    secret = secrets.token_hex(32)
    # Keep both reservations open until the configuration is ready, avoiding
    # selecting the same ephemeral port twice. The helper owns its private port.
    with socket.socket() as controller, socket.socket() as mixed:
        controller.bind(("127.0.0.1", 0))
        mixed.bind(("127.0.0.1", 0))
        port = controller.getsockname()[1]
        mixed_port = mixed.getsockname()[1]
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def http(path, method="GET", data=None, authenticated=True):
        headers = {"Content-Type": "application/json"}
        if authenticated:
            headers["Authorization"] = f"Bearer {secret}"
        request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=data, headers=headers, method=method)
        try:
            with opener.open(request, timeout=5) as response:
                return response.status, response.read(65536)
        except urllib.error.HTTPError as error:
            return error.code, error.read(65536)

    yaml = f"""external-controller: 127.0.0.1:{port}
secret: {secret}
mixed-port: {mixed_port}
mode: rule
ipv6: false
find-process-mode: off
geo-auto-update: false
dns: {{enable: false, enhanced-mode: redir-host, fake-ip-range: 198.18.77.1/30, default-nameserver: [127.0.0.1]}}
tun:
  enable: true
  stack: gvisor
  device: utun1989
  auto-route: false
  auto-detect-interface: false
  strict-route: false
  mtu: 1500
  inet4-address: [198.18.77.1/30]
  inet6-address: []
  dns-hijack: []
proxies: [{{name: UnusedLoopback, type: socks5, server: 127.0.0.1, port: 9}}]
rules: ['MATCH,DIRECT']
"""
    try:
        interface_absent()
        pong = eventually(lambda: ipc("ping"))
        assert pong["op"] == "pong" and pong["helper_version"].endswith(f":{version}+ipc2"), pong
        report["helper_version"] = pong["helper_version"]
        report["checks"].append("owner UID Ping and installed kernel/protocol version")
        legacy = ipc("start_kernel", exec_path="/bin/sh", work_dir="/tmp", cfg_path="/etc/passwd", log_path="/tmp/forbidden")
        assert legacy["op"] == "error", legacy
        assert not ipc("status")["running"]
        report["checks"].append("legacy arbitrary-path StartKernel rejected")
        started = ipc("start_kernel_v2", config_yaml=yaml)
        assert started["op"] == "started" and started["pid"] > 0, started
        report["kernel_pid"] = started["pid"]
        status, body = http("/version")
        assert status == 200 and version.lstrip("v") in json.loads(body)["version"], (status, body)
        assert http("/version", authenticated=False)[0] == 401
        assert http("/configs", "PUT", b'{"path":"/etc/passwd"}')[0] == 403
        assert http("/upgrade", "POST", b"{}")[0] == 403
        status, body = http("/configs")
        safe_config = json.loads(body)
        assert status == 200 and "secret" not in safe_config and "external-controller" not in safe_config
        assert secret.encode() not in body
        report["checks"].append("authenticated gateway version, anonymous rejection, forbidden config/upgrade, credential filtering")
        report["ifconfig"] = eventually(interface_present)
        assert default_routes() == before, "default route changed during no-auto-route TUN test"
        report["checks"].append("real utun1989 at 198.18.77.1/30 with default routes unchanged")
        assert ipc("stop_kernel")["op"] == "stopped"
        assert not ipc("status")["running"]
        eventually(interface_absent)
        assert default_routes() == before, "default route changed after StopKernel"
        report["checks"].append("StopKernel stopped process and removed test TUN")
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error).replace(secret, "[REDACTED]")
        raise
    finally:
        try:
            ipc("stop_kernel")
        except Exception:
            pass  # shell trap still bootouts the launch daemon, killing its children
        report["default_routes_after"] = default_routes()
        report_path.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    guard()
    parser = argparse.ArgumentParser()
    parser.add_argument("--stop", action="store_true")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--version")
    args = parser.parse_args()
    if args.stop:
        ipc("stop_kernel")
    else:
        assert args.report and args.version
        run(args.report, args.version)

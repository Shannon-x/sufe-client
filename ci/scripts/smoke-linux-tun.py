#!/usr/bin/env python3
"""CI-only: verify the installed capability kernel's TUN lifecycle, not egress.

No sudo, external requests, DNS listener, providers, or default-route changes.
Only the child started here and its temporary files are cleaned up.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import stat
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

DEVICE = 'sufecitun0'
ADDRESS = '198.18.88.1'
KERNEL = Path('/usr/bin/mihomo')
IP = '/usr/sbin/ip' if Path('/usr/sbin/ip').exists() else '/usr/bin/ip'


def require_ci():
    if (sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('RUNNER_OS') != 'Linux' or os.geteuid() == 0):
        raise RuntimeError('This smoke test requires an ordinary user on a GitHub-hosted Linux runner')


def ip_json(*arguments):
    result = subprocess.run([IP, '-j', *arguments], check=True, capture_output=True, text=True, timeout=5)
    return json.loads(result.stdout)


def routes():
    return {family: sorted(json.dumps(item, sort_keys=True) for item in ip_json(family, 'route', 'show', 'table', 'all', 'default'))
            for family in ('-4', '-6')}


def device_present():
    return any(item.get('ifname') == DEVICE for item in ip_json('link', 'show'))


def wait_for(check, timeout=15):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            value = check()
            if value: return value
        except (OSError, urllib.error.URLError, ValueError) as error:
            last = error
        time.sleep(0.2)
    raise RuntimeError(f'Timed out waiting for {check.__name__}: {last}')


def config_for(port, secret):
    # v1.19.30 derives top-level TUN IPv4 from dns.fake-ip-range, even when
    # DNS is disabled. tun.inet4-address is ignored by that release.
    return f'''external-controller: 127.0.0.1:{port}
secret: {secret}
mixed-port: 0
port: 0
socks-port: 0
allow-lan: false
mode: rule
ipv6: false
find-process-mode: off
geo-auto-update: false
log-level: info
dns:
  enable: false
  enhanced-mode: redir-host
  fake-ip-range: {ADDRESS}/30
  default-nameserver: [127.0.0.1]
  nameserver: [127.0.0.1]
  use-system-hosts: false
ntp: {{enable: false}}
tun:
  enable: true
  stack: gvisor
  device: {DEVICE}
  auto-route: false
  auto-redirect: false
  auto-detect-interface: false
  strict-route: false
  mtu: 1500
  inet6-address: []
  dns-hijack: []
proxies: []
proxy-groups: []
rules: ['MATCH,DIRECT']
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version', required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    require_ci()
    if not Path('/dev/net/tun').exists() or not stat.S_ISCHR(Path('/dev/net/tun').stat().st_mode):
        raise RuntimeError('/dev/net/tun is unavailable')
    if not KERNEL.is_file() or KERNEL.is_symlink() or KERNEL.stat().st_uid != 0:
        raise RuntimeError('Expected the root-owned deb kernel at /usr/bin/mihomo')
    if device_present(): raise RuntimeError(f'{DEVICE} already exists; refusing to interfere')
    if any(info.get('local') == ADDRESS for item in ip_json('address', 'show') for info in item.get('addr_info', [])):
        raise RuntimeError('Test address already in use')
    before = routes()
    report = {'uid': os.geteuid(), 'device': DEVICE, 'address': ADDRESS + '/30', 'checks': []}
    process = None
    # Do not inherit any proxy environment for loopback controller checks.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with tempfile.TemporaryDirectory(prefix='sufe-ci-tun-') as temporary:
        work = Path(temporary)
        secret = secrets.token_hex(32)
        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1', 0))
            port = reserved.getsockname()[1]
            config = work / 'config.yaml'
            config.write_text(config_for(port, secret), encoding='utf-8')
            config.chmod(0o600)
        log = work / 'mihomo.log'
        with log.open('wb') as output:
            try:
                process = subprocess.Popen([str(KERNEL), '-d', str(work), '-f', str(config)],
                                           stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT)

                def controller_ready():
                    if process.poll() is not None:
                        raise RuntimeError(f'Kernel exited with {process.returncode}')
                    request = urllib.request.Request(f'http://127.0.0.1:{port}/version', headers={'Authorization': 'Bearer ' + secret})
                    with opener.open(request, timeout=1) as response:
                        value = json.loads(response.read(8192))
                    if value.get('version') != args.version:
                        raise RuntimeError(f'Unexpected controller version: {value.get("version")}')
                    return value

                report['controller'] = wait_for(controller_ready)
                wait_for(device_present)
                def address_ready():
                    addresses = ip_json('-4', 'address', 'show', 'dev', DEVICE)
                    return any('UP' in item.get('flags', []) and info.get('local') == ADDRESS and info.get('prefixlen') == 30
                               for item in addresses for info in item.get('addr_info', []))
                wait_for(address_ready)
                if routes() != before: raise RuntimeError('Default routes changed during TUN smoke')
                report['checks'].append('non-root capability kernel, authenticated controller version and TUN address; default routes unchanged')
            finally:
                if process is not None and process.poll() is None:
                    process.terminate()
                    try: process.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
                cleanup_ok = False
                try:
                    wait_for(lambda: not device_present(), timeout=5)
                    cleanup_ok = routes() == before
                    if not cleanup_ok: raise RuntimeError('Default routes differ after stopping our child')
                    report['checks'].append('own child stopped, TUN device removed, default routes unchanged after cleanup')
                finally:
                    report['cleanup_ok'] = cleanup_ok
                    args.report.parent.mkdir(parents=True, exist_ok=True)
                    args.report.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
                    # No credentials/config are retained; log contains only this kernel's startup.
                    args.report.with_suffix('.log').write_bytes(log.read_bytes())
    print('Linux TUN lifecycle passed; no airport data-plane or external traffic was tested.')


if __name__ == '__main__': main()

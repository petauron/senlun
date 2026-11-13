#!/usr/bin/env python3
"""Collect one address family's Shanghai TCP routes over an existing SSH alias.
No package installation, rootfs, throughput test, system tuning or report upload.
The output is an administrator-importable Senlun route report, not an attestation.
"""
import argparse
import datetime
import ipaddress
import json
import pathlib
import re
import shlex
import subprocess

REVISION = 'db52fc60523320fc4a34b5c369f28be38df21d13'
SHA256 = '351491ece557ad4c26143b33a38d8b0b50a1872c7b073839e22ee8845771f928'
REMOTE = r'''set -eu
umask 077
family=$1
expected=$2
for tool in bash curl traceroute timeout sha256sum tar awk sed grep sort find head date; do
 command -v "$tool" >/dev/null || { echo "Missing existing tool: $tool; no package was installed" >&2; exit 2; }
done
sudo -n true
work=$(mktemp -d /tmp/senlun-shanghai.XXXXXX)
trap 'sudo -n rm -rf -- "$work"' EXIT HUP INT TERM
url=https://api.ipify.org
[ "$family" = 4 ] || url=https://api6.ipify.org
check_address() {
 actual=$(env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY -u all_proxy curl --noproxy '*' -fsS --max-time 15 -"$family" "$url")
 [ "$actual" = "$expected" ] || { echo 'Actual public egress differs from the selected registered address; report not generated' >&2; exit 3; }
}
check_address
curl --noproxy '*' --proto '=https' --tlsv1.2 -fsSL --max-time 30 'https://raw.githubusercontent.com/ibsgss/TcpQuality/__REVISION__/runTcpQuality-core.sh' -o "$work/core.sh"
printf '%s  %s\n' '__SHA256__' "$work/core.sh" | sha256sum -c - >&2
# Preflight above prevents the upstream dependency helpers from installing tools.
# A private HOME/TMPDIR and empty environment prevent inherited credentials/options.
sudo -n timeout -k 5s 240s env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME="$work" TMPDIR="$work" TERM=dumb LANG=C.UTF-8 bash "$work/core.sh" --route --route-protocol tcp --province sh -p 1 -v"$family" --debug > "$work/run.log" 2>&1 || {
 tail -c 2048 "$work/run.log" >&2
 exit 4
}
check_address
archive=$(find "$work" -maxdepth 1 -type f -name 'tcpquality-results.*.tar.gz' -print | head -n 1)
[ -n "$archive" ] || { echo 'No complete route report' >&2; exit 5; }
member=$(sudo -n tar -tzf "$archive" | grep "/route${family}_final.txt$" | head -n 1)
[ -n "$member" ] || { echo 'No final carrier results' >&2; exit 6; }
printf 'CHECKED_AT=%s\n' "$(date -u +%s)"
sudo -n tar -xOzf "$archive" "$member"
'''.replace('__REVISION__', REVISION).replace('__SHA256__', SHA256)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', required=True, help='Existing trusted SSH alias')
    parser.add_argument('--address', required=True, help='Exact registered public egress address')
    parser.add_argument('--output', required=True, type=pathlib.Path)
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,127}', args.host):
        parser.error('Use a trusted SSH alias, not an SSH command or URL')
    address = ipaddress.ip_address(args.address)
    if not address.is_global:
        parser.error('A public egress address is required')
    if args.output.exists():
        parser.error('Output already exists; choose a new report path')
    command = ['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
               '-o', 'ControlMaster=no', '-o', 'ControlPath=none', '-o', 'ConnectTimeout=10',
               args.host, 'bash -s -- ' + shlex.join([str(address.version), str(address)])]
    result = subprocess.run(command, input=REMOTE, text=True, capture_output=True, timeout=300, check=False)
    if result.returncode:
        raise SystemExit(result.stderr[-4096:] or 'Collection failed; no report generated')
    if len(result.stdout) > 16384:
        raise SystemExit('Unexpectedly large collector output')
    lines = result.stdout.splitlines()
    if not lines or not re.fullmatch(r'CHECKED_AT=\d+', lines[0]):
        raise SystemExit('Invalid collector response')
    checked = int(lines.pop(0).split('=', 1)[1]) * 1000
    names = {'电信': 'telecom', '联通': 'unicom', '移动': 'mobile'}
    carriers = []
    for line in lines:
        status, province, carrier, protocol, target, label = line.split('|', 5)
        if province != '上海' or protocol != 'tcp' or carrier not in names:
            raise SystemExit('Unexpected province, carrier or protocol')
        unknown = status != 'OK' or label in ('Hidden', 'Unknown', '未知', '-')
        carriers.append({'carrier': names[carrier], 'target': target,
                         'status': 'unknown' if unknown else 'ok', 'label': label[:64] or '未知'})
    if len(carriers) != 3 or {entry['carrier'] for entry in carriers} != set(names.values()):
        raise SystemExit('A complete three-carrier result is required')
    report = {'family': address.version, 'address': str(address), 'region': 'SH', 'protocol': 'tcp',
              'source': 'TcpQuality', 'revision': REVISION, 'checked_at_unix_ms': checked, 'carriers': carriers}
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write('\n')
    args.output.chmod(0o600)
    print('Shanghai TCP report saved:', args.output)
    print('Collected:', datetime.datetime.fromtimestamp(checked / 1000, datetime.timezone.utc).isoformat())


if __name__ == '__main__':
    main()

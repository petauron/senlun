# Fixed, reviewed Shanghai TCP collector. No caller-supplied shell code.
set -eu
umask 077
family=$1
expected=$2
allow_temporary=1
for tool in bash curl timeout sha256sum tar awk sed grep sort find head date; do
 command -v "$tool" >/dev/null || { echo "Missing existing tool: $tool; no package was installed" >&2; exit 2; }
done
true
work=$(mktemp -d "${TMPDIR:?}/senlun-shanghai.XXXXXX")
trap 'rm -rf -- "$work"' EXIT HUP INT TERM
tool_path=/usr/sbin:/usr/bin:/sbin:/bin
if ! command -v traceroute >/dev/null; then
 [ "$allow_temporary" = 1 ] || { echo 'Missing traceroute; explicitly allow temporary official tool extraction' >&2; exit 2; }
 for tool in apt-cache dpkg-deb dpkg; do command -v "$tool" >/dev/null || exit 2; done
 . /etc/os-release
 arch=$(dpkg --print-architecture)
 case "$ID:$arch" in
  ubuntu:arm64) origin=https://ports.ubuntu.com/ubuntu-ports ;;
  ubuntu:amd64) origin=https://archive.ubuntu.com/ubuntu ;;
  debian:arm64|debian:amd64) origin=https://deb.debian.org/debian ;;
  *) echo 'Temporary traceroute supports reviewed Debian/Ubuntu architectures only' >&2; exit 2 ;;
 esac
 apt-cache show --no-all-versions traceroute > "$work/package.meta"
 filename=$(awk '/^Filename: / {print $2; exit}' "$work/package.meta")
 digest=$(awk '/^SHA256: / {print $2; exit}' "$work/package.meta")
 version=$(awk '/^Version: / {print $2; exit}' "$work/package.meta")
 case "$filename" in pool/*/traceroute/traceroute_*_"$arch".deb) ;; *) exit 2 ;; esac
 [ "${#digest}" = 64 ] && ! printf '%s' "$digest" | grep -q '[^0-9a-f]' || exit 2
 curl -q --noproxy '*' --proto '=https' --tlsv1.2 -fsSL --max-time 30 --max-filesize 1048576 "$origin/$filename" -o "$work/traceroute.deb"
 printf '%s  %s\n' "$digest" "$work/traceroute.deb" | sha256sum -c - >&2
 [ "$(dpkg-deb -f "$work/traceroute.deb" Package)" = traceroute ] || exit 2
 [ "$(dpkg-deb -f "$work/traceroute.deb" Architecture)" = "$arch" ] || exit 2
 depends=$(dpkg-deb -f "$work/traceroute.deb" Depends)
 printf '%s\n' "$depends" | grep -Eq '^libc6 \((>=|=) [0-9A-Za-z.+:~_-]+\)$' || { echo 'Unexpected traceroute dependencies; review required' >&2; exit 2; }
 dpkg-deb -x "$work/traceroute.deb" "$work/package"
 binary="$work/package/usr/bin/traceroute.db"
 [ -x "$binary" ] || binary="$work/package/usr/bin/traceroute"
 [ -x "$binary" ] || { echo 'Unrecognized traceroute package layout' >&2; exit 2; }
 mkdir "$work/bin"
 ln -s "$binary" "$work/bin/traceroute"
 tool_path="$work/bin:$tool_path"
 printf 'Temporary official traceroute %s verified; no package installed\n' "$version" >&2
fi
url=https://api.ipify.org
[ "$family" = 4 ] || url=https://api6.ipify.org
check_address() {
 actual=$(env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY -u all_proxy curl -q --noproxy '*' -fsS --max-time 15 -"$family" "$url")
 [ "$actual" = "$expected" ] || { echo 'Actual public egress differs from the selected registered address; report not generated' >&2; exit 3; }
}
check_address
curl -q --noproxy '*' --proto '=https' --tlsv1.2 -fsSL --max-time 30 --max-filesize 1048576 'https://raw.githubusercontent.com/ibsgss/TcpQuality/db52fc60523320fc4a34b5c369f28be38df21d13/runTcpQuality-core.sh' -o "$work/core.sh"
printf '%s  %s\n' '351491ece557ad4c26143b33a38d8b0b50a1872c7b073839e22ee8845771f928' "$work/core.sh" | sha256sum -c - >&2
# Preflight above prevents the upstream dependency helpers from installing tools.
# A private HOME/TMPDIR and empty environment prevent inherited credentials/options.
timeout -k 5s 240s env -i PATH="$tool_path" HOME="$work" TMPDIR="$work" TERM=xterm LANG=C.UTF-8 bash "$work/core.sh" --route --route-protocol tcp --province sh -p 1 -v"$family" --debug > "$work/run.log" 2>&1 || {
 tail -c 2048 "$work/run.log" >&2
 exit 4
}
check_address
archive=$(find "$work" -maxdepth 1 -type f -name 'tcpquality-results.*.tar.gz' -print | head -n 1)
[ -n "$archive" ] || { echo 'No complete route report' >&2; exit 5; }
member=$(tar -tzf "$archive" | grep "/route${family}_final.txt$" | head -n 1)
[ -n "$member" ] || { echo 'No final carrier results' >&2; exit 6; }
printf 'CHECKED_AT=%s\n' "$(date -u +%s)"
tar -xOzf "$archive" "$member"

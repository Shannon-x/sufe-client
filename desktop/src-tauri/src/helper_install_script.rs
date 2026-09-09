//! Pure shell generation shared with host-independent regression tests.
use std::path::Path;

pub const BASE: &str = "/Library/Application Support/com.xboard.client";
pub const HELPER: &str = "/Library/Application Support/com.xboard.client/xboard-helper";
pub const PLIST: &str = "/Library/LaunchDaemons/com.xboard.client.helper.plist";

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

const VALIDATE: &str = r#"
set -eu
umask 077
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
LC_ALL=C
export LC_ALL
safe() {
  [ ! -L "$1" ] && [ -e "$1" ] || { echo 'unsafe service path' >&2; exit 1; }
  [ "$(/usr/bin/stat -f '%u' "$1")" = 0 ] || { echo 'service path is not root-owned' >&2; exit 1; }
  mode=$(/usr/bin/stat -f '%Lp' "$1")
  [ $((0$mode & 022)) -eq 0 ] || { echo 'service path is writable by other users' >&2; exit 1; }
  permissions=$(/bin/ls -lde "$1")
  case "${permissions%% *}" in *+*) echo 'unexpected installation ACL' >&2; exit 1;; esac
}
safe /
safe /Library
safe '/Library/Application Support'
safe /Library/LaunchDaemons
"#;

// The elevated process never executes a user-writable staging script. It
// opens the source nonblocking/no-follow, fstats the *open file*, reads a
// bounded regular file, and verifies its pinned hash before any installation.
const COPY: &str = r#"
use strict; use warnings; use Fcntl qw(:DEFAULT :mode); use Digest::SHA;
my ($src,$dst,$hash,$uid)=@ARGV;
die "invalid digest" unless $hash =~ /\A[0-9a-f]{64}\z/;
sysopen(my $in,$src,O_RDONLY|O_NOFOLLOW|O_NONBLOCK) or die "open source: $!";
my @s=stat($in);
die "invalid source" unless @s && S_ISREG($s[2]) && $s[4] == $uid && $s[7] > 0 && $s[7] <= 268435456;
sysopen(my $out,$dst,O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW,0600) or die "create snapshot: $!";
my $digest=Digest::SHA->new(256); my $total=0;
while (1) { my $n=sysread($in,my $buf,65536); die "read: $!" unless defined $n; last unless $n; $total += $n; die "source changed size" if $total > $s[7]; $digest->add($buf); my $offset=0; while($offset<$n) {my $w=syswrite($out,$buf,$n-$offset,$offset); die "write: $!" unless defined $w && $w>0; $offset += $w;} }
close($out) or die "close snapshot: $!";
die "snapshot mismatch" unless $total == $s[7] && $digest->hexdigest eq $hash;
"#;

pub fn install_script(
    helper: &Path,
    kernel: &Path,
    helper_hash: &str,
    kernel_hash: &str,
    uid: u32,
) -> Result<String, String> {
    if uid == 0
        || uid == u32::MAX
        || [helper_hash, kernel_hash]
            .iter()
            .any(|hash| hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("invalid install identity or digest".into());
    }
    let helper = helper.to_str().ok_or("helper path is not UTF-8")?;
    let kernel = kernel.to_str().ok_or("kernel path is not UTF-8")?;
    let mut script = VALIDATE.to_string();
    script.push_str(&format!(r#"
base={base}
if [ ! -e "$base" ]; then /bin/mkdir -m 0755 "$base"; fi
safe "$base"
[ -d "$base" ] || exit 1
stage=$(/usr/bin/mktemp -d "$base/.install.XXXXXXXX")
case "$stage" in "$base"/.install.*) ;; *) exit 1;; esac
trap '/bin/rm -rf "$stage"' EXIT HUP INT TERM
/usr/bin/perl -e {copy} -- {helper} "$stage/xboard-helper" {helper_hash} {uid}
/usr/bin/perl -e {copy} -- {kernel} "$stage/mihomo" {kernel_hash} {uid}
/usr/bin/printf '%s' {plist_xml} > "$stage/daemon.plist"
/usr/bin/printf '%s\n' {uid} > "$stage/owner.uid"
/bin/chmod -N "$stage" "$stage/xboard-helper" "$stage/mihomo" "$stage/daemon.plist" "$stage/owner.uid"
/usr/sbin/chown root:wheel "$stage/xboard-helper" "$stage/mihomo" "$stage/daemon.plist" "$stage/owner.uid"
/bin/chmod 0755 "$stage/xboard-helper" "$stage/mihomo"
/bin/chmod 0644 "$stage/daemon.plist"
/bin/chmod 0600 "$stage/owner.uid"
/usr/bin/plutil -lint "$stage/daemon.plist" >/dev/null
if /bin/launchctl print system/com.xboard.client.helper >/dev/null 2>&1; then /bin/launchctl bootout system/com.xboard.client.helper; fi
for part in ipc state; do
  if [ -e "$base/$part" ]; then safe "$base/$part"; [ -d "$base/$part" ] || exit 1; else /bin/mkdir "$base/$part"; fi
done
/bin/chmod -N "$base/ipc" "$base/state"
/bin/chmod 0755 "$base/ipc"
/bin/chmod 0700 "$base/state"
for file in xboard-helper mihomo owner.uid; do
  if [ -e "$base/$file" ] || [ -L "$base/$file" ]; then safe "$base/$file"; [ -f "$base/$file" ] || exit 1; fi
done
if [ -e {plist_path} ] || [ -L {plist_path} ]; then safe {plist_path}; [ -f {plist_path} ] || exit 1; fi
/bin/mv -f "$stage/xboard-helper" "$base/xboard-helper"
/bin/mv -f "$stage/mihomo" "$base/mihomo"
/bin/mv -f "$stage/owner.uid" "$base/owner.uid"
/bin/mv -f "$stage/daemon.plist" {plist_path}
if [ -e "$base/state/helper.log" ] || [ -L "$base/state/helper.log" ]; then safe "$base/state/helper.log"; [ -f "$base/state/helper.log" ] || exit 1; else /usr/bin/touch "$base/state/helper.log"; fi
/bin/chmod 0600 "$base/state/helper.log"
/bin/launchctl bootstrap system {plist_path}
"#,
        base=shell_quote(BASE), copy=shell_quote(COPY), helper=shell_quote(helper), kernel=shell_quote(kernel),
        helper_hash=shell_quote(&helper_hash.to_ascii_lowercase()), kernel_hash=shell_quote(&kernel_hash.to_ascii_lowercase()), uid=uid,
        plist_xml=shell_quote(&render_plist()), plist_path=shell_quote(PLIST)));
    Ok(script)
}

pub fn uninstall_script() -> String {
    format!(
        r#"{validate}
base={base}
if [ ! -e "$base" ]; then exit 0; fi
safe "$base"
if /bin/launchctl print system/com.xboard.client.helper >/dev/null 2>&1; then /bin/launchctl bootout system/com.xboard.client.helper; fi
for file in xboard-helper mihomo owner.uid; do
  if [ -e "$base/$file" ] || [ -L "$base/$file" ]; then safe "$base/$file"; [ -f "$base/$file" ] || exit 1; /bin/rm -f "$base/$file"; fi
done
if [ -e {plist} ] || [ -L {plist} ]; then safe {plist}; /bin/rm -f {plist}; fi
"#,
        validate = VALIDATE,
        base = shell_quote(BASE),
        plist = shell_quote(PLIST)
    )
}

pub fn render_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.xboard.client.helper</string>
<key>ProgramArguments</key><array><string>{HELPER}</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>ProcessType</key><string>Interactive</string>
<key>AbandonProcessGroup</key><false/>
<key>ExitTimeOut</key><integer>15</integer><key>Umask</key><integer>63</integer>
<key>StandardOutPath</key><string>{BASE}/state/helper.log</string>
<key>StandardErrorPath</key><string>{BASE}/state/helper.log</string>
</dict></plist>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_arguments_do_not_execute_metacharacters() {
        assert_eq!(shell_quote("a'b$(id)`id`\n"), "'a'\\''b$(id)`id`\n'");
        let text = install_script(
            Path::new("/tmp/a'$(id)"),
            Path::new("/tmp/mihomo"),
            &"a".repeat(64),
            &"b".repeat(64),
            501,
        )
        .unwrap();
        assert!(text.contains("'/tmp/a'\\''$(id)'"));
        assert!(text.contains("O_RDONLY|O_NOFOLLOW|O_NONBLOCK"));
        assert!(text.contains("digest->hexdigest eq $hash"));
        assert!(!text.contains("/tmp/xboard-client-install"));
    }
    #[test]
    fn owner_and_hash_cannot_inject_installer_commands() {
        assert!(install_script(
            Path::new("/a"),
            Path::new("/b"),
            "$(id)",
            &"b".repeat(64),
            501
        )
        .is_err());
        assert!(install_script(
            Path::new("/a"),
            Path::new("/b"),
            &"a".repeat(64),
            &"b".repeat(64),
            0
        )
        .is_err());
        assert!(!render_plist().contains("ALLOW_NONROOT"));
    }
}

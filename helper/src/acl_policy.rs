//! Shared, fail-closed policy for protected macOS installation ACLs.
//! Both the elevated installer and the running helper execute this exact
//! parser. A deny ACE can only remove access; no allow ACE is accepted.

pub const VERIFY_LS_ACL: &str = r#"
use strict; use warnings;
my @lines=<STDIN>; chomp @lines;
exit 1 unless @lines;
my $header=shift @lines;
exit 1 unless $header =~ /\A[bcdlps-][r-][w-][xsS-][r-][w-][xsS-][r-][w-][xtT-]([@+]?)[ \t]+\S.*\z/;
my $marker=$1;
exit($marker eq '+' ? 1 : 0) unless @lines;
my %known=map { $_=>1 } qw(read write execute append delete list add_file search add_subdirectory delete_child readattr writeattr readextattr writeextattr readsecurity writesecurity chown file_inherit directory_inherit limit_inherit only_inherit);
my $index=0;
for my $line (@lines) {
  exit 1 unless $line =~ /\A\s*([0-9]+): (?:user|group):([^\s,:]+) (?:inherited )?deny ([a-z_,]+)\z/;
  my ($number,$principal,$rights)=($1,$2,$3);
  exit 1 unless $number == $index++;
  my @rights=split /,/, $rights, -1;
  exit 1 unless @rights;
  my %seen;
  for my $right (@rights) { exit 1 unless $known{$right} && !$seen{$right}++; }
}
exit 0;
"#;

#[cfg(any(target_os = "macos", test))]
pub fn verify_listing(listing: &[u8]) -> std::io::Result<bool> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    if listing.len() > 64 * 1024 {
        return Ok(false);
    }
    #[cfg(not(windows))]
    let interpreter = "/usr/bin/perl";
    #[cfg(windows)]
    let interpreter = "C:/Program Files/Git/usr/bin/perl.exe";
    let mut child = Command::new(interpreter)
        .env_clear()
        .env("LC_ALL", "C")
        .args(["-e", VERIFY_LS_ACL])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child.stdin.take().unwrap().write_all(listing)?;
    Ok(child.wait()?.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "drwxr-xr-x+  5 root  wheel  160 Sep 9 10:00 /Library\n";

    #[test]
    fn accepts_only_unambiguous_protective_deny_entries() {
        for entries in [
            " 0: group:everyone deny delete\n",
            " 0: group:everyone deny delete\n 1: user:runner inherited deny write,append,delete_child\n",
            " 0: user:deny deny write\n",
        ] {
            assert!(verify_listing(format!("{HEADER}{entries}").as_bytes()).unwrap());
        }
    }

    #[test]
    fn validates_acl_entries_even_when_xattrs_hide_the_acl_marker() {
        // Apple ls prefers '@' over '+' when both xattrs and ACLs exist, but
        // -e still prints the ACL entries. The suffix must never bypass them.
        for marker in ["", "@", "+"] {
            let header = HEADER.replace('+', marker);
            assert!(
                verify_listing(format!("{header} 0: group:everyone deny delete\n").as_bytes())
                    .unwrap()
            );
            assert!(
                !verify_listing(format!("{header} 0: user:runner allow write\n").as_bytes())
                    .unwrap()
            );
            assert_eq!(verify_listing(header.as_bytes()).unwrap(), marker != "+");
        }
        assert!(!verify_listing(HEADER.replace('+', "@+").as_bytes()).unwrap());
        assert!(!verify_listing(HEADER.replace('+', ".").as_bytes()).unwrap());
        assert!(!verify_listing(HEADER.replace("drwxr-xr-x+", "dwwxw-xw-x").as_bytes()).unwrap());
    }

    #[test]
    fn rejects_grants_ambiguous_principals_and_unknown_permissions() {
        for entries in [
            "",
            " 0: group:everyone allow write\n",
            " 0: group:everyone deny delete\n 1: user:runner allow write\n",
            " 0: user:deny allow write\n",
            " 0: user:name deny delete allow write\n",
            " 0: user:name deny delete:allow write\n",
            " 0: user:name deny,allow write\n",
            " 0: user:name with spaces deny delete\n",
            " 0: group:everyone deny delete,unknown\n",
            " 0: group:everyone deny delete,\n",
            " 0: group:everyone deny delete,delete\n",
            " 1: group:everyone deny delete\n",
            " 0: group:everyone deny delete\n 0: group:everyone deny write\n",
            " 0: group:everyone deny delete\n\n",
        ] {
            assert!(
                !verify_listing(format!("{HEADER}{entries}").as_bytes()).unwrap(),
                "{entries}"
            );
        }
        assert!(!verify_listing(b"changed header\n 0: group:everyone deny delete\n").unwrap());
    }
}

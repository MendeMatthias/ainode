//! Input checks for everything the UI (or a config file) hands us before it
//! reaches the helper, a child process, or the filesystem.
//!
//! The helper's own parser (`ParseHostPort`, btx src/modelnet/model_nat.cpp:31)
//! is deliberately loose: it splits on the LAST colon, strips brackets, and
//! `stoi`s the port, so `foo bar:12abc` is a valid peer to it. Peers persist in
//! the helper's catalog (`ModelCatalog::AddPeer`, catalog.cpp:1172, capped at
//! 64), so junk typed once would be dialled forever. We accept only the three
//! shapes a person means: an IPv4 address, a bracketed IPv6 address, or a DNS
//! name, each with a non-zero port.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Validate and normalise a `host:port` peer. Returns the trimmed form the
/// helper is given (`[::1]:29447`, `203.0.113.7:29447`, `host.example:29447`).
pub fn host_port(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Enter a host and port, like host.example:29447".into());
    }
    if s.len() > 260 {
        return Err("That address is too long".into());
    }
    // Said in their own words: both used to get the IPv6 hint below, which
    // told someone who pasted a link to put brackets round it.
    if s.contains("://") {
        return Err("Type the host and its port only, without http:// or btx://: host.example:29447".into());
    }
    if s.contains(char::is_whitespace) {
        return Err("One host and its port, without spaces: host.example:29447".into());
    }
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| "An IPv6 address needs its closing bracket: [::1]:29447".to_string())?;
        let (v6, after) = rest.split_at(close);
        let port = after
            .strip_prefix("]:")
            .ok_or_else(|| "Add the port after the bracket: [::1]:29447".to_string())?;
        let addr = v6.parse::<Ipv6Addr>().map_err(|_| format!("{v6} is not an IPv6 address"))?;
        // Printed back in its one canonical form: [2001:DB8:0::1] and
        // [2001:db8::1] are the same host and must be one entry, and an
        // IPv4-mapped address ([::ffff:192.0.2.1]) is its IPv4 address.
        match addr.to_ipv4_mapped() {
            Some(v4) => (v4.to_string(), port),
            None => (format!("[{addr}]"), port),
        }
    } else {
        let (host, port) = s
            .rsplit_once(':')
            .ok_or_else(|| "Add the port: host.example:29447".to_string())?;
        if host.contains(':') {
            // Bare IPv6 is ambiguous: is the last group the port or the address?
            return Err("Put an IPv6 address in brackets: [2001:db8::1]:29447".into());
        }
        if !is_ipv4_or_hostname(host) {
            return Err(format!("{host} is not a valid host name or IPv4 address"));
        }
        // An IPv4 address in its canonical dotted form; a name in lower case.
        let host = match host.parse::<Ipv4Addr>() {
            Ok(a) => a.to_string(),
            Err(_) => host.to_ascii_lowercase(),
        };
        (host, port)
    };
    let port = parse_port(port)?;
    Ok(format!("{host}:{port}"))
}

fn parse_port(p: &str) -> Result<u16, String> {
    if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{p:?} is not a port number"));
    }
    match p.parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => Ok(n as u16),
        _ => Err(format!("Port {p} is out of range (1 to 65535)")),
    }
}

/// RFC 1123 host name, or a dotted-quad that actually parses. An all-numeric
/// name that is not a valid IPv4 address (`999.1.1.1`) is refused rather than
/// handed to the resolver.
fn is_ipv4_or_hostname(h: &str) -> bool {
    // ASCII only (a name with "é" is refused, not resolved): and every slice
    // below is then on a character boundary. 0.9.1's first build sliced
    // "hé" at byte 2 and panicked.
    if h.is_empty() || h.len() > 253 || !h.is_ascii() {
        return false;
    }
    let labels: Vec<&str> = h.split('.').collect();
    let digits = |l: &str| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit());
    if labels.iter().all(|l| digits(l)) {
        return h.parse::<Ipv4Addr>().is_ok();
    }
    // A name an address parser could read as a number: `0x7f.1`, `127.1`
    // with a word in front, a hex label. No real host name has a numeric last
    // label or a 0x label, so these are refused rather than resolved.
    if labels.last().is_some_and(|l| digits(l))
        || labels.iter().any(|l| l.len() > 2 && l[..2].eq_ignore_ascii_case("0x") && l[2..].bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return false;
    }
    labels.iter().all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// Only plain https:// links leave the app, and only to the default browser
/// via `open`. Anything else (file:, javascript:, a bare path that `open`
/// would launch as an app) is refused.
pub fn https_url(input: &str) -> Result<String, String> {
    let s = input.trim();
    let rest = s
        .strip_prefix("https://")
        .ok_or_else(|| "Only https:// links can be opened".to_string())?;
    if s.len() > 2048 {
        return Err("That link is too long".into());
    }
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("That link contains spaces or control characters".into());
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // `https://easybtx.com@evil.example/` reads as easybtx.com and opens
    // evil.example: no AInode link carries a user name (security audit I1).
    if host.contains('@') {
        return Err("That link names a user before its host".into());
    }
    if host.is_empty() || host.starts_with(':') {
        return Err("That link has no host".into());
    }
    Ok(s.to_string())
}

/// `path` is strictly inside `root` (not `root` itself), judged on real,
/// symlink-resolved paths. A path that does not exist yet is resolved through
/// its nearest existing ancestor; a `..` in the part that does not exist makes
/// it unjudgeable, which counts as outside. Guards the one place the app
/// deletes a directory.
pub fn strictly_inside(root: &Path, path: &Path) -> bool {
    match (resolve(root), resolve(path)) {
        (Some(root), Some(path)) => path != root && path.starts_with(&root),
        _ => false,
    }
}

fn resolve(p: &Path) -> Option<PathBuf> {
    if !p.is_absolute() {
        return None;
    }
    let mut existing = p.to_path_buf();
    let mut rest: Vec<OsString> = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        // file_name() is None for a path ending in `..`.
        rest.push(existing.file_name()?.to_os_string());
        if !existing.pop() {
            return None;
        }
    }
    let mut base = std::fs::canonicalize(&existing).ok()?;
    for n in rest.into_iter().rev() {
        base.push(n);
    }
    Some(base)
}

/// A `btx://` MODEL link as a person pastes it, checked the way upstream's
/// `DecodeResource` does (btx src/modelnet/resource_uri.cpp:104-161, read)
/// and stricter: lowercase `btx://`, 85 characters of the Bech32 alphabet, a
/// valid Bech32m checksum over `"btx1" + token`, version 1, kind 0 (MODEL),
/// a 48-byte digest with no stray bits. Surrounding space and one trailing
/// `/` are dropped. Nothing else ever reaches the helper: its `getmodel` and
/// `getmodelsharecard` read a local file when handed a path
/// (helper.cpp:5727-5735, 7138-7142, read), and an alias can wedge it
/// (btxchain/btx#225).
pub fn model_uri(input: &str) -> Result<String, String> {
    const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    let s = input.trim();
    let s = s.strip_suffix('/').unwrap_or(s);
    let token = s
        .strip_prefix("btx://")
        .ok_or("A BTX model link starts with btx://")?;
    if token.len() != 85 {
        return Err("That is not a whole BTX model link: it has the wrong length".into());
    }
    let mut data = Vec::with_capacity(85);
    for b in token.bytes() {
        let v = CHARSET.iter().position(|&c| c == b).ok_or("That link has a character a BTX link never has")?;
        data.push(v as u8);
    }
    // Bech32m (BIP-350) over hrp "btx" and the 85 groups, checksum included.
    let hrp = b"btx";
    let mut values: Vec<u8> = hrp.iter().map(|c| c >> 5).collect();
    values.push(0);
    values.extend(hrp.iter().map(|c| c & 31));
    values.extend(&data);
    if bech32_polymod(&values) != 0x2bc8_30a3 {
        return Err("That link is mistyped: its checksum does not match".into());
    }
    let payload = &data[..data.len() - 6];
    if payload[0] != 1 {
        return Err("That link is of a version AInode does not know".into());
    }
    if payload[1] != 0 {
        return Err("That is a BTX link, but not to a model (a bounty or another record)".into());
    }
    // 77 groups of 5 bits = 385 bits: 48 bytes and one spare bit, which must be 0.
    let digest = &payload[2..];
    if digest.len() != 77 || digest[76] & 1 != 0 {
        return Err("That link is not canonical".into());
    }
    Ok(format!("btx://{token}"))
}

fn bech32_polymod(values: &[u8]) -> u32 {
    const GEN: [u32; 5] = [0x3b6a_57b2, 0x2650_8e6d, 0x1ea1_19fa, 0x3d42_33dd, 0x2a14_62b3];
    let mut chk: u32 = 1;
    for &v in values {
        let top = chk >> 25;
        chk = ((chk & 0x01ff_ffff) << 5) ^ u32::from(v);
        for (i, g) in GEN.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

/// A folder the user chose for AInode's data at the first start: a full path
/// on a local drive, not inside AInode's own program folder.
pub fn data_dir(input: &str, engine_dir: &Path) -> Result<PathBuf, String> {
    let p = PathBuf::from(input.trim());
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("Choose a full path for AInode's folder".into());
    }
    #[cfg(windows)]
    if crate::wsl::linux_path(&p).is_none() {
        return Err("AInode's folder must be on a local drive: a network place cannot hold its engine's files".into());
    }
    // The program folder: two above `<install>/resources/engine`; a staged
    // engine folder anywhere else (dev builds, tests) stands for itself.
    let program = if engine_dir.ends_with(Path::new("resources").join("engine")) {
        engine_dir.parent().and_then(Path::parent).unwrap_or(engine_dir)
    } else {
        engine_dir
    };
    if resolved(&p).starts_with(resolved(program)) {
        return Err("Choose a folder outside AInode's own program folder".into());
    }
    Ok(p)
}

/// `p` with its deepest existing ancestor made canonical (links followed,
/// Windows' `\\?\` prefix dropped) and the rest appended as given, so two
/// spellings of one place compare equal whether the folder exists yet or not.
fn resolved(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(c) = std::fs::canonicalize(cur) {
            let c = PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\"));
            return rest.iter().rev().fold(c, |acc, part| acc.join(part));
        }
        match (cur.file_name(), cur.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_dir_choice_is_a_full_local_path_outside_the_program() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = tmp.path().join("app").join("resources").join("engine");
        std::fs::create_dir_all(&engine).unwrap();
        let elsewhere = tmp.path().join("models");
        assert_eq!(data_dir(&elsewhere.to_string_lossy(), &engine).unwrap(), elsewhere);
        assert!(data_dir("relative/dir", &engine).is_err());
        assert!(data_dir(&tmp.path().join("x").join("..").join("y").to_string_lossy(), &engine).is_err());
        assert!(data_dir(&tmp.path().join("app").join("data").to_string_lossy(), &engine).is_err(), "inside the program folder");
        #[cfg(windows)]
        assert!(data_dir(r"\\server\share\AInode", &engine).is_err());
    }

    #[test]
    fn host_port_accepts_the_three_shapes() {
        assert_eq!(host_port("127.0.0.1:29447").unwrap(), "127.0.0.1:29447");
        assert_eq!(host_port("  [::1]:29447 ").unwrap(), "[::1]:29447");
        assert_eq!(host_port("[2001:db8::7]:1").unwrap(), "[2001:db8::7]:1");
        assert_eq!(host_port("Host.Example.org:29447").unwrap(), "host.example.org:29447");
        assert_eq!(host_port("localhost:65535").unwrap(), "localhost:65535");
        assert_eq!(host_port("a-b.c1:80").unwrap(), "a-b.c1:80");
    }

    #[test]
    fn host_port_says_what_is_wrong_in_plain_words() {
        assert!(host_port("http://host.example:29447").unwrap_err().contains("without http://"));
        assert!(host_port("btx://host.example:29447").unwrap_err().contains("without http://"));
        assert!(host_port("a.example:1 b.example:2").unwrap_err().contains("without spaces"));
        assert!(host_port("a.example : 1").unwrap_err().contains("without spaces"));
        assert!(host_port("2001:db8::1:29447").unwrap_err().contains("brackets"));
    }

    #[test]
    fn host_port_gives_one_form_per_address() {
        // The same IPv6 host, written three ways: one entry.
        for v in ["[2001:DB8::1]:1", "[2001:db8::1]:1", "[2001:db8:0::1]:1", "[2001:0db8:0000:0000:0000:0000:0000:0001]:1"] {
            assert_eq!(host_port(v).unwrap(), "[2001:db8::1]:1", "{v}");
        }
        assert_eq!(host_port("[0:0:0:0:0:0:0:1]:29447").unwrap(), "[::1]:29447");
        // An IPv4-mapped address is its IPv4 address, in every spelling.
        for v in ["[::FFFF:192.0.2.1]:1", "[::ffff:c000:201]:1", "[0:0:0:0:0:ffff:192.0.2.1]:1", "192.0.2.1:1"] {
            assert_eq!(host_port(v).unwrap(), "192.0.2.1:1", "{v}");
        }
        assert_eq!(host_port("192.0.2.1:080").unwrap(), "192.0.2.1:80");
        assert_eq!(host_port("HOST.example:1").unwrap(), host_port("host.EXAMPLE:1").unwrap());
    }

    #[test]
    fn host_port_refuses_what_the_helper_would_swallow() {
        for bad in [
            "",
            "host.example",
            "host.example:",
            ":29447",
            "host.example:0",
            "host.example:65536",
            "host.example:12abc",
            "host.example:+80",
            "foo bar:29447",
            "::1:29447",
            "[::1]29447",
            "[::1:29447",
            "[not-v6]:29447",
            "999.1.1.1:29447",
            "1.2.3:29447",
            "-lead.example:29447",
            "trail-.example:29447",
            "a..b:29447",
            "under_score.example:29447",
            "host.example:29447/path",
            // Names an address parser would read as a number.
            "0x7f.1:29447",
            // Not ASCII: refused, and no panic (0.9.1's first build sliced inside the "é").
            "h\u{e9}st.example:29447",
            "\u{e9}:1",
            "0\u{e9}x.example:1",
            "\u{1F600}.example:29447",
            "0x7f000001:29447",
            "127.1:29447",
            "host.127:29447",
            "a.0xff:29447",
            "2130706433:29447",
            "0177.0.0.1:29447",
            "1.2.3.4.5:29447",
        ] {
            assert!(host_port(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn https_url_only() {
        assert!(https_url("https://huggingface.co/ibm-granite/granite-4.0-350m-GGUF").is_ok());
        assert!(https_url("https://www.apache.org/licenses/LICENSE-2.0").is_ok());
        assert!(https_url("https://github.com/x/y/issues/new?body=a@b").is_ok(), "an @ after the host is fine");
        for bad in [
            "http://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "/Applications/Calculator.app",
            "https://",
            "https:///path",
            "https://exa mple.com",
            "https://example.com/\nx",
            "HTTPS://example.com",
            "https://easybtx.com@evil.example/nodes",
            "https://user:pass@example.com",
        ] {
            assert!(https_url(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn strictly_inside_refuses_escapes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("modelnet/checkout");
        let inside = root.join("abc123");
        std::fs::create_dir_all(&inside).unwrap();
        let outside = tmp.path().join("modelnet/store");
        std::fs::create_dir_all(&outside).unwrap();

        assert!(strictly_inside(&root, &inside));
        assert!(!strictly_inside(&root, &root), "the checkout root itself");
        assert!(!strictly_inside(&root, &outside));
        assert!(!strictly_inside(&root, &root.join("../store")));
        assert!(!strictly_inside(&root, &root.join("abc123/../../store")));
        assert!(!strictly_inside(&root, Path::new("/")));
        assert!(!strictly_inside(&root, Path::new("relative/abc")));
        // Not created yet: judged through the real (canonical) ancestor, so a
        // /var vs /private/var symlink in TMPDIR does not matter.
        assert!(strictly_inside(&root, &root.join("not-yet")));
        assert!(!strictly_inside(&root, &root.join("missing/../../store")));
        // A symlink inside the root that points out of it is outside.
        let link = root.join("sneaky");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(!strictly_inside(&root, &link));
        // A name that merely shares the prefix is not inside.
        let sibling = tmp.path().join("modelnet/checkout-evil");
        std::fs::create_dir_all(&sibling).unwrap();
        assert!(!strictly_inside(&root, &sibling));
    }

    #[test]
    fn model_links_are_checked_like_upstream_and_stricter() {
        // Every catalog link is valid (they came from the helper itself).
        for m in crate::catalog::bundled().models {
            assert_eq!(model_uri(&m.uri).as_deref(), Ok(m.uri.as_str()), "{}", m.id);
            assert_eq!(model_uri(&format!("  {}/ ", m.uri)).as_deref(), Ok(m.uri.as_str()));
        }
        let good = crate::catalog::bundled().models[0].uri.clone();
        // One character changed: the checksum catches it.
        let mut bad = good.clone().into_bytes();
        let last = bad.len() - 10;
        bad[last] = if bad[last] == b'q' { b'p' } else { b'q' };
        assert!(model_uri(std::str::from_utf8(&bad).unwrap()).is_err());
        assert!(model_uri(&good.to_uppercase()).is_err(), "only lowercase");
        assert!(model_uri(&good[6..]).is_err(), "the btx:// prefix is required");
        assert!(model_uri(&good[..good.len() - 1]).is_err());
        assert!(model_uri("btx:///etc/passwd").is_err());
        assert!(model_uri("/Users/me/model.btx").is_err());
        assert!(model_uri("btx://pq1notours").is_err());
        assert!(model_uri("").is_err());
    }
}

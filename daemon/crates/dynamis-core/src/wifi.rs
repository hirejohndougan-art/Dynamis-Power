//! Wi-Fi power saving through the kernel's wireless interface (nl80211),
//! decision 0018.
//!
//! The state is not a file: it is read and set with two generic netlink
//! requests to the local kernel (`NL80211_CMD_GET_POWER_SAVE` and
//! `NL80211_CMD_SET_POWER_SAVE`, with `NL80211_ATTR_IFINDEX` and
//! `NL80211_ATTR_PS_STATE`). Netlink talks only to the kernel; nothing here
//! opens a network connection. No scanning, connecting or reading network
//! names: only the power-save state of an interface.
//!
//! Tests never reach the real kernel (safety rule 8): under a fixture root
//! the state lives in `/run/dynamis-test/nl80211/<interface>/power_save`
//! inside the fixture.

use std::path::Path;

use crate::fs::SysRoot;

const NET: &str = "/sys/class/net";
const FIXTURE: &str = "/run/dynamis-test/nl80211";

/// Wireless interfaces (those with a `phy80211` link or `wireless` directory).
pub fn interfaces(root: &SysRoot) -> Vec<String> {
    let mut out: Vec<String> = root
        .list_dir(NET)
        .into_iter()
        .filter(|n| valid_name(n))
        .filter(|n| {
            root.exists(&format!("{NET}/{n}/phy80211"))
                || root.exists(&format!("{NET}/{n}/wireless"))
        })
        .collect();
    out.sort();
    out
}

/// An interface name the kernel could have made: short, no slashes.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 15
        && n != "."
        && n != ".."
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}

fn is_real(root: &SysRoot) -> bool {
    root.root() == Path::new("/")
}

/// Whether power saving is on for `ifname`.
pub fn get(root: &SysRoot, ifname: &str) -> Result<bool, String> {
    if !valid_name(ifname) {
        return Err(format!("not an interface name: {ifname}"));
    }
    if !is_real(root) {
        let v = root
            .read_string(&format!("{FIXTURE}/{ifname}/power_save"))
            .map_err(|e| e.to_string())?;
        return Ok(v.trim() == "on");
    }
    let idx = ifindex(root, ifname)?;
    nl::get_power_save(idx)
}

/// Switch power saving on or off for `ifname`. Only the writer calls this,
/// after its allow-list check.
pub(crate) fn set(root: &SysRoot, ifname: &str, on: bool) -> Result<(), String> {
    if !valid_name(ifname) {
        return Err(format!("not an interface name: {ifname}"));
    }
    if !is_real(root) {
        let p = root.path(&format!("{FIXTURE}/{ifname}/power_save"));
        if !p.exists() {
            return Err(format!("{}: no such file", p.display()));
        }
        return std::fs::write(p, if on { "on\n" } else { "off\n" }).map_err(|e| e.to_string());
    }
    let idx = ifindex(root, ifname)?;
    nl::set_power_save(idx, on)
}

fn ifindex(root: &SysRoot, ifname: &str) -> Result<u32, String> {
    root.read_u64(&format!("{NET}/{ifname}/ifindex"))
        .map(|v| v as u32)
        .map_err(|e| format!("{ifname}: {e}"))
}

/// For tests: make a wireless interface in a fixture.
pub mod fixture {
    use std::path::Path;

    pub fn add(root: &Path, ifname: &str, ifindex: u32, on: bool, driver: &str) {
        let net = root.join("sys/class/net").join(ifname);
        std::fs::create_dir_all(net.join("phy80211")).unwrap();
        std::fs::create_dir_all(net.join("wireless")).unwrap();
        std::fs::create_dir_all(net.join("device")).unwrap();
        std::fs::write(net.join("ifindex"), format!("{ifindex}\n")).unwrap();
        let drv = root.join("sys/bus/pci/drivers").join(driver);
        std::fs::create_dir_all(&drv).unwrap();
        let _ = std::fs::remove_file(net.join("device/driver"));
        std::os::unix::fs::symlink(&drv, net.join("device/driver")).unwrap();
        let ps = root.join("run/dynamis-test/nl80211").join(ifname);
        std::fs::create_dir_all(&ps).unwrap();
        std::fs::write(ps.join("power_save"), if on { "on\n" } else { "off\n" }).unwrap();
    }
}

/// Generic netlink, by hand: two requests do not justify a dependency.
mod nl {
    use std::mem::size_of;

    const NETLINK_GENERIC: i32 = 16;
    const GENL_ID_CTRL: u16 = 16;
    const CTRL_CMD_GETFAMILY: u8 = 3;
    const CTRL_ATTR_FAMILY_ID: u16 = 1;
    const CTRL_ATTR_FAMILY_NAME: u16 = 2;
    const NL80211_CMD_SET_POWER_SAVE: u8 = 61;
    const NL80211_CMD_GET_POWER_SAVE: u8 = 62;
    const NL80211_ATTR_IFINDEX: u16 = 3;
    const NL80211_ATTR_PS_STATE: u16 = 93;
    const NLM_F_REQUEST: u16 = 1;
    const NLM_F_ACK: u16 = 4;
    const NLMSG_ERROR: u16 = 2;
    const HDR: usize = 16 + 4; // nlmsghdr + genlmsghdr

    /// A netlink socket, closed on drop.
    struct Sock(i32);

    impl Drop for Sock {
        fn drop(&mut self) {
            // SAFETY: the descriptor is ours and closed once.
            unsafe { libc::close(self.0) };
        }
    }

    fn open() -> Result<Sock, String> {
        // SAFETY: plain socket(2) call; the result is checked.
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                NETLINK_GENERIC,
            )
        };
        if fd < 0 {
            return Err(format!(
                "cannot open a netlink socket: {}",
                std::io::Error::last_os_error()
            ));
        }
        let s = Sock(fd);
        let tv = libc::timeval {
            tv_sec: 1,
            tv_usec: 0,
        };
        // SAFETY: tv lives across the call and its size is passed.
        unsafe {
            libc::setsockopt(
                s.0,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &tv as *const _ as *const libc::c_void,
                size_of::<libc::timeval>() as u32,
            )
        };
        Ok(s)
    }

    fn attr(buf: &mut Vec<u8>, kind: u16, data: &[u8]) {
        let len = 4 + data.len();
        buf.extend_from_slice(&(len as u16).to_ne_bytes());
        buf.extend_from_slice(&kind.to_ne_bytes());
        buf.extend_from_slice(data);
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
    }

    fn message(family: u16, cmd: u8, flags: u16, attrs: &[u8]) -> Vec<u8> {
        let mut m = Vec::with_capacity(HDR + attrs.len());
        let len = (HDR + attrs.len()) as u32;
        m.extend_from_slice(&len.to_ne_bytes());
        m.extend_from_slice(&family.to_ne_bytes());
        m.extend_from_slice(&(NLM_F_REQUEST | flags).to_ne_bytes());
        m.extend_from_slice(&1u32.to_ne_bytes()); // sequence
        m.extend_from_slice(&0u32.to_ne_bytes()); // port: the kernel assigns
        m.extend_from_slice(&[cmd, 1, 0, 0]);
        m.extend_from_slice(attrs);
        m
    }

    /// Send one request and return the payloads of the replies (after the
    /// generic netlink header), or the kernel's error.
    fn ask(s: &Sock, msg: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        // SAFETY: an all-zero sockaddr_nl is valid; the fields are set next.
        let mut to: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        to.nl_family = libc::AF_NETLINK as u16;
        // SAFETY: msg and `to` outlive the call; their sizes are passed.
        let sent = unsafe {
            libc::sendto(
                s.0,
                msg.as_ptr() as *const libc::c_void,
                msg.len(),
                0,
                &to as *const _ as *const libc::sockaddr,
                size_of::<libc::sockaddr_nl>() as u32,
            )
        };
        if sent < 0 {
            return Err(format!("netlink: {}", std::io::Error::last_os_error()));
        }
        let mut out = Vec::new();
        let mut buf = vec![0u8; 16384];
        loop {
            // SAFETY: buf is writable for its whole length.
            let n = unsafe { libc::recv(s.0, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
            if n < 0 {
                return Err(format!("netlink: {}", std::io::Error::last_os_error()));
            }
            let mut off = 0usize;
            let n = n as usize;
            let mut done = false;
            while off + 16 <= n {
                let len = u32::from_ne_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
                let kind = u16::from_ne_bytes(buf[off + 4..off + 6].try_into().unwrap());
                if len < 16 || off + len > n {
                    return Err("netlink: a reply was cut short".into());
                }
                if kind == NLMSG_ERROR {
                    let code = i32::from_ne_bytes(buf[off + 16..off + 20].try_into().unwrap());
                    if code != 0 {
                        return Err(format!(
                            "the kernel refused: {}",
                            std::io::Error::from_raw_os_error(-code)
                        ));
                    }
                    done = true;
                } else if len >= HDR {
                    out.push(buf[off + HDR..off + len].to_vec());
                    done = true;
                }
                off += (len + 3) & !3;
            }
            if done {
                return Ok(out);
            }
        }
    }

    /// The attributes in a payload: (kind, data).
    fn attrs(p: &[u8]) -> Vec<(u16, &[u8])> {
        let mut out = Vec::new();
        let mut off = 0;
        while off + 4 <= p.len() {
            let len = u16::from_ne_bytes([p[off], p[off + 1]]) as usize;
            let kind = u16::from_ne_bytes([p[off + 2], p[off + 3]]) & 0x3fff;
            if len < 4 || off + len > p.len() {
                break;
            }
            out.push((kind, &p[off + 4..off + len]));
            off += (len + 3) & !3;
        }
        out
    }

    fn family(s: &Sock) -> Result<u16, String> {
        let mut a = Vec::new();
        attr(&mut a, CTRL_ATTR_FAMILY_NAME, b"nl80211\0");
        let replies = ask(s, &message(GENL_ID_CTRL, CTRL_CMD_GETFAMILY, 0, &a))
            .map_err(|e| format!("no wireless interface in the kernel ({e})"))?;
        replies
            .iter()
            .flat_map(|p| attrs(p))
            .find(|(k, d)| *k == CTRL_ATTR_FAMILY_ID && d.len() >= 2)
            .map(|(_, d)| u16::from_ne_bytes([d[0], d[1]]))
            .ok_or_else(|| "the kernel did not say where nl80211 is".into())
    }

    pub fn get_power_save(ifindex: u32) -> Result<bool, String> {
        let s = open()?;
        let fam = family(&s)?;
        let mut a = Vec::new();
        attr(&mut a, NL80211_ATTR_IFINDEX, &ifindex.to_ne_bytes());
        let replies = ask(&s, &message(fam, NL80211_CMD_GET_POWER_SAVE, 0, &a))?;
        replies
            .iter()
            .flat_map(|p| attrs(p))
            .find(|(k, d)| *k == NL80211_ATTR_PS_STATE && d.len() >= 4)
            .map(|(_, d)| u32::from_ne_bytes([d[0], d[1], d[2], d[3]]) == 1)
            .ok_or_else(|| "the driver did not report power saving".into())
    }

    pub fn set_power_save(ifindex: u32, on: bool) -> Result<(), String> {
        let s = open()?;
        let fam = family(&s)?;
        let mut a = Vec::new();
        attr(&mut a, NL80211_ATTR_IFINDEX, &ifindex.to_ne_bytes());
        attr(&mut a, NL80211_ATTR_PS_STATE, &u32::from(on).to_ne_bytes());
        ask(&s, &message(fam, NL80211_CMD_SET_POWER_SAVE, NLM_F_ACK, &a)).map(|_| ())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn messages_are_laid_out_as_the_kernel_expects() {
            let mut a = Vec::new();
            attr(&mut a, CTRL_ATTR_FAMILY_NAME, b"nl80211\0");
            assert_eq!(a.len(), 12, "4 + 8 bytes, already aligned");
            let m = message(GENL_ID_CTRL, CTRL_CMD_GETFAMILY, 0, &a);
            assert_eq!(m.len(), 32);
            assert_eq!(u32::from_ne_bytes(m[0..4].try_into().unwrap()), 32);
            assert_eq!(u16::from_ne_bytes(m[4..6].try_into().unwrap()), 16);
            assert_eq!(m[16], CTRL_CMD_GETFAMILY);
            let parsed = attrs(&a);
            assert_eq!(parsed, vec![(CTRL_ATTR_FAMILY_NAME, &b"nl80211\0"[..])]);
            let mut b = Vec::new();
            attr(&mut b, NL80211_ATTR_PS_STATE, &1u32.to_ne_bytes());
            assert_eq!(attrs(&b)[0].0, 93);
        }
    }
}

use super::{
    Budget, Endpoint, MAX_CANDIDATES, OwnedListener, ProcessGeneration, boundary, invalidated,
    verify_child,
};
use crate::ProbeFailure;
use libproc::libproc::{
    file_info::PIDFDInfoFlavor,
    net_info::{SocketFDInfo, SocketInfoKind, TcpSIState},
};
use std::{mem::size_of, num::NonZeroU16, path::Path, process::Stdio};
use usage_core::FetchError;

const EXECUTABLE: &str = "/Applications/Antigravity.app/Contents/Resources/bin/language_server";
const MAX_PIDS: usize = 32768;
const MAX_FDS: usize = 4096;
const MAX_ARG_BYTES: usize = 262144;

pub(super) async fn discover(budget: &Budget<'_>) -> Result<Vec<Endpoint>, ProbeFailure> {
    budget.check()?;
    let mut pids = vec![0i32; MAX_PIDS];
    let capacity = size_of_val(pids.as_slice());
    let bytes = unsafe { libc::proc_listpids(1, 0, pids.as_mut_ptr().cast(), capacity as i32) };
    let count = bounded_count(bytes, capacity, size_of::<i32>())?;
    pids.truncate(count);
    pids.sort_unstable();
    let mut endpoints = Vec::new();
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        budget.check()?;
        let Ok(info) = metadata(pid) else {
            continue;
        };
        if !same_user(&info) {
            continue;
        }
        let Ok(path) = executable(pid) else {
            continue;
        };
        if path != Path::new(EXECUTABLE) {
            continue;
        }
        if std::fs::canonicalize(&path).map_err(|_| FetchError::NeedsVerification)? != path {
            return Err(FetchError::NeedsVerification.into());
        }
        let process = generation(pid)?;
        verify_signature(budget).await?;
        let csrf = csrf(pid)?;
        let listeners = listeners(pid, budget)?;
        if listeners.is_empty() {
            return Err(FetchError::TemporarilyUnavailable.into());
        }
        for listener in listeners {
            if endpoints.len() >= MAX_CANDIDATES {
                return Err(FetchError::NeedsVerification.into());
            }
            endpoints.push(Endpoint {
                process: process.clone(),
                listener,
                csrf: csrf.clone(),
            });
        }
    }
    Ok(endpoints)
}

pub(super) async fn revalidate(
    endpoint: &Endpoint,
    budget: &Budget<'_>,
) -> Result<(), ProbeFailure> {
    budget.check()?;
    let current = generation(endpoint.process.pid)?;
    if current != endpoint.process {
        return Err(invalidated(FetchError::NeedsVerification.into()));
    }
    verify_signature(budget).await?;
    budget.check()?;
    if csrf(current.pid)? != endpoint.csrf
        || !listeners(current.pid, budget)?.contains(&endpoint.listener)
        || generation(current.pid)? != endpoint.process
    {
        return Err(invalidated(FetchError::NeedsVerification.into()));
    }
    Ok(())
}

fn metadata(pid: i32) -> Result<libc::proc_bsdinfo, ProbeFailure> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if bytes as usize != size_of::<libc::proc_bsdinfo>() || info.pbi_pid != pid as u32 {
        return Err(os_failure());
    }
    Ok(info)
}
fn same_user(info: &libc::proc_bsdinfo) -> bool {
    info.pbi_uid == unsafe { libc::geteuid() } && info.pbi_ruid == unsafe { libc::getuid() }
}
fn executable(pid: i32) -> Result<std::path::PathBuf, ProbeFailure> {
    use std::os::unix::ffi::OsStrExt;
    let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let bytes = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    if bytes <= 0 || bytes as usize >= path.len() {
        return Err(os_failure());
    }
    let end = path
        .iter()
        .position(|b| *b == 0)
        .ok_or(FetchError::NeedsVerification)?;
    Ok(Path::new(std::ffi::OsStr::from_bytes(&path[..end])).to_owned())
}
fn generation(pid: i32) -> Result<ProcessGeneration, ProbeFailure> {
    let info = metadata(pid)?;
    let executable = executable(pid)?;
    if !same_user(&info) || executable != Path::new(EXECUTABLE) {
        return Err(invalidated(FetchError::NeedsVerification.into()));
    }
    let canonical =
        std::fs::canonicalize(&executable).map_err(|_| FetchError::TemporarilyUnavailable)?;
    if canonical != executable {
        return Err(invalidated(FetchError::NeedsVerification.into()));
    }
    Ok(ProcessGeneration {
        pid,
        uid: info.pbi_uid,
        ruid: info.pbi_ruid,
        start_sec: info.pbi_start_tvsec,
        start_usec: info.pbi_start_tvusec,
        executable,
    })
}
fn csrf(pid: i32) -> Result<String, ProbeFailure> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut bytes = vec![0u8; MAX_ARG_BYTES];
    let mut length = bytes.len();
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || length > bytes.len() {
        return Err(os_failure());
    }
    let result = boundary::csrf_from_procargs(&bytes[..length]).map_err(Into::into);
    bytes.fill(0);
    result
}
fn bounded_count(bytes: i32, capacity: usize, stride: usize) -> Result<usize, ProbeFailure> {
    if bytes < 0 || bytes as usize >= capacity || !(bytes as usize).is_multiple_of(stride) {
        return Err(FetchError::NeedsVerification.into());
    }
    Ok(bytes as usize / stride)
}
fn listeners(pid: i32, budget: &Budget<'_>) -> Result<Vec<OwnedListener>, ProbeFailure> {
    let mut fds: Vec<libc::proc_fdinfo> = (0..MAX_FDS)
        .map(|_| unsafe { std::mem::zeroed() })
        .collect();
    let capacity = size_of_val(fds.as_slice());
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            capacity as i32,
        )
    };
    if bytes <= 0 {
        return Err(os_failure());
    }
    let count = bounded_count(bytes, capacity, size_of::<libc::proc_fdinfo>())?;
    let mut listeners = Vec::new();
    for fd in &fds[..count] {
        budget.check()?;
        if fd.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
            continue;
        }
        let mut info = SocketFDInfo::default();
        let bytes = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd.proc_fd,
                PIDFDInfoFlavor::SocketInfo as i32,
                (&mut info as *mut SocketFDInfo).cast(),
                size_of::<SocketFDInfo>() as i32,
            )
        };
        if bytes <= 0 {
            return Err(os_failure());
        }
        if bytes as usize != size_of::<SocketFDInfo>() {
            return Err(FetchError::NeedsVerification.into());
        }
        if let Some(listener) = listener(fd.proc_fd, &info) {
            if listeners.len() >= MAX_CANDIDATES {
                return Err(FetchError::NeedsVerification.into());
            }
            listeners.push(listener);
        }
    }
    listeners.sort_by_key(|listener| (listener.port, listener.fd));
    Ok(listeners)
}
fn listener(fd: i32, info: &SocketFDInfo) -> Option<OwnedListener> {
    if info.psi.soi_family != libc::AF_INET
        || info.psi.soi_protocol != libc::IPPROTO_TCP
        || info.psi.soi_kind != SocketInfoKind::Tcp as i32
        || info.psi.soi_type != libc::SOCK_STREAM
    {
        return None;
    }
    let tcp = unsafe { info.psi.soi_proto.pri_tcp };
    if tcp.tcpsi_state != TcpSIState::Listen as i32 {
        return None;
    }
    let address = unsafe { tcp.tcpsi_ini.insi_laddr.ina_46.i46a_addr4.s_addr }.to_ne_bytes();
    if address != [127, 0, 0, 1] && address != [0; 4] {
        return None;
    }
    Some(OwnedListener {
        fd,
        port: NonZeroU16::new(u16::from_be(tcp.tcpsi_ini.insi_lport as u16))?,
        address,
        generation: tcp.tcpsi_ini.insi_gencnt,
    })
}
async fn verify_signature(budget: &Budget<'_>) -> Result<(), ProbeFailure> {
    budget.check()?;
    let child = tokio::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "-R=anchor apple generic and identifier \"language_server\" and certificate leaf[subject.OU] = \"EQHXZ8M8AV\"", EXECUTABLE])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .kill_on_drop(true).spawn().map_err(|_| FetchError::NeedsVerification)?;
    verify_child(child, budget).await
}
fn os_failure() -> ProbeFailure {
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM | libc::EACCES) => FetchError::PermissionDenied.into(),
        _ => FetchError::TemporarilyUnavailable.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_owner_requires_both_current_user_ids() {
        let mut info = metadata(std::process::id() as i32).unwrap();
        assert!(same_user(&info));
        info.pbi_uid = info.pbi_uid.wrapping_add(1);
        assert!(!same_user(&info));
        info.pbi_uid = unsafe { libc::geteuid() };
        info.pbi_ruid = info.pbi_ruid.wrapping_add(1);
        assert!(!same_user(&info));
    }
    #[test]
    fn truncated_or_overflowed_native_arrays_fail_closed() {
        assert_eq!(bounded_count(8, 16, 4).unwrap(), 2);
        for bytes in [-1, 7, 16, 20] {
            assert_eq!(
                bounded_count(bytes, 16, 4).unwrap_err().error,
                FetchError::NeedsVerification
            );
        }
    }
    #[test]
    fn native_socket_probe_matches_self_owned_listener() {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let cancellation = crate::Cancellation::default();
        let listeners = listeners(std::process::id() as i32, &Budget::new(&cancellation)).unwrap();
        assert!(listeners.iter().any(|item| item.port.get()
            == socket.local_addr().unwrap().port()
            && item.address == [127, 0, 0, 1]));
    }
    #[test]
    fn rejects_remote_bound_non_listening_and_non_tcp_sockets() {
        let mut info = SocketFDInfo::default();
        info.psi.soi_family = libc::AF_INET;
        info.psi.soi_protocol = libc::IPPROTO_TCP;
        info.psi.soi_type = libc::SOCK_STREAM;
        info.psi.soi_kind = SocketInfoKind::Tcp as i32;
        let mut tcp = libproc::libproc::net_info::TcpSockInfo {
            tcpsi_state: TcpSIState::Listen as i32,
            ..Default::default()
        };
        tcp.tcpsi_ini.insi_lport = 1234u16.to_be() as i32;
        tcp.tcpsi_ini.insi_laddr.ina_46 = libproc::libproc::net_info::In4In6Addr {
            i46a_pad32: [0; 3],
            i46a_addr4: libc::in_addr {
                s_addr: u32::from_ne_bytes([127, 0, 0, 1]),
            },
        };
        info.psi.soi_proto.pri_tcp = tcp;
        assert!(listener(7, &info).is_some());
        tcp.tcpsi_ini.insi_laddr.ina_46.i46a_addr4.s_addr = u32::from_ne_bytes([192, 0, 2, 1]);
        info.psi.soi_proto.pri_tcp = tcp;
        assert!(listener(7, &info).is_none());
        tcp.tcpsi_ini.insi_laddr.ina_46.i46a_addr4.s_addr = 0;
        info.psi.soi_proto.pri_tcp = tcp;
        assert!(listener(7, &info).is_some());
        tcp.tcpsi_state = TcpSIState::Established as i32;
        info.psi.soi_proto.pri_tcp = tcp;
        assert!(listener(7, &info).is_none());
        info.psi.soi_protocol = libc::IPPROTO_UDP;
        assert!(listener(7, &info).is_none());
    }
}

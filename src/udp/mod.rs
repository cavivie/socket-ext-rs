//! This crate provides some extensions and utils for UDP socket, here are examples:
//! * an [extension sync trait] for `std::net::UdpSocket`
//! * an [extension async trait] for `tokio::net::UdpSocket`
//! these support source address selection for outgoing UDP datagrams., which is useful for
//! implementing a UDP server that binds multiple network interfaces.
//!
//! The implementation relies on socket options [`IP_PKTINFO`] \(for IPv4) and [`IPV6_RECVPKTINFO`]
//! \(for IPv6).
//!
//! [extension sync trait]:       trait.UdpSocketExt.html
//! [extension async trait]:      trait.UdpSocketExtAsync.html
//! [`IP_PKTINFO`]:               http://man7.org/linux/man-pages/man7/ip.7.html      
//! [`IPV6_RECVPKTINFO`]:         http://man7.org/linux/man-pages/man7/ipv6.7.html
//!
//!
//! ```
//! use std::net::{UdpSocket, SocketAddr};
//!
//! use socket_ext::UdpSocketExt;
//!
//! fn main() {
//!     demo().unwrap();
//! }
//!
//! fn demo() -> std::io::Result<()> {
//!     // Note: FreeBSD (also OS X, and I believe NetBSD & OpenBSD) will respond
//!     //       to requests sent to configuredaddresses on the loopback interface,
//!     //       just as they would for addresses on any other interface.
//!     //
//!     // If you want to pass the following tests on these family machines,
//!     // must enable the corresponding loopback address first:
//!     // * sudo ifconfig lo0 alias 127.0.0.2 netmask 0xFFFFFFFF
//!     // * sudo ifconfig lo0 alias 127.0.0.3 netmask 0xFFFFFFFF
//!
//!     let mut buf = [0u8; 128];
//!
//!     // Create the server socket and bind it to 0.0.0.0:30012
//!     //
//!     // Note: we will use 127.0.0.23 as source/destination address
//!     //       for our datagrams (to demonstrate the crate features)
//!     //
//!     let srv = UdpSocket::bind_ext("0.0.0.0:3010".parse::<SocketAddr>().unwrap())?;
//!     let srv_addr = "127.0.0.2:3010".parse().unwrap();
//!
//!     // Create the client socket and bind it to an anonymous port
//!     //
//!     // Note: we will use 127.0.0.45 as source/destination address
//!     //       for our datagrams (to demonstrate the crate features)
//!     //
//!     let cli = UdpSocket::bind_ext("0.0.0.0:0".parse::<SocketAddr>().unwrap())?;
//!     let cli_addr = SocketAddr::new(
//!         "127.0.0.3".parse().unwrap(),
//!         cli.local_addr().unwrap().port(),
//!     );
//!     assert_ne!(cli_addr.port(), 0);
//!
//!     // send a request (msg1) from the client to the server
//!     let msg1 = "hello world";
//!     let nb = cli.send_ext(msg1.as_bytes(), Some(&cli_addr.ip()), &srv_addr)?;
//!     assert_eq!(nb, msg1.as_bytes().len());
//!
//!     // receive the request on the server
//!     let (nb, peer, local) = srv.recv_ext(&mut buf)?;
//!
//!     assert_eq!(peer, cli_addr);
//!     assert_eq!(local, Some(srv_addr.ip()));
//!     assert_eq!(nb, msg1.as_bytes().len());
//!     assert_eq!(&buf[0..nb], msg1.as_bytes());
//!
//!     // send a reply (msg2) from the server to the client
//!     let msg2 = "Forty-two";
//!     let nb = srv.send_ext(msg2.as_bytes(), local.as_ref(), &peer)?;
//!     assert_eq!(nb, msg2.as_bytes().len());
//!
//!     // receive the reply on the client
//!     let (nb, peer, local) = cli.recv_ext(&mut buf)?;
//!     assert_eq!(peer, srv_addr);
//!     assert_eq!(local, Some(cli_addr.ip()));
//!     assert_eq!(nb, msg2.as_bytes().len());
//!     assert_eq!(&buf[0..nb], msg2.as_bytes());
//!
//!     Ok(())
//! }
//! ```
mod udp_async;
mod udp_sync;

pub use udp_async::*;
pub use udp_sync::*;

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs},
    os::fd::RawFd,
};

use nix::sys::socket::{
    recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags, SockaddrIn, SockaddrIn6,
    SockaddrLike, SockaddrStorage,
};

const SOCKADDR_IN_LEN: usize = std::mem::size_of::<libc::sockaddr_in>();
const SOCKADDR_IN6_LEN: usize = std::mem::size_of::<libc::sockaddr_in6>();

fn storage_to_sockaddr(s: Option<SockaddrStorage>) -> io::Result<SocketAddr> {
    if let Some(ss) = s {
        if let Some(sin) = ss.as_sockaddr_in() {
            return Ok(SocketAddr::from((Ipv4Addr::from(sin.ip()), sin.port())));
        }
        if let Some(sin6) = ss.as_sockaddr_in6() {
            return Ok(SocketAddr::from((sin6.ip(), sin6.port())));
        }
    };
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid argument",
    ))
}

fn storage_to_ipaddr(s: Option<SockaddrStorage>) -> Option<IpAddr> {
    if let Some(ss) = s {
        if let Some(sin) = ss.as_sockaddr_in() {
            return Some(Ipv4Addr::from(sin.ip()).into());
        }
        if let Some(sin6) = ss.as_sockaddr_in6() {
            return Some(sin6.ip().into());
        }
    }
    None
}

/// Receive a datagram (low-level function).
///
/// Parameters
///
/// * `socket`: recvieve data on the socket
/// * `buf`: buffer for storing the payload
///
/// Returns a tuple containing:
///
///   * the size of the payload
///   * the source socket address (peer)
///   * the destination ip address (local), may not be present
/// if the underlying socket does not provide them.
pub fn udp_recv(socket: RawFd, buf: &mut [u8]) -> io::Result<(usize, SocketAddr, Option<IpAddr>)> {
    let mut iov = [io::IoSliceMut::new(buf)];
    let mut space = nix::cmsg_space!(libc::in_pktinfo, libc::in6_pktinfo);
    let flags = MsgFlags::empty();

    let msg = recvmsg::<SockaddrStorage>(socket, &mut iov, Some(&mut space), flags)?;
    let peer = storage_to_sockaddr(msg.address)?;

    let cmsgs = msg.cmsgs();
    let ss: SockaddrStorage = unsafe { std::mem::zeroed() };
    for ctl_msg in cmsgs {
        match ctl_msg {
            ControlMessageOwned::Ipv4PacketInfo(pktinfo) => unsafe {
                let sa = ss.as_ptr() as *mut libc::sockaddr_in;
                (*sa).sin_family = libc::AF_INET as libc::sa_family_t;
                (*sa).sin_addr = pktinfo.ipi_addr;
                #[cfg(any(
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "ios",
                    target_os = "macos",
                    target_os = "netbsd",
                    target_os = "openbsd"
                ))]
                {
                    (*sa).sin_len = SOCKADDR_IN_LEN as u8;
                }
            },
            ControlMessageOwned::Ipv6PacketInfo(pktinfo) => unsafe {
                let sa = ss.as_ptr() as *mut libc::sockaddr_in6;
                (*sa).sin6_family = libc::AF_INET6 as libc::sa_family_t;
                (*sa).sin6_addr = pktinfo.ipi6_addr;
                #[cfg(any(
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "ios",
                    target_os = "macos",
                    target_os = "netbsd",
                    target_os = "openbsd"
                ))]
                {
                    (*sa).sin6_len = SOCKADDR_IN6_LEN as u8;
                }
            },
            _ => continue,
        }
    }
    let local = storage_to_ipaddr(Some(ss));

    Ok((msg.bytes, peer, local))
}

/// Send datagram (low-level function).
///
/// Parameters
///
/// * `socket`: send data to the socket
/// * `buf`: buffer for sending the payload
/// * `src`: address for the routing table lookup,
/// and for setting up IP source route options.
/// * `dst`: address for sending to the target,
/// and it's unused when the socket is connected.
///
/// Return the size of the sent payload.
pub fn udp_send<S>(socket: RawFd, buf: &[u8], src: Option<&IpAddr>, dst: S) -> io::Result<usize>
where
    S: ToSocketAddrs,
{
    let iov = [io::IoSlice::new(buf)];

    let nb = match dst.to_socket_addrs()?.next() {
        Some(dst) => match dst {
            SocketAddr::V4(dst) => {
                let src_addr = match src {
                    Some(IpAddr::V4(ip)) => {
                        SockaddrIn::from(SocketAddrV4::new(*ip, 0))
                            .as_ref()
                            .sin_addr
                    }
                    Some(IpAddr::V6(_)) => panic!(""),
                    None => libc::in_addr { s_addr: 0 },
                };

                let dst_addr = SockaddrIn::from(dst);
                #[cfg(target_os = "netbsd")]
                let pi = libc::in_pktinfo {
                    ipi_ifindex: 0, /* Unspecified interface */
                    ipi_addr: libc::in_addr { s_addr: 0 },
                };
                #[cfg(not(target_os = "netbsd"))]
                let pi = libc::in_pktinfo {
                    ipi_ifindex: 0, /* Unspecified interface */
                    ipi_addr: libc::in_addr { s_addr: 0 },
                    ipi_spec_dst: src_addr,
                };
                let cmsgs = [ControlMessage::Ipv4PacketInfo(&pi)];
                sendmsg(socket, &iov, &cmsgs, MsgFlags::empty(), Some(&dst_addr))?
            }
            SocketAddr::V6(dst) => {
                let src_addr = match src {
                    Some(IpAddr::V6(ip)) => {
                        SockaddrIn6::from(SocketAddrV6::new(*ip, 0, 0, 0))
                            .as_ref()
                            .sin6_addr
                    }
                    Some(IpAddr::V4(_)) => panic!(""),
                    None => libc::in6_addr { s6_addr: [0u8; 16] },
                };
                let dst_addr = SockaddrIn6::from(dst);
                let pi = libc::in6_pktinfo {
                    ipi6_ifindex: 0, /* Unspecified interface */
                    ipi6_addr: src_addr,
                };
                let cmsgs = [ControlMessage::Ipv6PacketInfo(&pi)];
                sendmsg(socket, &iov, &cmsgs, MsgFlags::empty(), Some(&dst_addr))?
            }
        },
        None => sendmsg::<()>(socket, &iov, &[], MsgFlags::empty(), None)?,
    };

    Ok(nb)
}

#[cfg(test)]
mod test {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
        os::fd::AsRawFd,
    };

    use crate::utils::set_pktinfo;

    use super::{udp_recv, udp_send};

    #[test]
    fn test_cmsg_send_recv() {
        let cli = UdpSocket::bind("0.0.0.0:3010").unwrap();
        let cli_fd = cli.as_raw_fd();
        let cli_msg = "hello world";
        let cli_src = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let cli_dst = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4000));

        let srv = std::net::UdpSocket::bind("0.0.0.0:4010").unwrap();
        let srv_fd = srv.as_raw_fd();
        let mut srv_buf = [0u8; 128];
        set_pktinfo(srv.as_raw_fd()).unwrap();

        let snb = udp_send(cli_fd, cli_msg.as_bytes(), Some(&cli_src), &cli_dst).unwrap();
        assert_eq!(snb, cli_msg.as_bytes().len());

        let (rnb, peer, local) = udp_recv(srv_fd, &mut srv_buf).unwrap();
        assert_eq!(rnb, snb);
        assert_eq!(cli_msg.as_bytes().len(), rnb);
        assert_eq!(cli_msg.as_bytes(), &srv_buf[..rnb]);
        println!(
            "bytes {:?}, peer {:?}, local {:?}, data {:?}",
            rnb,
            peer,
            local,
            &srv_buf[..rnb]
        );

        let snb = udp_send(srv_fd, &srv_buf[..rnb], local.as_ref(), &peer).unwrap();
        assert_eq!(rnb, snb)
    }
}

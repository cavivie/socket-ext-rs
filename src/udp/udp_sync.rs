use std::{
    io,
    net::{IpAddr, SocketAddr},
    os::fd::AsRawFd,
};

use crate::set_pktinfo;

/// An extension trait to support source address selection in `std::net::UdpSocket`
///
/// See [module level][mod] documentation for more details.
///
/// [mod]: index.html
///
pub trait UdpSocketExt: Sized {
    /// Creates a UDP socket from the given address.
    ///
    /// The address type can be any implementor of [`ToSocketAddrs`] trait. See
    /// its documentation for concrete examples.
    ///
    /// [`ToSocketAddrs`]: https://doc.rust-lang.org/std/net/trait.ToSocketAddrs.html
    ///
    /// The new socket is configured with the `IP_PKTINFO` or `IPV6_RECVPKTINFO` option enabled.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::net::UdpSocket;
    /// use socket_ext::UdpSocketExt;
    ///
    /// let socket = UdpSocket::bind_ext("127.0.0.1:34254").expect("couldn't bind to address");
    /// ```
    fn bind_ext<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<Self>;

    /// Sends a datagram to the given `target` address and use the `local` address as its
    /// source.
    ///
    /// On success, returns the number of bytes written.
    fn send_ext(
        &self,
        buf: &[u8],
        local: Option<&IpAddr>,
        target: &SocketAddr,
    ) -> io::Result<usize>;

    /// Receive a datagram
    ///
    /// On success, returns a tuple `(nb, source, local)` containing the number of bytes read, the
    /// source socket address (peer address), and the destination ip address (local address).
    ///
    fn recv_ext(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr, Option<IpAddr>)>;
}

impl UdpSocketExt for std::net::UdpSocket {
    fn bind_ext<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<std::net::UdpSocket> {
        let sock = std::net::UdpSocket::bind(addr)?;
        set_pktinfo(sock.as_raw_fd())?;
        Ok(sock)
    }

    fn send_ext(
        &self,
        buf: &[u8],
        local: Option<&IpAddr>,
        target: &SocketAddr,
    ) -> io::Result<usize> {
        super::udp_send(self.as_raw_fd(), buf, local, target)
    }

    fn recv_ext(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr, Option<IpAddr>)> {
        super::udp_recv(self.as_raw_fd(), buf)
    }
}

#[cfg(test)]
mod test {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

    use super::UdpSocketExt;

    #[test]
    fn test_socket_send_recv() {
        let cli = UdpSocket::bind_ext("0.0.0.0:3010").unwrap();
        let cli_src = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let cli_dst = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4000));
        let cli_msg = "hello world";

        let srv = std::net::UdpSocket::bind_ext("0.0.0.0:4010").unwrap();
        let mut srv_buf = [0u8; 44];

        let snb = cli
            .send_ext(cli_msg.as_bytes(), Some(&cli_src), &cli_dst)
            .unwrap();
        assert_eq!(snb, cli_msg.as_bytes().len());

        let (rnb, peer, local) = srv.recv_ext(&mut srv_buf).unwrap();
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

        let snb = srv
            .send_ext(&srv_buf[..rnb], local.as_ref(), &peer)
            .unwrap();
        assert_eq!(rnb, snb)
    }
}

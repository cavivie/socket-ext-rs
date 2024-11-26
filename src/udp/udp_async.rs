use std::{
    io,
    net::{IpAddr, SocketAddr},
    os::fd::AsRawFd,
};

use crate::utils::{set_pktinfo, BoxFuture};

/// An extension trait to support source address selection in `tokio::net::UdpSocket`
///
/// See [module level][mod] documentation for more details.
///
/// [mod]: index.html
///
pub trait UdpSocketExtAsync<'a>: Sized {
    /// Creates a UDP socket from the given address.
    ///
    /// The address type can be any implementor of [`ToSocketAddrs`] trait. See
    /// its documentation for concrete examples.
    ///
    /// [`ToSocketAddrs`]: https://docs.rs/tokio/latest/tokio/net/trait.ToSocketAddrs.html
    ///
    /// The new socket is configured with the `IP_PKTINFO` or `IPV6_RECVPKTINFO` option enabled.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tokio::net::UdpSocket;
    /// use socket_ext::UdpSocketExtAsync;
    ///
    /// let socket = UdpSocket::bind_ext("127.0.0.1:34254").await.expect("couldn't bind to address");
    /// ```
    fn bind_ext<A: tokio::net::ToSocketAddrs + 'a>(addr: A) -> BoxFuture<'a, io::Result<Self>>;

    /// Sends a datagram to the given `target` address and use the `local` address as its
    /// source.
    ///
    /// On success, returns the number of bytes written.
    fn send_ext(
        &'a self,
        buf: &'a [u8],
        src: Option<&'a IpAddr>,
        dst: &'a SocketAddr,
    ) -> BoxFuture<'a, io::Result<usize>>;

    /// Receive a datagram
    ///
    /// On success, returns a tuple `(nb, source, local)` containing the number of bytes read, the
    /// source socket address (peer address), and the destination ip address (local address).
    ///
    fn recv_ext(
        &'a self,
        buf: &'a mut [u8],
    ) -> BoxFuture<'a, io::Result<(usize, SocketAddr, Option<IpAddr>)>>;
}

impl<'a> UdpSocketExtAsync<'a> for tokio::net::UdpSocket {
    fn bind_ext<A: tokio::net::ToSocketAddrs + 'a>(addr: A) -> BoxFuture<'a, io::Result<Self>> {
        BoxFuture::new(async move {
            let sock = tokio::net::UdpSocket::bind(addr).await?;
            set_pktinfo(sock.as_raw_fd())?;
            Ok(sock)
        })
    }

    fn send_ext(
        &'a self,
        buf: &'a [u8],
        src: Option<&'a IpAddr>,
        dst: &'a SocketAddr,
    ) -> BoxFuture<'a, io::Result<usize>> {
        BoxFuture::new(async move {
            loop {
                self.writable().await?;
                match self.try_io(tokio::io::Interest::WRITABLE, || {
                    super::udp_send(self.as_raw_fd(), buf, src, dst)
                }) {
                    Ok(ret) => return Ok(ret),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e),
                }
            }
        })
    }

    fn recv_ext(
        &'a self,
        buf: &'a mut [u8],
    ) -> BoxFuture<'a, io::Result<(usize, SocketAddr, Option<IpAddr>)>> {
        BoxFuture::new(async move {
            loop {
                self.readable().await?;
                match self.try_io(tokio::io::Interest::READABLE, || {
                    super::udp_recv(self.as_raw_fd(), buf)
                }) {
                    Ok(ret) => return Ok(ret),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e),
                }
            }
        })
    }
}

#[cfg(test)]
mod test {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

    use tokio::net::UdpSocket;

    use super::UdpSocketExtAsync;

    #[tokio::test]
    async fn test_socket_send_recv() {
        let local_set = tokio::task::LocalSet::new();

        local_set.spawn_local(async move {
            let srv = UdpSocket::bind_ext("0.0.0.0:4010").await.unwrap();
            let mut srv_buf = [0u8; 44];
            let expect_msg = "hello world";

            let (rnb, peer, local) = srv.recv_ext(&mut srv_buf).await.unwrap();
            assert_eq!(expect_msg.as_bytes().len(), rnb);
            assert_eq!(expect_msg.as_bytes(), &srv_buf[..rnb]);
            println!(
                "bytes {:?}, peer {:?}, local {:?}, data {:?}",
                rnb,
                peer,
                local,
                &srv_buf[..rnb]
            );

            let snb = srv
                .send_ext(&srv_buf[..rnb], local.as_ref(), &peer)
                .await
                .unwrap();
            assert_eq!(rnb, snb)
        });

        local_set.spawn_local(async move {
            let cli = UdpSocket::bind_ext("0.0.0.0:3010").await.unwrap();
            let cli_src = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
            let cli_dst = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4010));
            let cli_msg = "hello world";

            let snb = cli
                .send_ext(cli_msg.as_bytes(), Some(&cli_src), &cli_dst)
                .await
                .unwrap();
            assert_eq!(snb, cli_msg.as_bytes().len());
        });

        local_set.await;
    }
}

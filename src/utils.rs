use std::{
    future::{Future, IntoFuture},
    io,
    os::fd::RawFd,
    pin::Pin,
    task::{Context, Poll},
};

/// BoxFuture acts the same as the [BoxFuture in crate futures utils],
/// which is an owned dynamically typed Future for use in cases where you
/// can’t statically type your result or need to add some indirection.
/// But the difference of this structure is that it will conditionally
/// implement Send according to the properties of type T, which does not
/// require two sets of API interfaces in single-threaded and multi-threaded.
/// 
/// [BoxFuture in crate futures utils]: https://docs.rs/futures-util/latest/futures_util/future/type.BoxFuture.html 
pub struct BoxFuture<'a, T>(Pin<Box<dyn Future<Output = T> + 'a>>);

impl<'a, T> BoxFuture<'_, T> {
    pub fn new<F>(f: F) -> BoxFuture<'a, T>
    where
        F: IntoFuture<Output = T> + 'a,
    {
        BoxFuture(Box::pin(f.into_future()))
    }

    pub fn wrap(f: Pin<Box<dyn Future<Output = T> + 'a>>) -> BoxFuture<'_, T> {
        BoxFuture(f)
    }
}

unsafe impl<T: Send> Send for BoxFuture<'_, T> {}

impl<T> Future for BoxFuture<'_, T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(context)
    }
}

macro_rules! try_io {
    ($x:expr) => {
        match $x {
            -1 => {
                return Err(io::Error::last_os_error());
            }
            x => x,
        }
    };
}

#[allow(unused)]
pub fn get_sockopt<T>(
    socket: RawFd,
    level: libc::c_int,
    name: libc::c_int,
    value: &mut T,
) -> io::Result<libc::socklen_t> {
    unsafe {
        let mut len = std::mem::size_of::<T>() as libc::socklen_t;
        try_io!(libc::getsockopt(
            socket,
            level,
            name,
            value as *mut T as *mut libc::c_void,
            &mut len
        ));
        Ok(len)
    }
}

pub fn set_sockopt<T>(
    socket: RawFd,
    level: libc::c_int,
    name: libc::c_int,
    value: &T,
) -> io::Result<()> {
    unsafe {
        try_io!(libc::setsockopt(
            socket,
            level,
            name,
            value as *const T as *const libc::c_void,
            std::mem::size_of::<T>() as libc::socklen_t
        ));
        Ok(())
    }
}

/// get inet address family info on a raw socket fd
pub fn get_sockinet(socket: RawFd) -> io::Result<i32> {
    let mut domain = libc::c_int::default();
    get_sockopt(socket, libc::SOL_SOCKET, libc::SO_DOMAIN, &mut domain)?;
    Ok(domain)
}

/// enable IP_PKTINFO/IPV6_RECVPKTINFO on a socket
pub fn set_pktinfo(socket: RawFd) -> io::Result<()> {
    let (level, option) = match get_sockinet(socket)? {
        libc::AF_INET => (libc::IPPROTO_IP, libc::IP_PKTINFO),
        libc::AF_INET6 => (libc::IPPROTO_IPV6, libc::IPV6_RECVPKTINFO),
        _ => {
            return Err(io::Error::new(io::ErrorKind::Other, "not an inet socket"));
        }
    };
    set_sockopt(socket, level, option, &(1 as libc::c_int))
}

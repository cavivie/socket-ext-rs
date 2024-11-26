//! This crate provides some extensions and utils for socket operations.
//! * an [extension udp sync trait] for `std::net::UdpSocket`
//! * an [extension udp async trait] for `tokio::net::UdpSocket`
//! * fn [extension packet info fn] to enable IP_PKTINFO/IPV6_RECVPKTINFO.
//!
//! [extension udp sync trait]:   udp/trait.UdpSocketExt.html
//! [extension udp async trait]:  udp/trait.UdpSocketExtAsync.html
//! [extension packet info fn]:   utils/fn.set_pktinfo.html
pub mod udp;
pub mod utils;

pub use udp::*;
pub use utils::*;

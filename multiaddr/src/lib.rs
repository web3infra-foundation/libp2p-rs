///! Implementation of [multiaddr](https://github.com/jbenet/multiaddr) in Rust.
pub use multihash;

mod errors;
mod onion_addr;
pub mod protocol;

#[cfg(feature = "url")]
mod from_url;

pub use self::errors::{Error, Result};
pub use self::onion_addr::Onion3Addr;
pub use self::protocol::Protocol;
use serde::{
    de::{self, Error as DeserializerError},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{
    convert::TryFrom,
    fmt, io,
    iter::FromIterator,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    result::Result as StdResult,
    str::FromStr,
    sync::Arc,
};

#[cfg(feature = "url")]
pub use self::from_url::{from_url, from_url_lossy, FromUrlErr};
static_assertions::const_assert! {
    // This check is most certainly overkill right now, but done here
    // anyway to ensure the `as u64` casts in this crate are safe.
    std::mem::size_of::<usize>() <= std::mem::size_of::<u64>()
}

/// Representation of a Multiaddr.
#[allow(clippy::rc_buffer)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub struct Multiaddr {
    bytes: Arc<Vec<u8>>,
}

impl Multiaddr {
    /// Create a new, empty multiaddress.
    pub fn empty() -> Self {
        Self {
            bytes: Arc::new(Vec::new()),
        }
    }

    /// Create a new, empty multiaddress with the given capacity.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            bytes: Arc::new(Vec::with_capacity(n)),
        }
    }

    /// Return the length in bytes of this multiaddress.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns true if the length of this multiaddress is 0.
    pub fn is_empty(&self) -> bool {
        self.bytes.len() == 0
    }

    /// Return a copy of this [`Multiaddr`]'s byte representation.
    pub fn to_vec(&self) -> Vec<u8> {
        Vec::from(&self.bytes[..])
    }

    /// Adds an already-parsed address component to the end of this multiaddr.
    ///
    /// # Examples
    ///
    /// ```
    /// use libp2prs_multiaddr::{Multiaddr, protocol::Protocol};
    ///
    /// let mut address: Multiaddr = "/ip4/127.0.0.1".parse().unwrap();
    /// address.push(Protocol::Tcp(10000));
    /// assert_eq!(address, "/ip4/127.0.0.1/tcp/10000".parse().unwrap());
    /// ```
    ///
    pub fn push(&mut self, p: Protocol<'_>) {
        let mut w = io::Cursor::<&mut Vec<u8>>::new(Arc::make_mut(&mut self.bytes));
        w.set_position(w.get_ref().len() as u64);
        p.write_bytes(&mut w).expect("Writing to a `io::Cursor<&mut Vec<u8>>` never fails.")
    }

    /// Pops the last `Protocol` of this multiaddr, or `None` if the multiaddr is empty.
    /// ```
    /// use libp2prs_multiaddr::{Multiaddr, protocol::Protocol};
    ///
    /// let mut address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    ///
    /// assert_eq!(address.pop().unwrap(), Protocol::Sctp(5678));
    /// assert_eq!(address.pop().unwrap(), Protocol::Udt);
    /// ```
    ///
    pub fn pop<'a>(&mut self) -> Option<Protocol<'a>> {
        let mut slice = &self.bytes[..]; // the remaining multiaddr slice
        if slice.is_empty() {
            return None;
        }
        let protocol = loop {
            let (p, s) = Protocol::from_bytes(slice).expect("`slice` is a valid `Protocol`.");
            if s.is_empty() {
                break p.acquire();
            }
            slice = s
        };
        let remaining_len = self.bytes.len() - slice.len();
        Arc::make_mut(&mut self.bytes).truncate(remaining_len);
        Some(protocol)
    }

    pub fn value_for_protocol(&self, code: u32) -> Option<String> {
        let components = self.iter().collect::<Vec<_>>();
        for comp in components.iter() {
            if comp.get_key().is_ok() && comp.get_key().unwrap() == code {
                return Some(comp.to_string());
            }
        }
        None
    }

    pub fn is_loopback_addr(&self) -> bool {
        let mut is_loopback = false;
        let components = self.iter().collect::<Vec<_>>();
        for comp in components.iter() {
            match comp {
                protocol::Protocol::Ip4(ipv4_addr) => {
                    is_loopback = ipv4_addr.is_loopback();
                }
                protocol::Protocol::Ip6(ipv6_addr) => {
                    is_loopback = ipv6_addr.is_loopback();
                }
                _ => {}
            }
        }
        is_loopback
    }

    pub fn is_private_addr(&self) -> bool {
        let mut is_private = false;
        let components = self.iter().collect::<Vec<_>>();
        for comp in components.iter() {
            match comp {
                protocol::Protocol::Ip4(ipv4_addr) => {
                    is_private = ipv4_addr.is_private() || ipv4_addr.is_loopback() || ipv4_addr.is_link_local();
                }
                protocol::Protocol::Ip6(ipv6_addr) => {
                    is_private = ipv6_addr.is_loopback();
                    //TODO: unstable feature, ipv6_addr.is_unique_local()||ipv6_addr.is_unicast_link_local();
                }
                _ => {}
            }
        }
        is_private
    }

    pub fn is_ipv6_addr(&self) -> bool {
        let mut is_ipv6 = false;
        let components = self.iter().collect::<Vec<_>>();
        for comp in components.iter() {
            if let protocol::Protocol::Ip6(_ipv6_addr) = comp {
                // protocol::Protocol::Ip6(_ipv6_addr) => {
                is_ipv6 = true;
                //TODO: unstable feature, ipv6_addr.is_unique_local()||ipv6_addr.is_unicast_link_local();
                // }
                // _ => {}
            }
        }
        is_ipv6
    }

    /// we don't consume FD's for relay addresses for now as they will be consumed when the Relay Transport actually dials the Relay server.
    /// That dial call will also pass through this limiter with the address of the relay server i.e. non-relay address.
    pub fn should_consume_fd(&self) -> bool {
        let mut find = false;
        for p in self.iter() {
            if let Ok(key) = p.get_key() {
                if key == protocol::UNIX || key == protocol::TCP {
                    find = true;
                }
            }
        }
        find
    }

    /// Like [`Multiaddr::push`] but consumes `self`.
    pub fn with(mut self, p: Protocol<'_>) -> Self {
        let mut w = io::Cursor::<&mut Vec<u8>>::new(Arc::make_mut(&mut self.bytes));
        w.set_position(w.get_ref().len() as u64);
        p.write_bytes(&mut w).expect("Writing to a `io::Cursor<&mut Vec<u8>>` never fails.");
        self
    }

    /// Returns the components of this multiaddress.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::net::Ipv4Addr;
    /// use libp2prs_multiaddr::{Multiaddr, protocol::Protocol};
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    ///
    /// let components = address.iter().collect::<Vec<_>>();
    /// assert_eq!(components[0], Protocol::Ip4(Ipv4Addr::new(127, 0, 0, 1)));
    /// assert_eq!(components[1], Protocol::Udt);
    /// assert_eq!(components[2], Protocol::Sctp(5678));
    /// ```
    ///
    pub fn iter(&self) -> Iter<'_> {
        Iter(&self.bytes)
    }

    /// Replace a [`Protocol`] at some position in this `Multiaddr`.
    ///
    /// The parameter `at` denotes the index of the protocol at which the function
    /// `by` will be applied to the current protocol, returning an optional replacement.
    ///
    /// If `at` is out of bounds or `by` does not yield a replacement value,
    /// `None` will be returned. Otherwise a copy of this `Multiaddr` with the
    /// updated `Protocol` at position `at` will be returned.
    pub fn replace<'a, F>(&self, at: usize, by: F) -> Option<Multiaddr>
    where
        F: FnOnce(&Protocol) -> Option<Protocol<'a>>,
    {
        let mut address = Multiaddr::with_capacity(self.len());
        let mut fun = Some(by);
        let mut replaced = false;

        for (i, p) in self.iter().enumerate() {
            if i == at {
                let f = fun.take().expect("i == at only happens once");
                if let Some(q) = f(&p) {
                    address = address.with(q);
                    replaced = true;
                    continue;
                }
                return None;
            }
            address = address.with(p)
        }

        if replaced {
            Some(address)
        } else {
            None
        }
    }
}

impl fmt::Debug for Multiaddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_string().fmt(f)
    }
}

impl fmt::Display for Multiaddr {
    /// Convert a Multiaddr to a string
    ///
    /// # Example
    ///
    /// ```
    /// use libp2prs_multiaddr::Multiaddr;
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1/udt".parse().unwrap();
    /// assert_eq!(address.to_string(), "/ip4/127.0.0.1/udt");
    /// ```
    ///
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for s in self.iter() {
            s.to_string().fmt(f)?;
        }
        Ok(())
    }
}

impl AsRef<[u8]> for Multiaddr {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl<'a> IntoIterator for &'a Multiaddr {
    type Item = Protocol<'a>;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        Iter(&self.bytes)
    }
}

impl<'a> FromIterator<Protocol<'a>> for Multiaddr {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Protocol<'a>>,
    {
        let mut writer = Vec::new();
        for cmp in iter {
            cmp.write_bytes(&mut writer).expect("Writing to a `Vec` never fails.");
        }
        Multiaddr { bytes: Arc::new(writer) }
    }
}

impl FromStr for Multiaddr {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self> {
        let mut writer = Vec::new();
        let mut parts = input.split('/').peekable();

        if Some("") != parts.next() {
            // A multiaddr must start with `/`
            return Err(Error::InvalidMultiaddr);
        }

        while parts.peek().is_some() {
            let p = Protocol::from_str_parts(&mut parts)?;
            p.write_bytes(&mut writer).expect("Writing to a `Vec` never fails.");
        }

        Ok(Multiaddr { bytes: Arc::new(writer) })
    }
}

/// Iterator over `Multiaddr` [`Protocol`]s.
pub struct Iter<'a>(&'a [u8]);

impl<'a> Iterator for Iter<'a> {
    type Item = Protocol<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }

        let (p, next_data) = Protocol::from_bytes(self.0).expect("`Multiaddr` is known to be valid.");

        self.0 = next_data;
        Some(p)
    }
}

impl<'a> From<Protocol<'a>> for Multiaddr {
    fn from(p: Protocol<'a>) -> Multiaddr {
        let mut w = Vec::new();
        p.write_bytes(&mut w).expect("Writing to a `Vec` never fails.");
        Multiaddr { bytes: Arc::new(w) }
    }
}

impl From<IpAddr> for Multiaddr {
    fn from(v: IpAddr) -> Multiaddr {
        match v {
            IpAddr::V4(a) => a.into(),
            IpAddr::V6(a) => a.into(),
        }
    }
}

impl From<Ipv4Addr> for Multiaddr {
    fn from(v: Ipv4Addr) -> Multiaddr {
        Protocol::Ip4(v).into()
    }
}

impl From<Ipv6Addr> for Multiaddr {
    fn from(v: Ipv6Addr) -> Multiaddr {
        Protocol::Ip6(v).into()
    }
}

impl TryFrom<Vec<u8>> for Multiaddr {
    type Error = Error;

    fn try_from(v: Vec<u8>) -> Result<Self> {
        // Check if the argument is a valid `Multiaddr` by reading its protocols.
        let mut slice = &v[..];
        while !slice.is_empty() {
            let (_, s) = Protocol::from_bytes(slice)?;
            slice = s
        }
        Ok(Multiaddr { bytes: Arc::new(v) })
    }
}

impl TryFrom<String> for Multiaddr {
    type Error = Error;

    fn try_from(s: String) -> Result<Multiaddr> {
        s.parse()
    }
}

impl<'a> TryFrom<&'a str> for Multiaddr {
    type Error = Error;

    fn try_from(s: &'a str) -> Result<Multiaddr> {
        s.parse()
    }
}

impl Serialize for Multiaddr {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            serializer.serialize_bytes(self.as_ref())
        }
    }
}

impl<'de> Deserialize<'de> for Multiaddr {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor {
            is_human_readable: bool,
        }

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Multiaddr;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("multiaddress")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> StdResult<Self::Value, A::Error> {
                let mut buf: Vec<u8> = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(e) = seq.next_element()? {
                    buf.push(e);
                }
                if self.is_human_readable {
                    let s = String::from_utf8(buf).map_err(DeserializerError::custom)?;
                    s.parse().map_err(DeserializerError::custom)
                } else {
                    Multiaddr::try_from(buf).map_err(DeserializerError::custom)
                }
            }
            fn visit_str<E: de::Error>(self, v: &str) -> StdResult<Self::Value, E> {
                v.parse().map_err(DeserializerError::custom)
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> StdResult<Self::Value, E> {
                self.visit_str(v)
            }
            fn visit_string<E: de::Error>(self, v: String) -> StdResult<Self::Value, E> {
                self.visit_str(&v)
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> StdResult<Self::Value, E> {
                self.visit_byte_buf(v.into())
            }
            fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> StdResult<Self::Value, E> {
                self.visit_byte_buf(v.into())
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> StdResult<Self::Value, E> {
                Multiaddr::try_from(v).map_err(DeserializerError::custom)
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(Visitor { is_human_readable: true })
        } else {
            deserializer.deserialize_bytes(Visitor { is_human_readable: false })
        }
    }
}

/// Easy way for a user to create a `Multiaddr`.
///
/// Example:
///
/// ```rust
/// # use libp2prs_multiaddr::multiaddr;
/// let addr = multiaddr!(Ip4([127, 0, 0, 1]), Tcp(10500u16));
/// ```
///
/// Each element passed to `multiaddr!` should be a variant of the `Protocol` enum. The
/// optional parameter is turned into the proper type with the `Into` trait.
///
/// For example, `Ip4([127, 0, 0, 1])` works because `Ipv4Addr` implements `From<[u8; 4]>`.
#[macro_export]
macro_rules! multiaddr {
    ($($comp:ident $(($param:expr))*),+) => {
        {
            use std::iter;
            let elem = iter::empty::<$crate::protocol::Protocol>();
            $(
                let elem = {
                    let cmp = $crate::protocol::Protocol::$comp $(( $param.into() ))*;
                    elem.chain(iter::once(cmp))
                };
            )+
            elem.collect::<$crate::Multiaddr>()
        }
    }
}

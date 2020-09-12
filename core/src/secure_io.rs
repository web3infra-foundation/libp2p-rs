
use multiaddr::Multiaddr;
use libp2p_traits::{Read2, Write2};
use crate::transport::TransportError;
use crate::{PeerId, PublicKey};
use crate::identity::Keypair;


pub trait SecureIo: Read2 + Write2 + Unpin {
    fn local_peer(&self) -> PeerId;

    fn remote_peer(&self) -> PeerId;

    fn local_priv_key(&self) -> Keypair;

    fn remote_pub_key(&self) -> PublicKey;
}
use crate::identity::Keypair;
use crate::{PeerId, PublicKey};

pub trait SecureInfo {
    fn local_peer(&self) -> PeerId;

    fn remote_peer(&self) -> PeerId;

    fn local_priv_key(&self) -> Keypair;

    fn remote_pub_key(&self) -> PublicKey;
}

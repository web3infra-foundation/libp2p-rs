use crate::codec::len_prefix::LengthPrefixSocket;
use crate::error::PlaintextError;
use crate::secure_stream::SecureStream;
use crate::structs_proto::Exchange;
use crate::PlainTextConfig;
use libp2p_core::{PeerId, PublicKey};
use libp2p_traits::{ReadEx, WriteEx};
use log::error;
use prost::Message;
use std::io;

struct HandshakeContext<T> {
    config: PlainTextConfig,
    state: T,
}

pub struct Local {
    exchange_bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct Remote {
    pub peer_id: PeerId,
    pub public_key: PublicKey,
}

pub(crate) async fn handshake<T>(
    socket: T,
    config: PlainTextConfig,
) -> Result<(SecureStream<T>, Remote), PlaintextError>
where
    T: ReadEx + WriteEx + Send + 'static,
{
    let mut socket = LengthPrefixSocket::new(socket, config.clone().max_frame_length);
    let local_context = HandshakeContext::new(config.clone())?;
    socket
        .send_frame(local_context.state.exchange_bytes.as_ref())
        .await?;

    let buf = socket.recv_frame().await?;
    let remote_context = local_context.with_remote(buf)?;

    let local_id = config.clone().key.public().into_peer_id();

    let remote_state = remote_context.state;

    // info!("Remote ID: {:?}", remote_state.clone().peer_id);
    //
    // info!("Local ID: {:?}", local_id.clone());

    if remote_state.clone().public_key.into_peer_id() == local_id {
        return Err(PlaintextError::ConnectSelf);
    }

    let secure_stream = SecureStream::new(socket);

    Ok((secure_stream, remote_state))
}

impl HandshakeContext<Local> {
    pub fn new(config: PlainTextConfig) -> io::Result<HandshakeContext<Local>> {
        let public_key = config.key.public();
        let local_id = public_key.clone().into_peer_id();
        let local = Exchange {
            id: Some(local_id.into_bytes()),
            pubkey: Some(public_key.into_protobuf_encoding()),
        };
        let mut buf = Vec::with_capacity(local.encoded_len());
        local
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");
        Ok(HandshakeContext {
            config,
            state: Local {
                exchange_bytes: buf,
            },
        })
    }

    pub fn with_remote(
        self,
        exchange_bytes: Vec<u8>,
    ) -> Result<HandshakeContext<Remote>, PlaintextError> {
        let prop = match Exchange::decode(&exchange_bytes[..]) {
            Ok(prop) => prop,
            Err(e) => {
                error!("Err is {}", e);
                return Err(PlaintextError::HandshakeParsingFailure);
            }
        };
        let pubkey: Vec<u8> = match prop.pubkey {
            Some(p) => p,
            None => {
                return Err(PlaintextError::EmptyPublicKey);
            }
        };
        let public_key = match PublicKey::from_protobuf_encoding(pubkey.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                error!("Err is {}", e);
                return Err(PlaintextError::HandshakeParsingFailure);
            }
        };
        let peer_id = match PeerId::from_bytes(prop.id.unwrap_or_default()) {
            Ok(id) => id,
            Err(_) => {
                return Err(PlaintextError::HandshakeParsingFailure);
            }
        };
        if peer_id != public_key.clone().into_peer_id() {
            return Err(PlaintextError::MismatchedIDANDPubKey);
        }
        Ok(HandshakeContext {
            config: self.config,
            state: Remote {
                peer_id,
                public_key,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::PlainTextConfig;

    use async_std::task;
    use bytes::BytesMut;
    use futures::channel;
    //use futures::prelude::*;
    use libp2p_core::identity::Keypair;
    use libp2p_traits::{ReadEx, WriteEx};

    fn handshake_with_self_success(
        config_1: PlainTextConfig,
        config_2: PlainTextConfig,
        data: &'static [u8],
    ) {
        let (sender, receiver) = channel::oneshot::channel::<bytes::BytesMut>();
        let (addr_sender, addr_receiver) = channel::oneshot::channel::<::std::net::SocketAddr>();

        task::spawn(async move {
            let listener = async_std::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap();
            let listener_addr = listener.local_addr().unwrap();
            let _res = addr_sender.send(listener_addr);
            let (connect, _) = listener.accept().await.unwrap();
            let (mut handle, _) = config_1.handshake(connect).await.unwrap();
            let mut data = [0u8; 11];
            handle.read2(&mut data).await.unwrap();
            handle.write2(&data).await.unwrap();
        });

        task::spawn(async move {
            let listener_addr = addr_receiver.await.unwrap();
            let connect = async_std::net::TcpStream::connect(&listener_addr)
                .await
                .unwrap();
            let (mut handle, _) = config_2.handshake(connect).await.unwrap();
            handle.write2(data).await.unwrap();
            let mut data = [0u8; 11];
            handle.read2(&mut data).await.unwrap();
            let _res = sender.send(BytesMut::from(&data[..]));
        });

        task::block_on(async move {
            let received = receiver.await.unwrap();
            assert_eq!(received.to_vec(), data);
        });
    }

    #[test]
    fn handshake_with_self_success_secp256k1_small_data() {
        let key_1 = Keypair::generate_secp256k1();
        let key_2 = Keypair::generate_secp256k1();
        handshake_with_self_success(
            PlainTextConfig::new(key_1),
            PlainTextConfig::new(key_2),
            b"hello world",
        )
    }
}

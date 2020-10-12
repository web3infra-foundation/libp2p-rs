use async_std::task;
use libp2p_core::identity;
use libp2p_traits::{ReadEx, WriteEx};
use log::info;
use log::LevelFilter;
use std::string::ToString;
use libp2p_noise::{X25519, Keypair, NoiseConfig};
use libp2p_core::secure_io::SecureInfo;

fn main() {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    if std::env::args().nth(1) == Some("server".to_string()) {
        info!("Starting server ......");
        run_server();
    } else {
        info!("Starting client ......");
        run_client();
    }
}

fn run_server() {
    task::block_on(async {
        let listener = async_std::net::TcpListener::bind("127.0.0.1:3214")
            .await
            .unwrap();

        while let Ok((socket, _)) = listener.accept().await {
            task::spawn(async move {
                let server_id = identity::Keypair::generate_ed25519();
                // let server_id_public = server_id.public();

                let server_dh = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();

                let config = NoiseConfig::xx(server_dh, server_id);

                let (a, mut b) = config.handshake(socket, false).await.unwrap();

                info!("handshake finished");

                let mut buf = [0; 100];

                loop {
                    info!("outside loop");
                    if let Ok(n) = b.read2(&mut buf).await {
                        // info!("public key is {:?}", b.remote_pub_key());
                        info!("data is {:?}", buf.to_vec());
                        // let mut buffer = Vec::from(buf[..11]);
                        let u = b"!";
                        // buffer.push(u[0]);
                        buf[11] = u[0];
                        if b.write_all2(&buf).await.is_err() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            });
        }
    })
}

fn run_client() {
    task::block_on(async {
        let socket = async_std::net::TcpStream::connect("127.0.0.1:3214")
            .await
            .unwrap();
        info!("[client] connected to server: {:?}", socket.peer_addr());

        let client_id = identity::Keypair::generate_ed25519();
        // let client_id_public = client_id.public();

        let client_dh = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();

        let config = NoiseConfig::xx(client_dh, client_id);

        let (_a, mut b) = config.handshake(socket, true).await.unwrap();
        info!("Handshake finished");

        let data = b"hello world";
        b.write2(data).await;
        info!("write finished");
        let mut buf = vec![0u8; 100];
        b.read2(buf.as_mut()).await;
        info!("read finished, {:?}", String::from_utf8(buf).unwrap());
    })
}

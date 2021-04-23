use futures::{AsyncReadExt, AsyncWriteExt};
use libp2prs_core::upgrade::{UpgradeInfo, Upgrader};
use libp2prs_core::{identity, Transport};
use libp2prs_noise::{Keypair, X25519};
use libp2prs_noise::{NoiseConfig, X25519Spec};
use libp2prs_runtime::{
    net::{TcpListener, TcpStream},
    task,
};
use libp2prs_tcp::TcpConfig;
use log::info;

#[test]
fn test_mode_xx() {
    env_logger::builder().is_test(true).filter_level(log::LevelFilter::Debug).init();

    task::block_on(async {
        // server
        task::spawn(async {
            let server_id = identity::Keypair::generate_ed25519();
            let server_auth = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
            let server_config = NoiseConfig::xx(server_auth, server_id);

            log::info!("binding :9876");
            let listener = TcpListener::bind("127.0.0.1:9876").await.unwrap();
            while let Ok((socket, _)) = listener.accept().await {
                let cfg = server_config.clone();
                let (_, mut output) = cfg.handshake(socket, false).await.unwrap();
                info!("handshake finished");
                let mut buf = [0; 12];
                let _ = output.read_exact(&mut buf[..11]).await;
                info!("data is {:?}", buf.to_vec());
                buf[11] = b"!"[0];
                let _ = output.write_all(&buf).await;
                output.flush().await.expect("flush");
            }
        });

        task::sleep(std::time::Duration::from_secs(1)).await;
        let client_id = identity::Keypair::generate_ed25519();
        let client_auth = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();
        let client_config = NoiseConfig::xx(client_auth, client_id);

        //client
        let client_socket = TcpStream::connect("127.0.0.1:9876").await.unwrap();

        log::info!("connect :9876");

        let (_, mut output) = client_config.handshake(client_socket, true).await.unwrap();
        let data = b"hello world";
        let _ = output.write_all(data).await;
        output.flush().await.expect("flush");
        info!("write finished");
        let mut buf = vec![0u8; data.len()];
        let _ = output.read_exact(buf.as_mut()).await;
        info!("read finished, {:?}", String::from_utf8(buf).unwrap());
    });
}

#[test]
#[allow(clippy::clone_double_ref)]
fn test_xx_upgrader() {
    task::block_on(async {
        task::spawn(async {
            let server_id = identity::Keypair::generate_ed25519();
            let server_dh = Keypair::<X25519Spec>::new().into_authentic(&server_id).unwrap();

            let mut l = TcpConfig::new()
                .listen_on("/ip4/127.0.0.1/tcp/8765".parse().unwrap())
                .expect("listen on");

            while let Ok(socket) = l.accept_output().await {
                let cfg = NoiseConfig::xx(server_dh.clone(), server_id.clone());

                let info = cfg.protocol_info().get(0).unwrap().clone();

                let mut output = cfg.upgrade_inbound(socket, info).await.expect("upgrade inbound");

                let mut buf = [0; 12];
                let _ = output.read_exact(&mut buf[..11]).await;
                info!("data is {:?}", buf.to_vec());
                buf[11] = b"!"[0];
                let _ = output.write_all(&buf).await;
                output.flush().await.expect("flush");
            }
        });

        task::sleep(std::time::Duration::from_secs(1)).await;
        let client_id = identity::Keypair::generate_ed25519();
        let client_dh = Keypair::<X25519Spec>::new().into_authentic(&client_id).unwrap();
        let cfg = NoiseConfig::xx(client_dh, client_id);

        let info = cfg.protocol_info().get(0).unwrap().clone();

        let socket = TcpConfig::new()
            .dial("/ip4/127.0.0.1/tcp/8765".parse().unwrap())
            .await
            .expect("dial");

        let mut output = cfg.upgrade_outbound(socket, info).await.expect("upgrade outbound");

        let data = b"hello world";
        let _ = output.write_all(data).await;
        output.flush().await.expect("flush");
        info!("write finished");
        let mut buf = vec![0u8; data.len()];
        let _ = output.read_exact(buf.as_mut()).await;
        info!("read finished, {:?}", String::from_utf8(buf).unwrap());
    });
}

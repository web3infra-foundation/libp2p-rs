use libp2prs_runtime::task;
use std::time::Duration;

fn main() {
    task::block_on(async {
        //let s = TcpStream::connect();

        let h = task::spawn(async {
            {
                task::sleep(Duration::from_secs(1)).await;
            }

            32
        });

        let a = h.wait().await;
        println!("forced closed {:?}", a);

        task::sleep(Duration::from_millis(1200)).await;

        println!("wait for exit ");

        // let a = h.terminate().await;
        // println!("forced closed {:?}", a);

        //h.await;

        //let b = h.cancel().await;
        std::thread::sleep(Duration::from_millis(1000));

        println!("exited");
    });

    std::thread::sleep(Duration::from_millis(1000));
}

use libp2prs_kad::task_limit::TaskLimiter;
use libp2prs_runtime::task;
use log::info;
use std::num::NonZeroUsize;
use std::time::Duration;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut limiter = TaskLimiter::new(NonZeroUsize::new(5).unwrap());

    for i in 0..10 {
        limiter.spawn(async move {
            info!("job {} started...", i);
            task::sleep(Duration::from_secs(5)).await;
            info!("job {} stopped", i);
            Ok(())
        });
    }

    task::block_on(async {
        task::sleep(Duration::from_secs(1)).await;
        let stat = limiter.shutdown().await;
        //let stat = limiter.wait().await;
        info!("stat={:?}", stat);
    })

    //assert_eq!(limiter.handles.len(), 10);
}

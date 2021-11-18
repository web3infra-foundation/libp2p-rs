use std::num::NonZeroUsize;
use std::time::Duration;
use log::info;
use libp2prs_kad::task_limit::LimitedTaskMgr;
use libp2prs_runtime::task;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut limiter = LimitedTaskMgr::new(NonZeroUsize::new(5).unwrap());

    for i in 0..10 {
        limiter.spawn(async move {
            info!("job {} started...", i);
            task::sleep(Duration::from_secs(1)).await;
            info!("job {} stopped", i);
            Ok(())
        });
    }

    task::block_on(async {
        limiter.wait().await;
    })

    //assert_eq!(limiter.handles.len(), 10);

}

use futures::{StreamExt, stream::FuturesUnordered};
use tokio::signal::unix::{SignalKind, signal};
use tokio::time::{Duration, sleep};

use crate::collector::Collector;

mod collector;

async fn run_collector_loop() {
    let config = vec![String::from("microsoft")];

    let enabled_collectors = Collector::load_collector_config(config);
    let factory_map = Collector::build_factory_map();

    loop {
        let mut tasks = FuturesUnordered::new();

        for collector in enabled_collectors.iter() {
            if let Some(factory) = factory_map.get(collector) {
                let collector = factory();

                tasks.push(async move {
                    match collector.collect().await {
                        Ok(_) => println!("INFO: Collector {} finished", collector.name()),
                        Err(e) => eprintln!("ERROR: Collector {} failed: {}", collector.name(), e),
                    }
                });
            }
        }

        while tasks.next().await.is_some() {}

        sleep(Duration::from_secs(5 * 60)).await;
    }
}

async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = sigterm.recv() => {
            println!("INFO: Received SIGTERM");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("INFO: Received SIGINT");
        }
    }
}

#[tokio::main]
async fn main() {
    println!("job-ingestor started. Press Ctrl + C to exit");

    tokio::select! {
        _ = run_collector_loop() => {
            println!("INFO: Collector loop exited");
        }
        _ = shutdown_signal() =>  {
            println!("INFO: Shutdown signal received");
        }
    }
}

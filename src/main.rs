use miner;
use std::env;
use std::thread;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let instance_id = env::args().nth(1).unwrap_or_else(|| "default".to_string());
    println!("instance_id: {}", instance_id);

    let mongo_url = env::var("MONGO_URL").expect("MONGO_URL not set");

    let m = miner::Miner::new(&instance_id, &mongo_url);
    loop {
        println!("================================");
        println!("starting a new run");
        println!("================================");
        m.run()?;
        thread::sleep(Duration::from_millis(100));
    }
}

use miner;
use std::env;
use std::thread;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        println!("Usage: need exactly 1 arguments, got {}", args.len() - 1);
        std::process::exit(1);
    }

    let instance_id = args[1].clone();
    println!("instance_id: {}", instance_id);

    let m = miner::MidnightMiner::new(&instance_id);
    loop {
        println!("================================");
        println!("starting a new run");
        println!("================================");
        m.run()?;
        thread::sleep(Duration::from_millis(100));
    }
}

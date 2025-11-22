use miner::miner::*;
use miner::types::*;
use std::env;

fn main() -> anyhow::Result<()> {
    let instance_id = env::args().nth(1).unwrap_or_else(|| "default".to_string());
    println!("instance_id: {}", instance_id);

    let mongo_url = env::var("MONGO_URL").expect("MONGO_URL not set");

    let mongodb_config = MongodbConfig {
        mongo_url: mongo_url.clone(),
        mongo_db: "defensio".to_string(),
        coll_config: "config".to_string(),
        coll_challenge: "challenge".to_string(),
        coll_address: "address".to_string(),
        coll_submit: "submit".to_string(),
    };

    let m = Miner::new(&instance_id, mongodb_config);
    m.start_mining()
}

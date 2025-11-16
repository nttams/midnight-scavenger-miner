use anyhow::Result;
use ashmaize::*;
use chrono::{DateTime, Utc};
use mongodb::bson::Bson;
use mongodb::bson::doc;
use mongodb::sync::{Collection, Database};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MONGO_URI: &str = "TODO";
const COLL_CONFIG: &str = "config";
const COLL_CHALLENGES: &str = "challenges";
const COLL_ADDRESSES: &str = "addresses";
const COLL_SUBMIT: &str = "submit";

pub struct MidnightMiner {
    cfg: Config,
    stat: Arc<Stat>,
    mongo_client: Option<mongodb::sync::Client>,
    mongo_db: Option<mongodb::sync::Database>,
    coll_submit: Option<Collection<Solution>>,
}

// This stat mixes both per-task and overall stats,
// it's a mess, but it works
struct Stat {
    start_time: AtomicI32,
    hash_counter: AtomicI32,
    success_counter: AtomicI32,
    skip_counter: AtomicI32,
    error_counter: AtomicI32,
    total_task: AtomicI32,
}

impl MidnightMiner {
    pub fn new(instance_id: &str) -> Self {
        let mut miner = MidnightMiner {
            cfg: Config::default(),
            mongo_client: None,
            coll_submit: None,
            mongo_db: None,
            stat: Arc::new(Stat {
                start_time: AtomicI32::new(0),
                hash_counter: AtomicI32::new(0),
                success_counter: AtomicI32::new(0),
                skip_counter: AtomicI32::new(0),
                error_counter: AtomicI32::new(0),
                total_task: AtomicI32::new(0),
            }),
        };

        miner.mongo_client = Some(
            mongodb::sync::Client::with_uri_str(MONGO_URI).expect("failed to init mongo client"),
        );
        miner.mongo_db = Some(miner.mongo_client.as_ref().unwrap().database("mn"));
        miner.coll_submit = Some(miner.mongo_db.as_ref().unwrap().collection(COLL_SUBMIT));

        let cfg = fetch_config(miner.mongo_db.as_ref().unwrap(), &instance_id)
            .expect("failed to fetch config");
        println!("config: {cfg:#?}");

        miner.cfg = cfg;
        miner
    }

    pub fn run(&self) -> anyhow::Result<()> {
        let addresses = fetch_addresses(self.mongo_db.as_ref().unwrap(), &self.cfg.address_id)?;
        println!("fetched {} addresses", addresses.len());

        let challenges = fetch_challenges(self.mongo_db.as_ref().unwrap(), &vec![], 1000)?;
        println!("fetched {} challenges", challenges.len());

        for chall in &challenges {
            println!(
                "challenge_id: {}, difficulty: {}",
                chall.challenge.challenge_id, chall.challenge.difficulty
            );
        }

        let tasks: Vec<Task> = build_tasks(&self.cfg, challenges, addresses)?;
        println!("total tasks to process: {}", tasks.len());
        self.stat
            .total_task
            .store(tasks.len() as i32, Ordering::Relaxed);

        self.create_monitor_thread();

        println!("================================");
        println!("starting solving tasks");
        println!("================================");

        for mut task in tasks {
            self.handle(&mut task);
        }

        Ok(())
    }

    fn handle(&self, task: &mut Task) {
        let stat = Arc::clone(&self.stat);
        stat.hash_counter.store(0, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i32;
        stat.start_time.store(now, Ordering::Relaxed);

        if let Err(e) = self.handle_task(task) {
            if e.to_string().contains("duplicate key error") {
                println!(
                    "⏩ Skip {}:{}, claimed by others or solution is found",
                    task.challenge.challenge.challenge_id,
                    shorten_address(&task.addr)
                );
                self.stat.skip_counter.fetch_add(1, Ordering::Relaxed);
                return;
            }

            println!(
                "❌ Error {}{}: {}",
                task.challenge.challenge.challenge_id,
                shorten_address(&task.addr),
                e
            );
            self.stat.error_counter.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.stat.success_counter.fetch_add(1, Ordering::Relaxed);
    }

    fn handle_task(&self, task: &mut Task) -> Result<()> {
        let challenge_id = task.challenge.challenge.challenge_id.clone();
        let addr_short = shorten_address(&task.addr);

        if task.challenge.is_late(60) {
            let seconds_left = task.challenge.latest_submission_epoch as i64
                - SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

            let msg = format!(
                "{}:{} is late, only {} minutes left",
                challenge_id,
                addr_short,
                seconds_left / 60
            );
            return Err(anyhow::anyhow!(msg));
        }

        // Claim a slot in db, so other instances won't work on same challenge:address
        self.coll_submit
            .as_ref()
            .unwrap()
            .insert_one(&task.solution)
            .run()?;

        //
        // Actually solve
        //

        println!("=================================");
        println!(
            "🚀 Solving {}:{}, difficulty: {}",
            task.challenge.challenge.challenge_id,
            shorten_address(&task.addr),
            task.challenge.challenge.difficulty
        );

        let start = Instant::now();
        task.solution = self.work(&task);
        let time_taken = start.elapsed().as_secs() as i32;

        if task.solution.is_empty() {
            return Err(anyhow::anyhow!(
                "timeout/max hash reached, time taken: {}, hashes: {}",
                format_duration(time_taken),
                task.solution.total_hashes
            ));
        }
        task.solution.time_taken_sec = time_taken;
        println!(
            "💎 Solved {}:{}, time: {}, hash_count: {}",
            challenge_id,
            addr_short,
            format_duration(task.solution.time_taken_sec),
            task.solution.total_hashes
        );

        let status = if task.cfg.self_submit {
            "found_self_submit"
        } else {
            "found"
        }
        .to_string();

        //
        // Save solution to db
        //

        let query = doc! { "_id": &task.solution.id };
        let update = doc! {
            "$set": {
                "nonce": task.solution.nonce.clone(),
                "hash": task.solution.hash.clone(),
                "preimage": task.solution.preimage.clone(),
                "found_time": time_to_string(&Utc::now()),
                "time_taken_sec": start.elapsed().as_secs() as i32,
                "total_hashes": task.solution.total_hashes,
                "status": &status,
            }
        };
        self.coll_submit
            .as_ref()
            .unwrap()
            .update_one(query, update)
            .run()?;
        println!("💾 Saved {}:{}", challenge_id, addr_short);

        Ok(())
    }

    fn work(&self, task: &Task) -> Solution {
        thread::scope(|s| {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let solution_slot = Arc::new(Mutex::new(None));

            for _ in 0..task.cfg.num_threads {
                let stop_flag = Arc::clone(&stop_flag);
                let solution_slot = Arc::clone(&solution_slot);

                s.spawn(move || {
                    self.worker(task, stop_flag, solution_slot);
                });
            }

            while !stop_flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
            }

            let guard = solution_slot.lock().unwrap();
            match guard.clone() {
                Some(mut sol) => {
                    sol.total_hashes = Arc::clone(&self.stat).hash_counter.load(Ordering::Relaxed);
                    sol
                }
                None => {
                    let mut sol = Solution::default();
                    sol.total_hashes = Arc::clone(&self.stat).hash_counter.load(Ordering::Relaxed);
                    sol
                }
            }
        })
    }

    fn worker(
        &self,
        task: &Task,
        stop_flag: Arc<AtomicBool>,
        solution_slot: Arc<Mutex<Option<Solution>>>,
    ) {
        let difficulty = u32::from_str_radix(&task.challenge.challenge.difficulty, 16).unwrap();

        let static_part = format!(
            "{}{}{}{}{}{}",
            task.addr,
            task.challenge.challenge.challenge_id,
            task.challenge.challenge.difficulty,
            task.challenge.challenge.no_pre_mine,
            task.challenge.challenge.latest_submission,
            task.challenge.challenge.no_pre_mine_hour
        );
        let start = Instant::now();
        let mut hash_count: i32 = 0;
        let mut last_report = Instant::now();
        let mut rng = rand::rng();
        while !stop_flag.load(Ordering::Relaxed) {
            let nonce = format!("{:016x}", rng.random::<u64>());

            let mut preimage = String::with_capacity(16 + static_part.len());
            preimage.push_str(&nonce);
            preimage.push_str(&static_part);

            let hash_hex = hash(preimage.as_bytes(), &task.rom, 8, 256);
            let hash_string = hex::encode(hash_hex);

            let hash_value = u32::from_be_bytes(hash_hex[0..4].try_into().unwrap());

            if (hash_value | difficulty) == difficulty {
                if !stop_flag.swap(true, Ordering::Relaxed) {
                    let mut solution = task.solution.clone();
                    solution.nonce = nonce.clone();
                    solution.hash = hash_string.clone();
                    solution.preimage = preimage.clone();
                    solution.found_time = Utc::now();

                    let mut guard = solution_slot.lock().unwrap();
                    *guard = Some(solution);
                }
                break;
            }

            // Statistics and timeout check and max hash count
            hash_count += 1;
            if last_report.elapsed() >= Duration::from_secs(1) {
                self.stat
                    .hash_counter
                    .fetch_add(hash_count, Ordering::Relaxed);
                hash_count = 0;
                last_report = Instant::now();

                // Check timeout
                if start.elapsed() >= Duration::from_secs(task.cfg.timeout_sec as u64) {
                    stop_flag.store(true, Ordering::Relaxed);
                    break;
                }

                // Check hash count limit
                if self.stat.hash_counter.load(Ordering::Relaxed) >= task.cfg.max_hash_count {
                    stop_flag.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    fn create_monitor_thread(&self) {
        let stat = Arc::clone(&self.stat);
        thread::spawn(move || {
            let mut last_total: i32 = 0;
            let mut last_time = Instant::now();
            loop {
                thread::sleep(Duration::from_secs(10));
                let now = Instant::now();
                let total_hashes = stat.hash_counter.load(Ordering::Relaxed);
                let interval_hashes = total_hashes - last_total;
                let interval_secs = now.duration_since(last_time).as_secs_f64();
                let rate = (interval_hashes as f64 / interval_secs) as u64;

                let time_passed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i32
                    - stat.start_time.load(Ordering::Relaxed);

                println!(
                    "⛏️ Hash rate: {:04} hashes/sec, total: {}, time_taken: {}, done: {}, skipped: {}, errors: {}. total tasks: {}",
                    rate,
                    total_hashes,
                    format_duration(time_passed),
                    stat.success_counter.load(Ordering::Relaxed),
                    stat.skip_counter.load(Ordering::Relaxed),
                    stat.error_counter.load(Ordering::Relaxed),
                    stat.total_task.load(Ordering::Relaxed),
                );
                last_total = total_hashes;
                last_time = now;
            }
        });
    }
}

//
// Types
//

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct Solution {
    #[serde(rename = "_id")]
    id: String, // challenge_id:address
    instance_id: String,
    challenge_id: String,
    address: String,
    nonce: String,
    hash: String,
    preimage: String,
    create_time: DateTime<Utc>,
    found_time: DateTime<Utc>,
    submitted_time: DateTime<Utc>,
    time_taken_sec: i32,
    total_hashes: i32,
    status: String, // "onit" | "found" | "submitted"
    submitter_id: String,
}

impl Solution {
    fn is_empty(&self) -> bool {
        self.nonce.is_empty() || self.hash.is_empty() || self.preimage.is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Challenge {
    code: String,
    challenge: ChallengeData,
    mining_period_ends: String,
    max_day: i32,
    total_challenges: i32,
    current_day: i32,
    next_challenge_starts_at: String,
    latest_submission_epoch: i32,
}

impl Challenge {
    fn is_late(&self, minute: i64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.latest_submission_epoch as i64 - now <= minute * 60 // less than x minutes left
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChallengeData {
    challenge_id: String,
    challenge_number: i32,
    day: i32,
    issued_at: String,
    latest_submission: String,
    difficulty: String,
    no_pre_mine: String,
    no_pre_mine_hour: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Address {
    tag: String,
    address: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct Config {
    #[serde(rename = "_id")]
    id: String,
    address_id: String,
    num_threads: i32,
    self_submit: bool,
    submitter_id: String,
    timeout_sec: i32,
    max_hash_count: i32,
}

#[derive(Clone)]
struct Task {
    cfg: Config,
    rom: Arc<Rom>,
    addr: String,
    challenge: Challenge,
    solution: Solution,
}

//
// Helper functions
//

fn create_rom(no_pre_mine: &str) -> Rom {
    const MB: usize = 1024 * 1024;
    const GB: usize = 1024 * MB;

    let rom = Rom::new(
        no_pre_mine.as_bytes(),
        RomGenerationType::TwoStep {
            pre_size: 16 * MB,
            mixing_numbers: 4,
        },
        1 * GB,
    );
    rom
}

fn fetch_config(db: &Database, instance_id: &str) -> Result<Config> {
    let coll: Collection<Config> = db.collection(COLL_CONFIG);

    let filter = doc! { "_id": instance_id };
    let result = coll.find_one(filter).run()?;
    let mut cfg =
        result.ok_or_else(|| anyhow::anyhow!("No config for instance '{}'", instance_id))?;

    if cfg.timeout_sec <= 0 {
        cfg.timeout_sec = 60 * 60;
    }
    if cfg.max_hash_count <= 0 {
        cfg.max_hash_count = 10_000_000;
    }

    Ok(cfg)
}

fn fetch_addresses(db: &Database, address_id: &str) -> Result<Vec<String>> {
    let coll: Collection<Address> = db.collection(COLL_ADDRESSES);

    let filter = doc! { "tag": address_id };
    let cursor = coll.find(filter).run()?;
    let mut addresses = Vec::new();
    for result in cursor {
        let doc = result?;
        addresses.push(doc.address);
    }
    Ok(addresses)
}

fn fetch_challenges(db: &Database, done_chall: &Vec<String>, limit: i64) -> Result<Vec<Challenge>> {
    let time_limit = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64 + 3600;

    let filter = doc! {
        "_id": { "$nin": done_chall },
        "code": "active",
        "latest_submission_epoch": { "$gt": Bson::Int64(time_limit) }
    };

    let find_options = mongodb::options::FindOptions::builder()
        .sort(doc! { "latest_submission_epoch": 1 }) // oldest first
        .limit(limit)
        .build();

    let coll: Collection<Challenge> = db.collection(COLL_CHALLENGES);
    let mut cursor = coll.find(filter).with_options(find_options).run()?;
    let mut challenges = Vec::new();

    while let Some(challenge) = cursor.next() {
        challenges.push(challenge?);
    }

    Ok(challenges)
}

fn build_tasks(
    cfg: &Config,
    challenges: Vec<Challenge>,
    addresses: Vec<String>,
) -> Result<Vec<Task>> {
    let mut rom_cache: HashMap<String, Arc<Rom>> = HashMap::new();

    let mut tasks = Vec::new();
    for challenge in challenges {
        let key = challenge.challenge.no_pre_mine.clone();
        let rom = rom_cache
            .entry(key.clone())
            .or_insert_with(|| Arc::new(create_rom(&key)))
            .clone();

        for addr in &addresses {
            let mut task = Task {
                cfg: cfg.clone(),
                rom: rom.clone(),
                addr: addr.clone(),
                challenge: challenge.clone(),
                solution: Solution::default(),
            };
            task.solution = build_base_solution(&task);

            tasks.push(task);
        }
    }

    Ok(tasks)
}

fn build_base_solution(task: &Task) -> Solution {
    Solution {
        id: format!(
            "{}:{}",
            task.challenge.challenge.challenge_id,
            shorten_address(&task.addr)
        ),
        instance_id: task.cfg.id.clone(),
        challenge_id: task.challenge.challenge.challenge_id.clone(),
        address: task.addr.clone(),
        nonce: "".to_string(),
        hash: "".to_string(),
        preimage: "".to_string(),
        create_time: Utc::now(),
        found_time: Default::default(),
        submitted_time: Default::default(),
        time_taken_sec: 0,
        total_hashes: 0,
        submitter_id: task.cfg.submitter_id.clone(),
        status: if task.cfg.self_submit {
            "onit_self_submit"
        } else {
            "onit"
        }
        .to_string(),
    }
}

fn format_duration(mut seconds: i32) -> String {
    let hours = seconds / 3600;
    seconds %= 3600;
    let minutes = seconds / 60;
    seconds %= 60;

    let mut result = String::new();
    if hours > 0 {
        result.push_str(&format!("{}h", hours));
    }
    if minutes > 0 {
        result.push_str(&format!("{}m", minutes));
    }
    if seconds > 0 || result.is_empty() {
        result.push_str(&format!("{}s", seconds));
    }

    result
}

fn shorten_address(addr: &String) -> String {
    if addr.len() <= 24 {
        return addr.clone();
    }

    let prefix_len = 10;
    let suffix_len = 5;
    let start = &addr[..prefix_len];
    let end = &addr[addr.len() - suffix_len..];
    format!("{}...{}", start, end)
}

fn time_to_string(t: &DateTime<Utc>) -> String {
    return t.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
}

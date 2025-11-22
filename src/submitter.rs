use crate::types::*;
use crate::utils::*;
use anyhow::anyhow;
use chrono::DateTime;
use chrono::Utc;
use mongodb::bson::doc;
use mongodb::sync::Collection;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Submitter {
    cfg: Config,
    client: Client,
    coll_challenge: Collection<Challenge>,
    coll_submit: Collection<Solution>,
}

impl Submitter {
    pub fn new(cfg: Config, mongodb_config: MongodbConfig) -> Self {
        let mongo_client = mongodb::sync::Client::with_uri_str(&mongodb_config.mongo_url)
            .expect("failed to init mongo client");
        let mongo_db = mongo_client.database(&mongodb_config.mongo_db);

        Submitter {
            cfg,
            client: Client::new(),
            coll_challenge: mongo_db.collection(&mongodb_config.coll_challenge),
            coll_submit: mongo_db.collection(&mongodb_config.coll_submit),
        }
    }

    pub fn run(self) -> anyhow::Result<()> {
        println!("Submitter started");

        let this = Arc::new(self.clone());

        let t1 = {
            let s = Arc::clone(&this);
            thread::spawn(move || {
                let mut last_chall_update = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                loop {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    if now - last_chall_update > 5 {
                        match s.fetch_and_update_challenge() {
                            Ok(challenge) => {
                                println!(
                                    "Fetched/wrote to db challenge {}, diff {}",
                                    challenge.id, challenge.challenge.difficulty,
                                );
                            }
                            Err(e) => {
                                if !e.to_string().contains("duplicate key error") {
                                    println!("Error fetching/updating challenge: {:?}", e);
                                }
                            }
                        }

                        last_chall_update = now;
                    }
                }
            })
        };

        {
            let s = Arc::clone(&this);
            thread::spawn(move || {
                loop {
                    if let Err(err) = s.submit_solution_and_record() {
                        println!("Error submitting solutions: {:?}", err);
                    }

                    std::thread::sleep(std::time::Duration::from_secs(1)); // traffic is low, no rush
                }
            })
        };

        t1.join().unwrap();

        Ok(())
    }

    pub fn fetch_and_update_challenge(&self) -> anyhow::Result<Challenge> {
        let challenge = self.fetch_challenge()?;
        self.write_challenge(&challenge)?;
        Ok(challenge)
    }

    pub fn fetch_challenge(&self) -> anyhow::Result<Challenge> {
        let url = format!("{}/challenge", self.cfg.base_url);

        let resp = self.client.get(url).send()?.error_for_status()?;

        let mut data: Challenge = resp.json()?;

        let dt = DateTime::parse_from_rfc3339(&data.challenge.latest_submission).unwrap();
        data.latest_submission_epoch = dt.timestamp() as i32;
        data.id = data.challenge.challenge_id.clone();

        Ok(data)
    }

    pub fn write_challenge(&self, challenge: &Challenge) -> anyhow::Result<()> {
        self.coll_challenge.insert_one(challenge).run()?;
        Ok(())
    }

    pub fn submit_solution_and_record(&self) -> anyhow::Result<()> {
        let filter = doc! { "status": "found" };
        let cursor = self.coll_submit.find(filter).run()?;
        for result in cursor {
            let doc = result?;

            match self.submit_solution(&doc) {
                Ok(resp) => {
                    let update = doc! {
                        "$set": {
                            "status": "submitted",
                            "submitted_time": time_to_string(&Utc::now()),
                            "submit_response": mongodb::bson::to_bson(&resp)?,
                        }
                    };
                    self.coll_submit
                        .update_one(doc! { "_id": &doc.id }, update)
                        .run()?;

                    println!("Submitted {}", &doc.id);
                }
                Err(e) => {
                    println!("Error submitting solution: {:?}", e);
                    let update = doc! {
                        "$set": {
                            "status": e.to_string(),
                        }
                    };
                    self.coll_submit
                        .update_one(doc! { "_id": doc.id }, update)
                        .run()?;
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(1)); // traffic is low, no rush
        }
        Ok(())
    }

    pub fn submit_solution(&self, solution: &Solution) -> anyhow::Result<SubmitResponse> {
        let resp = self
            .client
            .post(format!(
                "{}/solution/{}/{}/{}",
                self.cfg.base_url, solution.address, solution.challenge_id, solution.nonce
            ))
            .send()?
            .error_for_status()?;

        let status = resp.status();
        let body = resp.text().unwrap_or_default();

        // Non-200 error handling
        if status != StatusCode::OK && status != StatusCode::CREATED {
            return Err(anyhow!("non-OK HTTP status: {}, body: {}", status, body));
        }

        // Parse JSON
        let parsed: SubmitResponse = serde_json::from_str(&body)
            .map_err(|err| anyhow!("invalid JSON: {}, body: {}", err, body))?;

        Ok(parsed)
    }
}

#[derive(Clone)]
pub struct Config {
    pub base_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_challenge() {
        let cfg = Config {
            base_url: "https://mine.defensio.io/api".to_string(),
        };

        let mut mongo_cfg = MongodbConfig::default();
        mongo_cfg.mongo_url = "mongodb://localhost:27017".to_string();
        let submitter = Submitter::new(cfg, mongo_cfg);

        let chall = submitter.fetch_challenge().unwrap();

        println!(
            "Fetched challenge: {}",
            serde_json::to_string(&chall).unwrap()
        );
    }
}

use crate::types::*;
use chrono::DateTime;
use reqwest::blocking::Client;

pub struct Submitter {
    cfg: Config,
    client: Client,
}

impl Submitter {
    pub fn new(cfg: Config) -> Self {
        Submitter {
            cfg,
            client: Client::new(),
        }
    }

    pub fn fetch_challenge(&self) -> anyhow::Result<Challenge> {
        let url = format!("{}/challenge", self.cfg.base_url);

        let resp = self.client.get(url).send()?.error_for_status()?;

        let mut data: Challenge = resp.json()?;

        let dt = DateTime::parse_from_rfc3339(&data.challenge.latest_submission).unwrap();
        data.latest_submission_epoch = dt.timestamp() as i32;

        Ok(data)
    }
}

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

        let submitter = Submitter::new(cfg);

        let chall = submitter.fetch_challenge().unwrap();

        println!(
            "Fetched challenge: {}",
            serde_json::to_string(&chall).unwrap()
        );
    }
}

use chrono::TimeDelta;
use serde::Deserialize;
use rand::Rng;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerOptions {
    cache_size: usize, // max size for each upload to be cached
    block_size: usize, // size of each chunk in bytes. if this is set to 0, uploads will be blocked
    cull_time: TimeDelta, // time after which an upload is removed from cache when considered stale
    token_format: String, // This is for the path of downloads. Normally {number}-{word}-{word}-{word}. options are {number}, {word}, {uuid}
    upload_format: String, // same as above.
    packet_delay: Option<TimeDelta> // time to limit between each packet
}

impl ServerOptions {
    pub fn new(cache_size: usize, block_size: usize, cull_time: TimeDelta, token_format: String, upload_format: String, packet_delay: Option<TimeDelta>) -> Self {
        ServerOptions {
            cache_size,
            block_size,
            cull_time,
            token_format,
            upload_format,
            packet_delay
        }
    }

    pub fn get_cache_size(&self) -> usize {
        self.cache_size
    }

    pub fn get_block_size(&self) -> usize {
        self.block_size
    }

    pub fn get_cull_time(&self) -> TimeDelta {
        self.cull_time
    }

    pub fn get_delay_time(&self) -> Option<TimeDelta> {
        self.packet_delay
    }

    fn generate_token(format: &String) -> String {
        // we need to see how many of each we need
        let mut rng = rand::rng();
        let words_raw = include_str!("../../wordlist.txt").trim(); // via https://gist.githubusercontent.com/dracos/dd0668f281e685bad51479e5acaadb93/raw/6bfa15d263d6d5b63840a8e5b64e04b382fdb079/valid-wordle-words.txt
        // now split by newlines
        let words = words_raw.split('\n').collect::<Vec<&str>>();

        let mut output = format.clone();
        while output.contains("{number}") {
            let number = rng.random_range(0..100);
            output = output.replacen("{number}", &number.to_string(), 1);
        }

        while output.contains("{word}") {
            let word = words[rng.random_range(0..words.len())].to_string();
            output = output.replacen("{word}", &word, 1);
        }

        while output.contains("{uuid}") {
            let uuid = Uuid::new_v4().to_string();
            output = output.replacen("{uuid}", &uuid, 1);
        }

        output
    }

    pub fn generate_upload_token(&self) -> String {
        return Self::generate_token(&self.token_format)
    }

    pub fn generate_key_token(&self) -> String {
        return Self::generate_token(&self.upload_format)
    }


}
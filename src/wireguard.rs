use crate::mongo::Mongo;
use configparser::ini::Ini;
use mongodb::bson::{doc, DateTime};
use serde::{Deserialize, Serialize};
use simple_error::{SimpleError, SimpleResult};
use std::collections::HashSet;
use std::io::Write;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
#[derive(Serialize, Deserialize, Debug)]
pub struct Peer {
    pub user_id: u64,
    pub username: String,
    pub public_key: Option<String>,
    pub private_key: Option<String>,
    pub ip: Option<Ipv4Addr>,
    pub date: DateTime,
}

pub async fn add_peer(peer: &mut Peer, mongo: &Mongo) {
    let (private_key, public_key) = gen_keys();
    peer.private_key = Some(private_key);
    peer.public_key = Some(public_key);
    peer.ip = Some(get_ip(&mut mongo.get_peers().await));
    let mut wg = match Command::new("/usr/bin/wg")
        .args([
            "set",
            "wg0",
            "peer",
            format!("{}", peer.public_key.clone().unwrap()).as_str(),
            "allowed-ips",
            format!("{}/32", peer.ip.unwrap()).as_str(),
        ])
        .spawn()
    {
        Err(why) => panic!("Cannot run wg: {}", why),
        Ok(wg) => wg,
    };
    wg.wait().expect("Could not add peer to wg");
    mongo.update(peer).await;
}

pub fn gen_conf(peer: &Peer) -> SimpleResult<String> {
    let mut config = Ini::new_cs();
    config.set(
        "Interface",
        "PrivateKey",
        Some(peer.private_key.clone().unwrap()),
    );
    config.set(
        "Interface",
        "Address",
        Some(format!("{}/16", peer.ip.unwrap().to_string())),
    );
    config.set("Interface", "DNS", Some("8.8.8.8".to_string()));
    config.set(
        "Peer",
        "PublicKey",
        Some("kFpzem87OujfORpD9WkVD7vjjESONndZRcT32Dw0xWg=".to_string()),
    );
    config.set("Peer", "Endpoint", Some("194.87.186.2:51820".to_string()));
    config.set("Peer", "AllowedIPs", Some("0.0.0.0/0".to_string()));
    let config_path = format!("/home/amid/{}", peer.username);
    match config.write(&config_path) {
        Err(why) => {
            log::error!("Cannot save a client config: {}", why);
            Err(SimpleError::from(why))
        }
        Ok(_) => Ok(config_path),
    }
}

fn get_ip(peers: &mut Vec<Peer>) -> Ipv4Addr {
    let mut ip_set = HashSet::new();
    for i in 0..255 {
        for j in 2..255 {
            ip_set.insert(Ipv4Addr::new(10, 0, i, j));
        }
    }
    let peers_ip_set: HashSet<Ipv4Addr> = peers.into_iter().flat_map(|peer| peer.ip).collect();
    ip_set.difference(&peers_ip_set).next().unwrap().to_owned()
}

fn gen_keys() -> (String, String) {
    let genkey_process = match Command::new("/usr/bin/wg")
        .arg("genkey")
        .stdout(Stdio::piped())
        .spawn()
    {
        Err(why) => panic!("Could not run wg genkey: {}", why),
        Ok(genkey_process) => genkey_process,
    };

    let genkey_output = match genkey_process.wait_with_output() {
        Err(why) => panic!("Could not run wg genkey: {}", why),
        Ok(genkey_output) => genkey_output,
    };

    if !genkey_output.status.success() {
        panic!(
            "wg genkey finished with code {}",
            String::from_utf8(genkey_output.stderr).unwrap()
        );
    }

    let private_key =
        String::from_utf8(genkey_output.stdout).expect("Cannot convert wg genkey to string");

    let mut pubkey_process = match Command::new("/usr/bin/wg")
        .arg("pubkey")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Err(why) => panic!("Could not run wg pubkey: {}", why),
        Ok(pubkey_process) => pubkey_process,
    };

    match pubkey_process
        .stdin
        .take()
        .unwrap()
        .write_all(&private_key.as_bytes())
    {
        Err(why) => panic!("Couldn't write to wg pubkey stdin: {}", why),
        Ok(_) => (),
    }

    let pubkey_output = match pubkey_process.wait_with_output() {
        Err(why) => panic!("Could not run wg genkey: {}", why),
        Ok(pubkey_output) => pubkey_output,
    };

    if !pubkey_output.status.success() {
        panic!(
            "wg pubkey finished with code {}",
            String::from_utf8(pubkey_output.stderr).unwrap()
        );
    }
    let public_key =
        String::from_utf8(pubkey_output.stdout).expect("Cannot convert wg pubkey to string");

    (
        private_key.trim().to_string(),
        public_key.trim().to_string(),
    )
}

#[cfg(test)]
#[test]
fn generate_keys() {
    let (private, public) = gen_keys();
    println!("{}", private.len());
    assert!(private.len() == 44 && public.len() == 44);
}

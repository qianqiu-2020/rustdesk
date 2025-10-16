use crate::client::*;
use async_trait::async_trait;
use hbb_common::{
    base64::{engine::general_purpose::STANDARD, Engine as _},
    config::{Config, PeerConfig},
    config::READ_TIMEOUT,
    futures::{SinkExt, StreamExt},
    log,
    message_proto::*,
    protobuf::Message as _,
    rendezvous_proto::ConnType,
    tokio::{self, sync::mpsc},
    Stream,
    ResultType,
};
use serde_derive::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Session {
    id: String,
    lc: Arc<RwLock<LoginConfigHandler>>,
    sender: mpsc::UnboundedSender<Data>,
    password: String,
}

impl Session {
    pub fn new(id: &str, sender: mpsc::UnboundedSender<Data>) -> Self {
        let mut password = "".to_owned();
        if PeerConfig::load(id).password.is_empty() {
            password = rpassword::prompt_password("Enter password: ").unwrap();
        }
        let session = Self {
            id: id.to_owned(),
            sender,
            password,
            lc: Default::default(),
        };
        session.lc.write().unwrap().initialize(
            id.to_owned(),
            ConnType::PORT_FORWARD,
            None,
            false,
            None,
            None,
            None,
        );
        session
    }
}

#[async_trait]
impl Interface for Session {
    fn set_multiple_windows_session(&self, _sessions: Vec<WindowsSession>) {
        // Not needed for CLI
    }

    fn get_lch(&self) -> Arc<RwLock<LoginConfigHandler>> {
        return self.lc.clone();
    }

    fn msgbox(&self, msgtype: &str, title: &str, text: &str, link: &str) {
        match msgtype {
            "input-password" => {
                self.sender
                    .send(Data::Login((self.password.clone(), "".to_string(), "".to_string(), true)))
                    .ok();
            }
            "re-input-password" => {
                log::error!("{}: {}", title, text);
                match rpassword::prompt_password("Enter password: ") {
                    Ok(password) => {
                        let login_data = Data::Login((password, "".to_string(), "".to_string(), true));
                        self.sender.send(login_data).ok();
                    }
                    Err(e) => {
                        log::error!("reinput password failed, {:?}", e);
                    }
                }
            }
            msg if msg.contains("error") => {
                log::error!("{}: {}: {}", msgtype, title, text);
            }
            _ => {
                log::info!("{}: {}: {}", msgtype, title, text);
            }
        }
    }

    fn handle_login_error(&self, err: &str) -> bool {
        handle_login_error(self.lc.clone(), err, self)
    }

    fn handle_peer_info(&self, pi: PeerInfo) {
        self.lc.write().unwrap().handle_peer_info(&pi);
    }

    async fn handle_hash(&self, pass: &str, hash: Hash, peer: &mut Stream) {
        log::info!(
            "password={}",
            hbb_common::password_security::temporary_password()
        );
        handle_hash(self.lc.clone(), &pass, hash, self, peer).await;
    }

    async fn handle_login_from_ui(
        &self,
        os_username: String,
        os_password: String,
        password: String,
        remember: bool,
        peer: &mut Stream,
    ) {
        handle_login_from_ui(
            self.lc.clone(),
            os_username,
            os_password,
            password,
            remember,
            peer,
        )
        .await;
    }

    async fn handle_test_delay(&self, t: TestDelay, peer: &mut Stream) {
        handle_test_delay(t, peer).await;
    }

    fn send(&self, data: Data) {
        self.sender.send(data).ok();
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn connect_test(id: &str, key: String, token: String) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Data>();
    let handler = Session::new(&id, sender);
    match crate::client::Client::start(id, &key, &token, ConnType::PORT_FORWARD, handler).await {
        Err(err) => {
            log::error!("Failed to connect {}: {}", &id, err);
        }
        Ok((stream_info, direct)) => {
            log::info!("direct: {:?}", direct);
            let (mut stream, _is_direct, _seal_bytes, _kcp, _protocol) = stream_info;
            // rpassword::prompt_password("Input anything to exit").ok();
            loop {
                tokio::select! {
                    res = hbb_common::timeout(READ_TIMEOUT, stream.next()) => match res {
                        Err(_) => {
                            log::error!("Timeout");
                            break;
                        }
                        Ok(Some(Ok(bytes))) => {
                            if let Ok(msg_in) = Message::parse_from_bytes(&bytes) {
                                match msg_in.union {
                                    Some(message::Union::Hash(hash)) => {
                                        log::info!("Got hash");
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn start_one_port_forward(
    id: String,
    port: i32,
    remote_host: String,
    remote_port: i32,
    key: String,
    token: String,
) {
    crate::common::test_rendezvous_server();
    crate::common::test_nat_type();
    let (sender, mut receiver) = mpsc::unbounded_channel::<Data>();
    let handler = Session::new(&id, sender);
    if let Err(err) = crate::port_forward::listen(
        handler.id.clone(),
        handler.password.clone(),
        port,
        handler.clone(),
        receiver,
        &key,
        &token,
        handler.lc.clone(),
        remote_host,
        remote_port,
    )
    .await
    {
        log::error!("Failed to listen on {}: {}", port, err);
    }
    log::info!("port forward (:{}) exit", port);
}

/// Server configuration structure for CLI import/export
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub relay: String,
    #[serde(default)]
    pub api: String,
    #[serde(default)]
    pub key: String,
}

impl ServerConfig {
    /// Create ServerConfig from current configuration
    pub fn from_current_config() -> Self {
        Self {
            host: Config::get_option("custom-rendezvous-server"),
            relay: Config::get_option("relay-server"),
            api: Config::get_option("api-server"),
            key: Config::get_option("key"),
        }
    }

    /// Encode configuration to base64 string (compatible with Flutter implementation)
    pub fn encode(&self) -> ResultType<String> {
        let json = serde_json::to_string(self)?;
        let bytes = json.as_bytes();
        let base64_str = STANDARD.encode(bytes);
        // Reverse the string to match Flutter implementation
        Ok(base64_str.chars().rev().collect())
    }

    /// Decode configuration from base64 string (compatible with Flutter implementation)
    pub fn decode(encoded: &str) -> ResultType<Self> {
        // Try direct JSON decode first (back compatible)
        if let Ok(config) = serde_json::from_str::<ServerConfig>(encoded) {
            return Ok(config);
        }
        // Try base64 decode with reverse
        let reversed: String = encoded.chars().rev().collect();
        // Try standard base64 decode with reverse (for strings with padding)
        if let Ok(decoded) = STANDARD.decode(&reversed) {
            if let Ok(json_str) = String::from_utf8(decoded) {
                if let Ok(config) = serde_json::from_str::<ServerConfig>(&json_str) {
                    return Ok(config);
                }
            }
        }

        hbb_common::bail!("Failed to decode configuration string")
    }

    /// Apply configuration to current settings
    pub fn apply(&self) -> ResultType<()> {
        let remove_end_slash = |input: &str| -> String {
            if input.ends_with('/') {
                input[..input.len() - 1].to_string()
            } else {
                input.to_string()
            }
        };

        let host = remove_end_slash(&self.host.trim());
        let relay = remove_end_slash(&self.relay.trim());
        let api = remove_end_slash(&self.api.trim());
        let key = self.key.trim();

        if !host.is_empty() {
            Config::set_option("custom-rendezvous-server".to_string(), host);
        }
        if !relay.is_empty() {
            Config::set_option("relay-server".to_string(), relay);
        }
        if !api.is_empty() {
            Config::set_option("api-server".to_string(), api);
        }
        if !key.is_empty() {
            Config::set_option("key".to_string(), key.to_string());
        }

        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        !self.host.trim().is_empty()
    }
}

/// Export current server configuration as encoded string
pub fn export_server_config() -> ResultType<String> {
    let config = ServerConfig::from_current_config();
    
    if !config.is_valid() {
        hbb_common::bail!("No server configuration found. Please configure at least the rendezvous server first.");
    }
    
    config.encode()
}

/// Import server configuration from encoded string
pub fn import_server_config(config_str: &str) -> ResultType<()> {
    let config_str = config_str.trim();
    if config_str.is_empty() {
        hbb_common::bail!("Configuration string is empty");
    }

    let config = ServerConfig::decode(config_str)?;
    
    if !config.is_valid() {
        hbb_common::bail!("Invalid server configuration: rendezvous server is required");
    }

    config.apply()?;
    Ok(())
}

/// Show current server configuration
pub fn show_server_config() {
    let config = ServerConfig::from_current_config();
    
    println!("Current Server Configuration:");
    println!("============================");
    
    if config.host.is_empty() && config.relay.is_empty() && config.api.is_empty() && config.key.is_empty() {
        println!("No custom server configuration found.");
        println!("Using default servers.");
        return;
    }
    
    if !config.host.is_empty() {
        println!("Rendezvous Server: {}", config.host);
    } else {
        println!("Rendezvous Server: (using default)");
    }
    
    if !config.relay.is_empty() {
        println!("Relay Server:      {}", config.relay);
    } else {
        println!("Relay Server:      (using default)");
    }
    
    if !config.api.is_empty() {
        println!("API Server:        {}", config.api);
    } else {
        println!("API Server:        (using default)");
    }
    
    if !config.key.is_empty() {
        println!("Key:               {}", config.key);
    } else {
        println!("Key:               (not set)");
    }
    
    println!();
    println!("To export this configuration, use: --export-config");
}

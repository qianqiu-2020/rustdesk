#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use librustdesk::*;

#[cfg(any(target_os = "android", target_os = "ios", feature = "flutter"))]
fn main() {
    if !common::global_init() {
        eprintln!("Global initialization failed.");
        return;
    }
    common::test_rendezvous_server();
    common::test_nat_type();
    common::global_clean();
}

#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    feature = "cli",
    feature = "flutter"
)))]
fn main() {
    if !common::global_init() {
        return;
    }
    #[cfg(all(windows, not(feature = "inline")))]
    unsafe {
        winapi::um::shellscalingapi::SetProcessDpiAwareness(2);
    }
    if let Some(args) = crate::core_main::core_main().as_mut() {
        ui::start(args);
    }
    common::global_clean();
}

#[cfg(feature = "cli")]
fn main() {
    if !common::global_init() {
        return;
    }
    use clap::{Arg, Command};
    use hbb_common::log;
    let matches = Command::new("rustdesk")
        .version(crate::VERSION)
        .author("Purslane Ltd<info@rustdesk.com>")
        .about("RustDesk command line tool")
        .arg(Arg::new("port-forward")
            .short('p')
            .long("port-forward")
            .value_name("PORT-FORWARD-OPTIONS")
            .help("Format: remote-id:local-port:remote-port[:remote-host]")
            .action(clap::ArgAction::Set))
        .arg(Arg::new("connect")
            .short('c')
            .long("connect")
            .value_name("REMOTE_ID")
            .help("test only")
            .action(clap::ArgAction::Set))
        .arg(Arg::new("key")
            .short('k')
            .long("key")
            .value_name("KEY")
            .help("")
            .action(clap::ArgAction::Set))
        .arg(Arg::new("get-hwcodec-status")
            .long("get-hwcodec-status")
            .help("Get current hardware codec status")
            .action(clap::ArgAction::SetTrue))
        .arg(Arg::new("export-config")
            .long("export-config")
            .help("Export server configuration as encoded string")
            .action(clap::ArgAction::SetTrue))
        .arg(Arg::new("import-config")
            .long("import-config")
            .value_name("CONFIG_STRING")
            .help("Import server configuration from encoded string")
            .action(clap::ArgAction::Set))
        .arg(Arg::new("show-config")
            .long("show-config")
            .help("Show current server configuration")
            .action(clap::ArgAction::SetTrue))
        .try_get_matches();
    
    let matches = match matches {
        Ok(m) => m,
        Err(e) => {
            // If clap fails to parse arguments, check if it's because of unknown arguments
            // In that case, delegate to core_main for handling
            if e.kind() == clap::error::ErrorKind::UnknownArgument {
                // Don't initialize logger here, let core_main handle it
                if let Some(args) = crate::core_main::core_main().as_mut() {
                    // core_main handled the arguments, we're done
                    println!("core_main handled the arguments.");
                }
                common::global_clean();
                return;
            } else {
                // Other clap errors should be displayed
                e.exit();
            }
        }
    };

    // Initialize logging after successful argument parsing
    use hbb_common::{config::LocalConfig, env_logger::*};
    init_from_env(Env::default().filter_or(DEFAULT_FILTER_ENV, "info"));

    // Handle configuration commands
    if matches.get_flag("export-config") {
        let config = cli::export_server_config();
        match config {
            Ok(config_str) => {
                println!("Server configuration (copy this string):");
                println!("{}", config_str);
            }
            Err(e) => {
                eprintln!("Failed to export configuration: {}", e);
                std::process::exit(1);
            }
        }
        return;
    } else if let Some(config_str) = matches.get_one::<String>("import-config") {
        match cli::import_server_config(config_str) {
            Ok(_) => {
                println!("Server configuration imported successfully");
            }
            Err(e) => {
                eprintln!("Failed to import configuration: {}", e);
                std::process::exit(1);
            }
        }
        return;
    } else if matches.get_flag("show-config") {
        cli::show_server_config();
        return;
    } else if matches.get_flag("get-hwcodec-status") {
        let hwcodec_status = hbb_common::config::Config::get_option("enable-hwcodec");
        let enabled = hbb_common::config::option2bool("enable-hwcodec", &hwcodec_status);
        println!("Hardware codec: {}", if enabled { "Enabled" } else { "Disabled" });
        
        // Also show availability information
        #[cfg(feature = "hwcodec")]
        {
            println!("Hardware codec availability:");
            
            // Force check available hardware codecs directly
            let hwcodec_config_str = scrap::hwcodec::check_available_hwcodec();
            if let Ok(hwcodec_config) = serde_json::from_str::<serde_json::Value>(&hwcodec_config_str) {
                if let Some(ram_encode) = hwcodec_config.get("ram_encode").and_then(|v| v.as_array()) {
                    let h264_encoders: Vec<_> = ram_encode.iter()
                        .filter(|codec| codec.get("format").and_then(|f| f.as_str()) == Some("H264"))
                        .collect();
                    let h265_encoders: Vec<_> = ram_encode.iter()
                        .filter(|codec| codec.get("format").and_then(|f| f.as_str()) == Some("H265"))
                        .collect();
                    
                    if !h264_encoders.is_empty() {
                        let names: Vec<String> = h264_encoders.iter()
                            .filter_map(|codec| codec.get("name").and_then(|n| n.as_str()))
                            .map(|s| s.to_string())
                            .collect();
                        println!("  H264 hardware encoder: Available ({})", names.join(", "));
                    } else {
                        println!("  H264 hardware encoder: Not available");
                    }
                    
                    if !h265_encoders.is_empty() {
                        let names: Vec<String> = h265_encoders.iter()
                            .filter_map(|codec| codec.get("name").and_then(|n| n.as_str()))
                            .map(|s| s.to_string())
                            .collect();
                        println!("  H265 hardware encoder: Available ({})", names.join(", "));
                    } else {
                        println!("  H265 hardware encoder: Not available");
                    }
                } else {
                    println!("  No hardware encoders detected");
                }
            } else {
                println!("  Failed to detect hardware codec capabilities");
            }
        }
        #[cfg(not(feature = "hwcodec"))]
        {
            println!("Hardware codec support not compiled in (missing 'hwcodec' feature)");
        }
        return;
    } else if let Some(p) = matches.get_one::<String>("port-forward") {
        let options: Vec<String> = p.split(":").map(|x| x.to_owned()).collect();
        if options.len() < 3 {
            log::error!("Wrong port-forward options");
            return;
        }
        let mut port = 0;
        if let Ok(v) = options[1].parse::<i32>() {
            port = v;
        } else {
            log::error!("Wrong local-port");
            return;
        }
        let mut remote_port = 0;
        if let Ok(v) = options[2].parse::<i32>() {
            remote_port = v;
        } else {
            log::error!("Wrong remote-port");
            return;
        }
        let mut remote_host = "localhost".to_owned();
        if options.len() > 3 {
            remote_host = options[3].clone();
        }
        common::test_rendezvous_server();
        common::test_nat_type();
        let key = matches.get_one::<String>("key").map(|s| s.clone()).unwrap_or_default();
        let token = LocalConfig::get_option("access_token");
        cli::start_one_port_forward(
            options[0].clone(),
            port,
            remote_host,
            remote_port,
            key,
            token,
        );
    } else if let Some(p) = matches.get_one::<String>("connect") {
        common::test_rendezvous_server();
        common::test_nat_type();
        let key = matches.get_one::<String>("key").map(|s| s.clone()).unwrap_or_default();
        let token = LocalConfig::get_option("access_token");
        cli::connect_test(p, key, token);
    }
    common::global_clean();
}

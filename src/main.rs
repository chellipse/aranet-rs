use std::{env, fs, net::ToSocketAddrs, time::Duration};

use anyhow::Result;
use bluer::{agent::Agent, AdapterEvent, Address, Device};
use clap::{Parser, Subcommand};
use futures::prelude::*;
use prometheus::{register, Gauge, IntGauge};
use serde::{de::DeserializeOwned, Deserialize};

use aranet::{bluetooth::*, metric};
use tokio::time::timeout;

#[derive(Deserialize)]
pub struct Cfg {
    /// EX: hci0
    pub adapter: String,
    /// FORMAT: ED:12:89:6C:08:37
    pub macs: Vec<String>,
    pub fahrenheit: Option<bool>,
    // Seconds
    pub stream_freq: Option<u64>,
    pub prometheus_address: Option<String>,
    pub conn_timeout_ms: Option<u64>,
}

pub fn try_get_cfg<T: DeserializeOwned>() -> Result<T> {
    let home = env::var("HOME")?;
    let content = fs::read_to_string(format!("{home}/.config/aranet/config.toml"))?;
    let config = toml::from_str::<T>(&content)?;
    Ok(config)
}

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Debug, Clone, Subcommand)]
enum Cmd {
    Oneline,
    StreamingOneline,
    Service,
}

fn main() {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Building runtime failed");

    rt.block_on(async {
        let cfg = try_get_cfg::<Cfg>().unwrap();
        let mut addresses: Vec<Address> = cfg
            .macs
            .iter()
            .map(|x| str_mac_to_array(x).unwrap())
            .map(|x| Address::new(x))
            .collect();

        let session = bluer::Session::new().await.unwrap();

        let _agent = session
            .register_agent(Agent {
                request_passkey: Some(Box::new(get_passkey)),
                ..Default::default()
            })
            .await
            .unwrap();

        let adapter = session.adapter(&cfg.adapter).unwrap();
        adapter.set_powered(true).await.unwrap();

        let main_adapter = adapter.clone();

        let (dev_sender, mut dev_receiver) = tokio::sync::mpsc::unbounded_channel::<Device>();

        tokio::spawn(async move {
            if let Ok(mut stream) = adapter.discover_devices().await {
                eprintln!("Discovering...");
                while let Some(event) = stream.next().await {
                    // eprintln!("Event: {event:?}");
                    match event {
                        AdapterEvent::DeviceAdded(address) => {
                            if let Some(idx) = addresses.iter().position(|x| x == &address) {
                                // Remove found addresses so we don't try them multiple times
                                addresses.swap_remove(idx);

                                eprintln!("Found: {address:?}");
                                if let Ok(device) = adapter.device(address) {
                                    let sender = dev_sender.clone();
                                    tokio::spawn(async move {
                                        if !device.is_connected().await? {
                                            eprintln!("    Connecting: {device:?}");
                                            device.connect().await?;
                                            eprintln!("    Connected!: {device:?}");
                                        }

                                        eprintln!("    Scanning: {device:?}");

                                        let mut count: u32 = 0;
                                        loop {
                                            let x = device.rssi().await?;
                                            eprintln!("    RSSI: {x:?} on {device:?}");
                                            match x {
                                                Some(_) => {
                                                    sender.send(device).unwrap();
                                                    break;
                                                }
                                                _ => {
                                                    count += 1;
                                                    tokio::time::sleep(Duration::from_millis(200))
                                                        .await
                                                }
                                            }

                                            // Just so we don't busy loop forever on connections which aren't present
                                            // this probably can't happen but i'm not 100% sure.
                                            if count > 100 {
                                                break;
                                            }
                                        }

                                        Ok::<(), bluer::Error>(())
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        });

        // dev should already be connected from the task
        let dev = timeout(
            Duration::from_millis(cfg.conn_timeout_ms.unwrap_or(15000)),
            dev_receiver.recv(),
        )
        .await
        .expect("Timeout while searching for device")
        .expect("Failed to find device within range");

        eprintln!("Dev: {dev:?}");

        match dev.is_paired().await {
            Ok(is_paired) => {
                if !is_paired {
                    println!("Device is not paired. Attempting to pair...");

                    match dev.pair().await {
                        Ok(_) => println!("Pairing successful!"),
                        Err(err) => eprintln!("Pairing failed: {:?}", err),
                    }
                }
            }
            Err(e) => {
                println!("Device Err: {e:?}");
                println!(
                    "Available device addresses: {:#?}",
                    main_adapter.device_addresses().await
                );
                return;
            }
        }

        let endpoint = map_device_endpoints(&dev).await.unwrap();

        if let Some(cmd) = cli.cmd {
            match cmd {
                Cmd::Oneline => {
                    let readings = endpoint.read().await.unwrap();
                    readings.print_oneline(cfg.fahrenheit.unwrap_or(false));
                }
                Cmd::StreamingOneline => loop {
                    if !dev.is_connected().await.unwrap() {
                        dev.connect().await.unwrap();
                    }

                    let readings = endpoint.read().await.unwrap();
                    readings.print_oneline(cfg.fahrenheit.unwrap_or(false));
                    tokio::time::sleep(Duration::from_secs(cfg.stream_freq.unwrap_or(30))).await;
                },
                Cmd::Service => {
                    let address = cfg
                        .prometheus_address
                        .unwrap_or("127.0.0.1:8080".to_string())
                        .to_socket_addrs()
                        .unwrap()
                        .next()
                        .unwrap();
                    metric::start_prometheus_listener_task(address)
                        .await
                        .unwrap();

                    let metric_co2 = IntGauge::new("aranet_co2", "Co2 in ppm").unwrap();
                    register(Box::new(metric_co2.clone()))
                        .map(|()| metric_co2.clone())
                        .unwrap();

                    let metric_temp_f =
                        Gauge::new("aranet_temp_fahrenheit", "Temp in Fahrenheit").unwrap();
                    register(Box::new(metric_temp_f.clone()))
                        .map(|()| metric_temp_f.clone())
                        .unwrap();

                    let metric_temp_c =
                        Gauge::new("aranet_temp_celsius", "Temp in Celsius").unwrap();
                    register(Box::new(metric_temp_c.clone()))
                        .map(|()| metric_temp_c.clone())
                        .unwrap();

                    let metric_relative_humidity =
                        IntGauge::new("aranet_relative_humidity", "Relative humidity %").unwrap();
                    register(Box::new(metric_relative_humidity.clone()))
                        .map(|()| metric_relative_humidity.clone())
                        .unwrap();

                    let metric_preasure =
                        Gauge::new("aranet_preasure", "Air preasure in hPa").unwrap();
                    register(Box::new(metric_preasure.clone()))
                        .map(|()| metric_preasure.clone())
                        .unwrap();

                    let metric_bat = IntGauge::new("aranet_bat", "Aranet4 battery %").unwrap();
                    register(Box::new(metric_bat.clone()))
                        .map(|()| metric_bat.clone())
                        .unwrap();

                    loop {
                        if !dev.is_connected().await.unwrap() {
                            dev.connect().await.unwrap();
                        }
                        let readings = endpoint.read().await.unwrap();
                        readings.print_oneline(cfg.fahrenheit.unwrap_or(false));

                        metric_co2.set(readings.c02 as i64);
                        metric_temp_f.set(readings.temp.f_float());
                        metric_temp_c.set(readings.temp.c_float());
                        metric_relative_humidity.set(readings.humidity as i64);
                        metric_preasure.set(readings.preasure as f64 / 10.0);
                        metric_bat.set(readings.bat as i64);

                        tokio::time::sleep(Duration::from_secs(cfg.stream_freq.unwrap_or(30)))
                            .await;
                    }
                }
            };
        } else {
            let readings = endpoint.read().await.unwrap();
            println!("{}", readings);
        }
    });
}

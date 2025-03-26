use std::{env, fs, net::ToSocketAddrs, time::Duration};

use anyhow::Result;
use bluer::{agent::Agent, AdapterEvent, Address};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use prometheus::{register, Gauge, IntGauge};
use serde::{de::DeserializeOwned, Deserialize};

use aranet::{bluetooth::*, metric};

#[derive(Deserialize)]
pub struct Cfg {
    /// EX: hci0
    pub adapter: String,
    /// FORMAT: ED:12:89:6C:08:37
    pub mac: String,
    pub fahrenheit: Option<bool>,
    // Seconds
    pub stream_freq: Option<u64>,
    pub prometheus_address: Option<String>,
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
        let mac_array = str_mac_to_array(&cfg.mac).unwrap();

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

        let cfg_address = Address::new(mac_array);

        if let Ok(mut stream) = adapter.discover_devices().await {
            eprintln!("Discovering...");
            while let Some(event) = stream.next().await {
                eprintln!("{event:?}");
                match event {
                    AdapterEvent::DeviceAdded(device) => {
                        if device == cfg_address {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        };

        let dev = adapter.device(cfg_address).unwrap();

        dev.connect().await.unwrap();

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
                    adapter.device_addresses().await
                );
                return;
            }
        }

        // while dev.is_services_resolved().await == Ok(false) {
        // println!("Waiting for service resolve...");
        // tokio::time::sleep(Duration::from_millis(20)).await;
        // }

        let endpoint = map_device_endpoints(&dev).await.unwrap();

        if let Some(cmd) = cli.cmd {
            match cmd {
                Cmd::Oneline => {
                    let readings = endpoint.read().await.unwrap();
                    readings.print_oneline(cfg.fahrenheit.unwrap_or(false));
                }
                Cmd::StreamingOneline => {
                    if !dev.is_connected().await.unwrap() {
                        dev.connect().await.unwrap();
                    }
                    let readings = endpoint.read().await.unwrap();
                    readings.print_oneline(cfg.fahrenheit.unwrap_or(false));
                    tokio::time::sleep(Duration::from_secs(cfg.stream_freq.unwrap_or(30))).await;
                }
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

use anyhow::{anyhow, Result};
use arrayref::{array_ref, array_refs};
use bluer::{
    agent::{self, Agent, ReqResult},
    gatt::remote::Characteristic,
    AdapterEvent, Address, Device,
};
use futures::StreamExt;
use prometheus::{register, Gauge, IntGauge};
use serde::Deserialize;
use std::{
    env,
    fmt::{Display, Error as FmtError, Formatter},
    fs,
    future::Future,
    io::{Read, Write},
    net::ToSocketAddrs,
    pin::Pin,
    process::{Command, Stdio},
    result::Result as StdResult,
    time::Duration,
};
use uuid::Uuid;

mod metric;

fn get_passkey(req: agent::RequestPasskey) -> Pin<Box<dyn Future<Output = ReqResult<u32>> + Send>> {
    println!("Passkey requested");
    Box::pin(async move {
        println!(
            "Device requesting PIN code for {} on {}",
            req.device, req.adapter
        );

        let mut child = Command::new("pinentry-qt")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();

        // Write commands to pinentry
        stdin.write_all(b"SETTITLE Bluetooth PIN\n").unwrap();
        stdin
            .write_all(format!("SETDESC Enter PIN for device {}\n", req.device).as_bytes())
            .unwrap();
        stdin.write_all(b"GETPIN\n").unwrap();

        // Read response line by line
        let mut pin = String::new();
        let mut buf = [0u8; 1024];

        loop {
            let n = stdout.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }

            let response = String::from_utf8_lossy(&buf[..n]);
            if response.starts_with("D ") {
                pin = response[2..8].trim().to_string();
                break;
            }
        }

        child.kill().unwrap();

        dbg!(&pin);

        if pin.is_empty() {
            Err(bluer::agent::ReqError::Rejected)
        } else {
            Ok(pin.parse::<u32>().unwrap())
        }
    })
}

#[derive(Debug)]
/// Internally represented as 20 * $temp_in_c
struct Temp(u16);

#[allow(dead_code)]
impl Temp {
    /// gives 10 * Int C
    fn c(&self) -> u16 {
        self.0 / 2
    }
    /// gives 10 * Int F
    fn f(&self) -> u16 {
        (((self.0 * 9) / 5) / 2) + 320
    }
    fn c_float(&self) -> f64 {
        self.0 as f64 / 20.0
    }
    fn f_float(&self) -> f64 {
        (self.0 as f64 / 20.0) * (9.0 / 5.0) + 32.0
    }
}

impl Display for Temp {
    fn fmt(&self, f: &mut Formatter) -> StdResult<(), FmtError> {
        write!(f, "{}", self.0)?;
        Ok(())
    }
}

#[derive(Debug)]
struct CurrentReading {
    c02: u16,
    temp: Temp,
    preasure: u16,
    humidity: u8,
    bat: u8,
    status: u8,
}

impl CurrentReading {
    fn print_oneline(&self, fahrenheit: bool) {
        println!(
            "{}ppm {:.2}°{} {}% {}hPa",
            self.c02,
            if fahrenheit {
                self.temp.f_float()
            } else {
                self.temp.c_float()
            },
            if fahrenheit { "F" } else { "C" },
            self.humidity,
            self.preasure / 10
        );
    }
}

impl Display for CurrentReading {
    fn fmt(&self, f: &mut Formatter) -> StdResult<(), FmtError> {
        writeln!(f, "CO2:         {}", self.c02)?;
        writeln!(
            f,
            "Temperature: {:.2}°C / {:.2}°F",
            self.temp.c_float(),
            self.temp.f_float(),
        )?;
        writeln!(f, "Humidity:    {}", self.humidity)?;
        writeln!(f, "Presure:     {}", self.preasure)?;
        writeln!(f, "Battery:     {}", self.bat)?;
        write!(f, "Status:      {}", self.status)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct EndPoints {
    battery_level: Option<Characteristic>,
    sensor_state: Option<Characteristic>,
    cmd: Option<Characteristic>,
    calibration_data: Option<Characteristic>,
    current_readings: Option<Characteristic>,
    current_readings_ar2: Option<Characteristic>,
    total_readings: Option<Characteristic>,
    interval: Option<Characteristic>,
    history_readings_v1: Option<Characteristic>,
    seconds_since_update: Option<Characteristic>,
    history_readings_v2: Option<Characteristic>,
    current_readings_det: Option<Characteristic>,
    current_readings_a: Option<Characteristic>,
    current_readings_a_ar2: Option<Characteristic>,
}

impl EndPoints {
    async fn read(&self) -> Result<CurrentReading> {
        if let Some(c) = &self.current_readings {
            let bytes = c.read().await?;
            let src = array_ref![bytes, 0, 9];
            let (c02, temp, preasure, humidity, bat, status) = array_refs![src, 2, 2, 2, 1, 1, 1];
            return Ok(CurrentReading {
                c02: u16::from_le_bytes(*c02),
                temp: Temp(u16::from_le_bytes(*temp)),
                preasure: u16::from_le_bytes(*preasure),
                humidity: humidity[0],
                bat: bat[0],
                status: status[0],
            });
        }
        Err(anyhow!("Failed"))
    }
}

#[allow(dead_code)]
#[rustfmt::skip]
async fn map_device_endpoints(dev: &Device) -> Result<EndPoints> {
    const SERVICE_GAP: Uuid = Uuid::from_u128(0x0000180000001000800000805f9b34fb);
    const CHAR_DEVICE_NAME: Uuid = Uuid::from_u128(0x00002a000001000800000805f9b34fb);
    const CHAR_APPEARANCE: Uuid = Uuid::from_u128(0x00002a010001000800000805f9b34fb);

    const SERVICE_DIS: Uuid = Uuid::from_u128(0x0000180a00001000800000805f9b34fb);
    const CHAR_SYSTEM_ID: Uuid = Uuid::from_u128(0x00002a230001000800000805f9b34fb);
    const CHAR_MODEL_NUMBER: Uuid = Uuid::from_u128(0x00002a240001000800000805f9b34fb);
    const CHAR_SERIAL_NO: Uuid = Uuid::from_u128(0x00002a250001000800000805f9b34fb);
    const CHAR_SW_REV: Uuid = Uuid::from_u128(0x00002a260001000800000805f9b34fb);
    const CHAR_HW_REV: Uuid = Uuid::from_u128(0x00002a270001000800000805f9b34fb);
    const CHAR_SW_REV_FACTORY: Uuid = Uuid::from_u128(0x00002a280001000800000805f9b34fb);
    const CHAR_MANUFACTURER_NAME: Uuid = Uuid::from_u128(0x00002a290001000800000805f9b34fb);

    const SERVICE_BATTTERY: Uuid = Uuid::from_u128(0x0000180f00001000800000805f9b34fb); // v1.2.0 and later
    const CHAR_BATTERY_LEVEL: Uuid = Uuid::from_u128(0x00002a190001000800000805f9b34fb);

    const SERVICE_SAF_TEHNIKA: Uuid = Uuid::from_u128(0x0000fce000001000800000805f9b34fb); // v1.2.0 and later
    const CHAR_SENSOR_STATE: Uuid = Uuid::from_u128(0xf0cd140195da4f4b9ac8aa55d312af0c);
    const CHAR_CMD: Uuid = Uuid::from_u128(0xf0cd140295da4f4b9ac8aa55d312af0c);
    const CHAR_CALIBRATION_DATA: Uuid = Uuid::from_u128(0xf0cd150295da4f4b9ac8aa55d312af0c);
    const CHAR_CURRENT_READINGS: Uuid = Uuid::from_u128(0xf0cd150395da4f4b9ac8aa55d312af0c);
    const CHAR_CURRENT_READINGS_AR2: Uuid = Uuid::from_u128(0xf0cd150495da4f4b9ac8aa55d312af0c); // Aranet2 Only
    const CHAR_TOTAL_READINGS: Uuid = Uuid::from_u128(0xf0cd200195da4f4b9ac8aa55d312af0c);
    const CHAR_INTERVAL: Uuid = Uuid::from_u128(0xf0cd200295da4f4b9ac8aa55d312af0c);
    const CHAR_HISTORY_READINGS_V1: Uuid = Uuid::from_u128(0xf0cd200395da4f4b9ac8aa55d312af0c);
    const CHAR_SECONDS_SINCE_UPDATE: Uuid = Uuid::from_u128(0xf0cd200495da4f4b9ac8aa55d312af0c);
    const CHAR_HISTORY_READINGS_V2: Uuid = Uuid::from_u128(0xf0cd200595da4f4b9ac8aa55d312af0c);
    const CHAR_CURRENT_READINGS_DET: Uuid = Uuid::from_u128(0xf0cd300195da4f4b9ac8aa55d312af0c);
    const CHAR_CURRENT_READINGS_A: Uuid = Uuid::from_u128(0xf0cd300295da4f4b9ac8aa55d312af0c);
    const CHAR_CURRENT_READINGS_A_AR2: Uuid = Uuid::from_u128(0xf0cd300395da4f4b9ac8aa55d312af0c); // Aranet2 Only

    const SERVICE_NORDIC_SEMICONDUCTOR: Uuid = Uuid::from_u128(0x0000fe5900001000800000805f9b34fb);
    const CHAR_SECURE_DFU: Uuid = Uuid::from_u128(0x8ec90003f3154f609fb8838830daea50);

    let mut endpoint = EndPoints::default();

    for service in dev.services().await? {
        let service_uuid = service.uuid().await?;
        for characteristic in service.characteristics().await? {
            let characteristic_uuid = characteristic.uuid().await?;
            match (service_uuid, characteristic_uuid) {
                (SERVICE_BATTTERY, CHAR_BATTERY_LEVEL) => endpoint.battery_level = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_SENSOR_STATE) => endpoint.sensor_state = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CMD) => endpoint.cmd = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CALIBRATION_DATA) => endpoint.calibration_data = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CURRENT_READINGS) => endpoint.current_readings = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CURRENT_READINGS_AR2) => endpoint.current_readings_ar2 = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_TOTAL_READINGS) => endpoint.total_readings = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_INTERVAL) => endpoint.interval = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_HISTORY_READINGS_V1) => endpoint.history_readings_v1 = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_SECONDS_SINCE_UPDATE) => endpoint.seconds_since_update = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_HISTORY_READINGS_V2) => endpoint.history_readings_v2 = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CURRENT_READINGS_DET) => endpoint.current_readings_det = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CURRENT_READINGS_A) => endpoint.current_readings_a = Some(characteristic),
                (SERVICE_SAF_TEHNIKA, CHAR_CURRENT_READINGS_A_AR2) => endpoint.current_readings_a_ar2 = Some(characteristic),
                (_, _) => {},
            }
        }
    }

    Ok(endpoint)
}

fn str_mac_to_array(mac_address: &str) -> Result<[u8; 6]> {
    let mut mac_array = [0u8; 6];

    for (i, part) in mac_address.split(':').enumerate() {
        mac_array[i] = u8::from_str_radix(part, 16)?;
    }

    Ok(mac_array)
}

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

pub fn try_get_cfg() -> Result<Cfg> {
    let home = env::var("HOME")?;
    let content = fs::read_to_string(format!("{home}/.config/aranet/config.toml"))?;
    let config = toml::from_str::<Cfg>(&content)?;
    Ok(config)
}

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP_MSG: &str = "  By default will do a single multi-line print of the current readings

OPTIONS
      --help              Print this message
      --oneline           One-line partial print
      --streaming-oneline Print one-line updates at cfg.stream_freq in seconds
      --service           Host prometheus endpoint, see config, also prints
      --version           Print version
";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cfg = try_get_cfg().unwrap();
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
            return Ok(());
        }
    }

    // while dev.is_services_resolved().await == Ok(false) {
    // println!("Waiting for service resolve...");
    // tokio::time::sleep(Duration::from_millis(20)).await;
    // }

    let endpoint = map_device_endpoints(&dev).await.unwrap();

    if let Some(arg) = env::args().skip(1).next() {
        match arg.as_str() {
            "--help" => {
                print!("USAGE: {PKG_NAME} [OPTIONS]\n{HELP_MSG}");
            }
            "--version" => {
                println!("{PKG_NAME}: v{VERSION}");
            }
            "--oneline" => {
                let readings = endpoint.read().await.unwrap();
                readings.print_oneline(cfg.fahrenheit.unwrap_or(false));
            }
            "--streaming-oneline" => loop {
                if !dev.is_connected().await.unwrap() {
                    dev.connect().await.unwrap();
                }
                let readings = endpoint.read().await.unwrap();
                readings.print_oneline(cfg.fahrenheit.unwrap_or(false));
                tokio::time::sleep(Duration::from_secs(cfg.stream_freq.unwrap_or(30))).await;
            },
            "--service" => {
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

                let metric_temp_c = Gauge::new("aranet_temp_celsius", "Temp in Celsius").unwrap();
                register(Box::new(metric_temp_c.clone()))
                    .map(|()| metric_temp_c.clone())
                    .unwrap();

                let metric_relative_humidity =
                    IntGauge::new("aranet_relative_humidity", "Relative humidity %").unwrap();
                register(Box::new(metric_relative_humidity.clone()))
                    .map(|()| metric_relative_humidity.clone())
                    .unwrap();

                let metric_preasure = Gauge::new("aranet_preasure", "Air preasure in hPa").unwrap();
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

                    tokio::time::sleep(Duration::from_secs(cfg.stream_freq.unwrap_or(30))).await;
                }
            }
            _ => todo!(),
        }
    } else {
        let readings = endpoint.read().await.unwrap();
        println!("{}", readings);
    }

    Ok(())
}

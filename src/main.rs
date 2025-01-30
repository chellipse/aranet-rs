use anyhow::Result;
use bluer::{
    agent::{self, Agent, ReqResult},
    Address, Device,
};
use std::{
    future::Future,
    io::{Read, Write},
    pin::Pin,
    process::{Command, Stdio},
};
use uuid::Uuid;

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

#[allow(dead_code)]
#[rustfmt::skip]
async fn map_device_endpoints(dev: &Device) -> Result<()> {
    // SAF Tehnika Service Characteristics (Aranet2 has different readings characteristic uuids)
    const CHARACTERISTIC_SENSOR_STATE: Uuid = Uuid::from_u128(0xf0cd140195da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CMD: Uuid = Uuid::from_u128(0xf0cd140295da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CALIBRATION_DATA: Uuid = Uuid::from_u128(0xf0cd150295da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CURRENT_READINGS: Uuid = Uuid::from_u128(0xf0cd150395da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CURRENT_READINGS_AR2: Uuid = Uuid::from_u128(0xf0cd150495da4f4b9ac8aa55d312af0c); // Aranet2 Only
    const CHARACTERISTIC_TOTAL_READINGS: Uuid = Uuid::from_u128(0xf0cd200195da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_INTERVAL: Uuid = Uuid::from_u128(0xf0cd200295da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_HISTORY_READINGS_V1: Uuid = Uuid::from_u128(0xf0cd200395da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_SECONDS_SINCE_UPDATE: Uuid = Uuid::from_u128(0xf0cd200495da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_HISTORY_READINGS_V2: Uuid = Uuid::from_u128(0xf0cd200595da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CURRENT_READINGS_DET: Uuid = Uuid::from_u128(0xf0cd300195da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CURRENT_READINGS_A: Uuid = Uuid::from_u128(0xf0cd300295da4f4b9ac8aa55d312af0c);
    const CHARACTERISTIC_CURRENT_READINGS_A_AR2: Uuid = Uuid::from_u128(0xf0cd300395da4f4b9ac8aa55d312af0c); // Aranet2 Only

    for service in dev.services().await? {
        println!(
            "    Service: {} HexId:{:x}",
            service.id(),
            service.uuid().await?
        );
        dbg!(&service.all_properties().await);
        for characteristic in service.characteristics().await? {
            print!(
                "        Char: Id: {} Hex: {:x} -> ",
                characteristic.id(),
                characteristic.uuid().await?
            );
            println!("{:?}", characteristic.cached_value().await);
            dbg!(&characteristic.all_properties().await);
            // println!("{:?}", characteristic.read().await);
        }
    }

    Ok(())
}

fn str_mac_to_array(mac_address: &str) -> Result<[u8; 6]> {
    let mut mac_array = [0u8; 6];

    for (i, part) in mac_address.split(':').enumerate() {
        mac_array[i] = u8::from_str_radix(part, 16)?;
    }

    Ok(mac_array)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let mac_array = str_mac_to_array("ED:12:89:6C:08:37")?;

    // Create a new Bluetooth session
    let session = bluer::Session::new().await?;

    let agent = Agent {
        request_passkey: Some(Box::new(get_passkey)),
        ..Default::default()
    };

    let _agent = session.register_agent(agent).await?;

    // Get the adapter and power it on
    let adapter = session.adapter("hci0")?;
    adapter.set_powered(true).await?;

    let dev = adapter.device(Address::new(mac_array))?;
    println!("Found device: {:?}", dev);

    if !dev.is_paired().await? {
        println!("Device is not paired. Attempting to pair...");

        match dev.pair().await {
            Ok(_) => println!("Pairing successful!"),
            Err(err) => eprintln!("Pairing failed: {:?}", err),
        }
    } else {
        println!("Device is already paired.");
    }

    dev.connect().await?;

    while dev.is_services_resolved().await == Ok(false) {
        println!("Waiting for service resolve...");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    map_device_endpoints(&dev).await?;

    Ok(())
}

use bluer::{
    agent::{self, Agent, ReqResult},
    Address,
};
use futures::StreamExt;
use std::{
    future::Future,
    io::{Read, Write},
    pin::Pin,
    process::{Command, Stdio},
};

fn get_pin(req: agent::RequestPinCode) -> Pin<Box<dyn Future<Output = ReqResult<String>> + Send>> {
    println!("PIN code requested");
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

        if pin.is_empty() {
            Err(bluer::agent::ReqError::Rejected)
        } else {
            Ok(pin)
        }
    })
}

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

#[tokio::main(flavor = "multi_thread")]
async fn main() -> bluer::Result<()> {
    let mac_address = "ED:12:89:6C:08:37";
    let mut mac_array = [0u8; 6];

    // Convert MAC address string to [u8; 6]
    for (i, part) in mac_address.split(':').enumerate() {
        mac_array[i] = u8::from_str_radix(part, 16).expect("Invalid hex value");
    }

    // Create a new Bluetooth session
    let session = bluer::Session::new().await?;

    let agent = Agent {
        request_pin_code: Some(Box::new(get_pin)),
        request_passkey: Some(Box::new(get_passkey)),
        display_pin_code: Some(Box::new(|req| {
            println!("Pincode display: {}", req.pincode);
            Box::pin(async move { Ok(()) })
        })),
        display_passkey: Some(Box::new(|req| {
            println!("Passkey display: {}", req.passkey);
            Box::pin(async move { Ok(()) })
        })),
        ..Default::default()
    };

    let agent = session.register_agent(agent).await?;
    let _guard = agent;

    let adapter_names = session.adapter_names().await?;

    for adapter_name in adapter_names {
        println!("Bluetooth adapter {}:", &adapter_name);

        // Get the adapter and power it on
        let adapter = session.adapter(&adapter_name)?;
        adapter.set_powered(true).await?;

        // Start device discovery
        let _stream = adapter.discover_devices().await?;

        // Spawn a task to handle device discovery events
        tokio::spawn(async move {
            let mut stream = _stream;
            while let Some(item) = stream.next().await {
                println!("Discovered device: {:?}", item);
            }
        });

        // Wait for a moment to allow device discovery
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Get the device by its MAC address
        let dev = adapter.device(Address::new(mac_array))?;
        println!("Found device: {:?}", dev);

        // Handle pairing
        if !dev.is_paired().await? {
            println!("Device is not paired. Attempting to pair...");

            // Start pairing
            match dev.pair().await {
                Ok(_) => println!("Pairing successful!"),
                Err(err) => eprintln!("Pairing failed: {:?}", err),
            }
        } else {
            println!("Device is already paired.");
        }
    }

    Ok(())
}

use anyhow::{anyhow, Result};
use camloc_common::{get_from_stdin, yes_no_choice};
use camloc_server::{
    compass::{serial::SerialCompass, Compass},
    extrapolations::{Extrapolation, LinearExtrapolation},
    service::{LocationService, Subscriber},
    PlacedCamera, TimedPosition,
};
use std::{
    io::{stderr, Write},
    net::SocketAddr,
    time::Duration,
};
use tokio_serial::{SerialPortBuilderExt, SerialPortType};

fn main() {
    if let Err(e) = run() {
        println!("Exiting with error: {e}");
    } else {
        println!("Exiting test...");
    }
}

fn get_compass() -> Result<Option<Box<dyn Compass + Send>>> {
    if !yes_no_choice("Do you want to use a microbit compass?", false) {
        return Ok(None);
    }

    let Ok(devices) = tokio_serial::available_ports() else {
        println!("  Couldn't get available serial devices");
        return Ok(None);
    };
    if devices.is_empty() {
        println!("  No serial devices available");
        return Ok(None);
    }

    println!("  Available serial devices:");

    for (i, d) in devices.iter().enumerate() {
        println!(
            "  {i:<3}{} | {}",
            d.port_name,
            match &d.port_type {
                SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                SerialPortType::Unknown => "unknown".to_string(),
                SerialPortType::UsbPort(info) => {
                    let mut s = "USB".to_string();
                    if let Some(m) = &info.manufacturer {
                        s.push_str(" | ");
                        s.push_str(m);
                    }
                    if let Some(m) = &info.product {
                        s.push_str(" | ");
                        s.push_str(m);
                    }

                    s
                }
                SerialPortType::PciPort => "PCI".to_string(),
            }
        );
    }

    let d = &devices[get_from_stdin::<usize>("  Enter index: ").map_err(anyhow::Error::msg)?];
    let baud_rate = get_from_stdin("  Enter baud rate (115200hz): ").unwrap_or(115200);
    let offset = get_from_stdin("  Enter compass offset in degrees (0 deg): ").unwrap_or(0u8);

    let p = tokio_serial::new(&d.port_name, baud_rate)
        .open_native_async()
        .map(|p| SerialCompass::start(p, offset as f64));

    if let Ok(Ok(p)) = p {
        Ok(Some(Box::new(p)))
    } else {
        Err(anyhow!("Couldn't open serial port"))
    }
}

fn run() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;

    let compass = get_compass()?;

    let location_service = LocationService::start(
        runtime.handle().clone(),
        Some(Extrapolation::new::<LinearExtrapolation>(
            Duration::from_millis(500),
        )),
        // no_extrapolation!(),
        camloc_common::hosts::constants::MAIN_PORT,
        compass, // no_compass!(),
        Duration::from_millis(500),
    )?;

    location_service.subscribe(Subscriber::Connection(on_connect));

    location_service.subscribe(Subscriber::Disconnection(on_disconnect));

    let ctrlc_task = runtime.spawn(tokio::signal::ctrl_c());

    if yes_no_choice("Subscription or query mode?", true) {
        location_service.subscribe(Subscriber::Position(on_position));
    } else {
        loop {
            if let Some(p) = location_service.get_position() {
                on_position(p)?;
            } else {
                println!("Couldn't get position");
            }

            if ctrlc_task.is_finished() {
                break;
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    if let Err(_) | Ok(Err(_)) = runtime.block_on(ctrlc_task) {
        return Err(anyhow!("Something failed in the ctrl+c channel"));
    }

    Ok(())
}

fn on_position(p: TimedPosition) -> Result<()> {
    println!("{p}");

    let mut se = stderr().lock();

    se.write_all(&[0])?;

    se.write_all(p.position.x.to_be_bytes().as_slice())?;
    se.write_all(p.position.y.to_be_bytes().as_slice())?;
    se.write_all(p.position.rotation.to_be_bytes().as_slice())?;

    se.flush()?;

    Ok(())
}

fn on_connect(address: SocketAddr, camera: PlacedCamera) -> Result<()> {
    let address = address.to_string();
    let mut se = stderr().lock();

    se.write_all(&[1])?;
    se.write_all((address.len() as u16).to_be_bytes().as_slice())?;
    se.write_all(address.as_bytes())?;

    se.write_all((camera.position.x).to_be_bytes().as_slice())?;
    se.write_all((camera.position.y).to_be_bytes().as_slice())?;
    se.write_all((camera.position.rotation).to_be_bytes().as_slice())?;

    se.write_all((camera.fov).to_be_bytes().as_slice())?;

    se.flush()?;

    Ok(())
}

fn on_disconnect(address: SocketAddr, _: PlacedCamera) -> Result<()> {
    let address = address.to_string();
    println!("Camera disconnected from {address}");

    let mut se = stderr().lock();

    se.write_all(&[2])?;
    se.write_all((address.len() as u16).to_be_bytes().as_slice())?;
    se.write_all(address.as_bytes())?;

    se.flush()?;

    Ok(())
}

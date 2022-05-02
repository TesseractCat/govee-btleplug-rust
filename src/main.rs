use btleplug::api::{
    bleuuid::uuid_from_u16,
    Central, CentralEvent,
    Manager as _, Peripheral as _, Characteristic,
    ScanFilter, WriteType
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use uuid::Uuid;

use futures::stream::StreamExt;
use tokio::time;

use std::error::Error;
use std::time::Duration;
use std::sync::Arc;

use warp::Filter;
use colors_transform::{Rgb, Color};

fn construct_message(id: u8, data: Vec<u8>) -> Vec<u8> {
    let xor: u8 = data.iter().fold(id, |a, b| a ^ b);
    let mut result: Vec<u8> = data.clone();
    result.resize(18, 0);
    result.push(xor);
    result.insert(0, id);
    assert_eq!(result.len(), 20);

    result
}

fn light_message(r: u8, g: u8, b: u8) -> Vec<u8> {
    construct_message(0x33, vec![
        0x05, 0x15, 0x01, // Identifier
        r, g, b, // Colors (R, G, B)
        0x00, 0x00, 0x00, 0x00, 0x00,
        0xFF, 0x0F, // Segments
    ])
}

async fn send_message(light: &Peripheral, characteristic: &Characteristic, message: Vec<u8>) {
    println!("Sending message: {:02X?}", message);

    light.write(characteristic, &message, WriteType::WithoutResponse).await;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Searching...");

    let manager = Manager::new().await.unwrap();

    let adapters = manager.adapters().await?;
    let central = adapters.into_iter().nth(0).unwrap();
    let mut events = central.events().await?;

    central.start_scan(ScanFilter::default()).await?;

    let mut light_search: Option<Peripheral> = None;
    while let Some(event) = events.next().await {
        if let CentralEvent::DeviceDiscovered(id) = event {
            let p = central.peripheral(&id).await?;
            let addr: String = p.address().to_string_no_delim();
            if addr == "d7313030344c" {
                light_search = Some(p);
                println!("Found light!");
                break;
            }
        }
    }

    let light: Arc<Peripheral> = Arc::new(light_search.unwrap());

    println!("Connecting...");
    light.connect().await?;
    println!("Connected! Identifying characteristics...");
    light.discover_services().await?;

    let chars = light.characteristics();

    let mut light_characteristic: Option<Arc<Characteristic>> = None;
    for c in chars.iter() {
        if c.uuid.to_hyphenated().to_string() == "00010203-0405-0607-0809-0a0b0c0d2b11" {
            light_characteristic = Some(Arc::new(c.clone()));
            println!("Identified light characteristic!");
        }
    }

    {
        println!("Starting keep alive loop...");

        let light = light.clone();
        let light_characteristic = light_characteristic.clone();

        tokio::spawn(async move {
            loop {
                time::sleep(Duration::from_secs(2)).await;
                let keep_alive = construct_message(0xAA, vec![0x33]);
                send_message(&light, light_characteristic.as_ref().unwrap(), keep_alive).await;
            }
        });
    }

    {
        println!("Starting warp...");

        let light = light.clone();
        let light_characteristic = light_characteristic.clone();

        let path = warp::path!("light" / String).then(move |hex: String| {
            let light = light.clone();
            let light_characteristic = light_characteristic.clone();

            async move {
                let color = Rgb::from_hex_str(&hex).unwrap();
                println!("Changing color to {:?}", color);

                send_message(&light, light_characteristic.as_ref().unwrap(),
                            light_message(color.get_red() as u8, color.get_green() as u8, color.get_blue() as u8)
                ).await;

                Ok("Color set")
            }
        });
        warp::serve(path).run(([127, 0, 0, 1], 3030)).await;
    }

    light.disconnect().await?;

    println!("Done!");
    Ok(())
}

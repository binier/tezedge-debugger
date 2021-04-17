// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{sync::{Arc, atomic::{Ordering, AtomicBool}}, fs::File, io::Write};
    use serde::{Serialize, ser};
    use bpf_memprof::{Client, Event, EventKind};
    use tezedge_memprof::ProcessMap;

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    #[derive(Serialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum StackEntry {
        Unknown,
        Symbol {
            filename: String,
            address: Address,
        },
    }

    #[derive(Serialize)]
    struct Record {
        event: EventKind,
        stack: Vec<StackEntry>,
    }

    struct Address(usize);

    impl Serialize for Address {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            serializer.serialize_str(&format!("{:016x}", self.0))
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = running.clone();
        ctrlc::set_handler(move || running.store(false, Ordering::Relaxed))?;
    }

    let (mut client, mut rb) = Client::new("/tmp/bpf-memprof.sock")?;
    client.send_command("dummy command")?;

    //let mut cached_map = None;
    let mut history = vec![];
    while running.load(Ordering::Relaxed) {
        let events = rb.read_blocking::<Event>(&running)?;
        for event in events {
            /*let map = match &cached_map {
                None => {
                    let map = ProcessMap::new(event.pid)?;
                    cached_map = Some(map);
                    cached_map.as_ref().unwrap()
                },
                Some(map) => map,
            };*/
            let map = ProcessMap::new(event.pid)?;

            let mut record = Record {
                event: event.kind,
                stack: vec![],
            };

            match event.stack {
                Ok(stack) => {
                    for ip in stack.ips() {
                        match map.find(*ip) {
                            None => {
                                record.stack.push(StackEntry::Unknown);
                            },
                            Some((path, addr)) => {
                                let entry = StackEntry::Symbol {
                                    filename: format!("{:?}", path),
                                    address: Address(addr),
                                };
                                record.stack.push(entry);
                            },
                        }
                    }
                },
                Err(code) => {
                    log::error!("failed to receive stack, error code: {}", code);
                },
            }

            history.push(record);
            if history.len() & 0xfff == 0 {
                log::info!("processed: {} events", history.len());
            }
        }
    }

    let history = serde_json::to_string(&history)?;
    File::create("target/report.json")?.write_all(history.as_bytes())?;

    Ok(())
}

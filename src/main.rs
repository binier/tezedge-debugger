#![allow(dead_code)]

mod configuration;
mod actors;
mod network;

use failure::{Error, Fail};
use riker::actors::*;

use pnet::{packet::{
    Packet as _,
    tcp::TcpPacket,
    ipv4::Ipv4Packet,
    ipv6::Ipv6Packet,
    ethernet::{EthernetPacket, EtherTypes},
    ip::IpNextHeaderProtocols,
}, datalink};

use crate::{
    actors::prelude::*,
    configuration::AppConfig,
};

#[derive(Debug, Fail)]
enum AppError {
    #[fail(display = "no valid network interface found")]
    NoNetworkInterface,
    #[fail(display = "only ethernet channels supported for now")]
    UnsupportedNetworkChannelType,
    #[fail(display = "encountered io error: {}", _0)]
    IOError(std::io::Error),
    #[fail(display = "received invalid packet")]
    InvalidPacket,
}

fn main() -> Result<(), Error> {
    // -- Initialize logger
    simple_logger::init()?;

    // -- Load basic arguments + TODO: Add more arguments and more options ways to pass arguments
    let app_config = AppConfig::from_env();
    log::info!("Loaded arguments from CLI");
    let identity = app_config.load_identity()?;
    log::info!("Loaded identity file from '{}'", app_config.identity_file);

    // -- Start Actor system
    let system = ActorSystem::new()?;
    let orchestrator = system.actor_of(Props::new_args(PacketOrchestrator::new, PacketOrchestratorArgs {
        local_identity: identity.clone()
    }), "packet_orchestrator")?;

    // -- Acquire raw network interface
    let interface = datalink::interfaces().into_iter()
        .filter(|x| x.is_up() && x.is_broadcast() && x.is_multicast())
        .next()
        .ok_or(AppError::NoNetworkInterface)?;
    let (_, mut rx) = datalink::channel(&interface, Default::default())
        .map_err(|err| AppError::IOError(err))
        .and_then(|chan| match chan {
            datalink::Channel::Ethernet(tx, rx) => Ok((tx, rx)),
            _ => Err(AppError::UnsupportedNetworkChannelType)
        })?;

    log::info!("Starting to analyze traffic on port {}", app_config.port);

    loop {
        let packet = EthernetPacket::new(rx.next()?).unwrap();
        let (payload, protocol) = match packet.get_ethertype() {
            EtherTypes::Ipv4 => {
                let header = Ipv4Packet::new(packet.payload()).unwrap();
                (header.payload().to_vec(), header.get_next_level_protocol())
            }
            EtherTypes::Ipv6 => {
                let header = Ipv6Packet::new(packet.payload()).unwrap();
                ((header.payload()).to_vec(), header.get_next_header())
            }
            _ => continue,
        };

        if protocol == IpNextHeaderProtocols::Tcp {
            let tcp = TcpPacket::new(&payload).unwrap();
            let (source, dest) = (tcp.get_source(), tcp.get_destination());
            if app_config.port == dest {
                orchestrator.send_msg(Packet::outgoing(source, tcp.payload().to_vec()), None);
            } else if app_config.port == source {
                orchestrator.send_msg(Packet::incoming(dest, tcp.payload().to_vec()), None);
            } else {
                continue;
            }
        }
    }
}
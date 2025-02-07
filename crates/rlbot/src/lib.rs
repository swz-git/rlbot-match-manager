use std::{
    io::{Read, Write},
    net::{AddrParseError, SocketAddr, TcpStream},
    str::FromStr,
};

use rlbot_flat::planus::{self, ReadAsRoot};
use thiserror::Error;

pub mod agents;
pub mod util;

#[cfg(feature = "glam")]
pub use rlbot_flat::glam;

pub use rlbot_flat::flat;

use flat::*;

#[derive(Error, Debug)]
pub enum PacketParseError {
    #[error("Invalid data type: {0}")]
    InvalidDataType(u16),
    #[error("Unpacking flatbuffer failed")]
    InvalidFlatbuffer(#[from] planus::Error),
}

#[derive(Error, Debug)]
pub enum RLBotError {
    #[error("Connection to RLBot failed")]
    Connection(#[from] std::io::Error),
    #[error("Parsing packet failed")]
    PacketParseError(#[from] PacketParseError),
    #[error("Invalid address, cannot parse")]
    InvalidAddrError(#[from] AddrParseError),
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Packet {
    None,
    GamePacket(GamePacket),
    FieldInfo(FieldInfo),
    StartCommand(StartCommand),
    MatchConfiguration(MatchConfiguration),
    PlayerInput(PlayerInput),
    DesiredGameState(DesiredGameState),
    RenderGroup(RenderGroup),
    RemoveRenderGroup(RemoveRenderGroup),
    MatchComm(MatchComm),
    BallPrediction(BallPrediction),
    ConnectionSettings(ConnectionSettings),
    StopCommand(StopCommand),
    SetLoadout(SetLoadout),
    InitComplete,
    ControllableTeamInfo(ControllableTeamInfo),
}

macro_rules! gen_impl_from_flat_packet {
    ($($x:ident),+) => {
        $(
            impl From<$x> for Packet {
                fn from(x: $x) -> Self {
                    Packet::$x(x)
                }
            }
        )+
    };
}

gen_impl_from_flat_packet!(
    // None
    GamePacket,
    FieldInfo,
    StartCommand,
    MatchConfiguration,
    PlayerInput,
    DesiredGameState,
    RenderGroup,
    RemoveRenderGroup,
    MatchComm,
    BallPrediction,
    ConnectionSettings,
    StopCommand,
    SetLoadout,
    // InitComplete
    ControllableTeamInfo
);

impl Packet {
    pub const fn data_type(&self) -> u16 {
        match *self {
            Packet::None => 0,
            Packet::GamePacket(_) => 1,
            Packet::FieldInfo(_) => 2,
            Packet::StartCommand(_) => 3,
            Packet::MatchConfiguration(_) => 4,
            Packet::PlayerInput(_) => 5,
            Packet::DesiredGameState(_) => 6,
            Packet::RenderGroup(_) => 7,
            Packet::RemoveRenderGroup(_) => 8,
            Packet::MatchComm(_) => 9,
            Packet::BallPrediction(_) => 10,
            Packet::ConnectionSettings(_) => 11,
            Packet::StopCommand(_) => 12,
            Packet::SetLoadout(_) => 13,
            Packet::InitComplete => 14,
            Packet::ControllableTeamInfo(_) => 15,
        }
    }

    pub fn build(self, builder: &mut planus::Builder) -> Vec<u8> {
        // TODO: make this mess nicer
        macro_rules! p {
            ($x:ident) => {{
                builder.clear();
                builder.finish($x, None).to_vec()
            }};
        }

        match self {
            Packet::None => Vec::new(),
            Packet::GamePacket(x) => p!(x),
            Packet::FieldInfo(x) => p!(x),
            Packet::StartCommand(x) => p!(x),
            Packet::MatchConfiguration(x) => p!(x),
            Packet::PlayerInput(x) => p!(x),
            Packet::DesiredGameState(x) => p!(x),
            Packet::RenderGroup(x) => p!(x),
            Packet::RemoveRenderGroup(x) => p!(x),
            Packet::MatchComm(x) => p!(x),
            Packet::BallPrediction(x) => p!(x),
            Packet::ConnectionSettings(x) => p!(x),
            Packet::StopCommand(x) => p!(x),
            Packet::SetLoadout(x) => p!(x),
            Packet::InitComplete => Vec::new(),
            Packet::ControllableTeamInfo(x) => p!(x),
        }
    }

    pub fn from_payload(data_type: u16, payload: &[u8]) -> Result<Self, PacketParseError> {
        // TODO: make this mess nicer
        macro_rules! p {
            ($x:ident) => {
                $x::read_as_root(payload)?.try_into().unwrap()
            };
        }

        match data_type {
            0 => Ok(Self::None),
            1 => Ok(Self::GamePacket(p!(GamePacketRef))),
            2 => Ok(Self::FieldInfo(p!(FieldInfoRef))),
            3 => Ok(Self::StartCommand(p!(StartCommandRef))),
            4 => Ok(Self::MatchConfiguration(p!(MatchConfigurationRef))),
            5 => Ok(Self::PlayerInput(p!(PlayerInputRef))),
            6 => Ok(Self::DesiredGameState(p!(DesiredGameStateRef))),
            7 => Ok(Self::RenderGroup(p!(RenderGroupRef))),
            8 => Ok(Self::RemoveRenderGroup(p!(RemoveRenderGroupRef))),
            9 => Ok(Self::MatchComm(p!(MatchCommRef))),
            10 => Ok(Self::BallPrediction(p!(BallPredictionRef))),
            11 => Ok(Self::ConnectionSettings(p!(ConnectionSettingsRef))),
            12 => Ok(Self::StopCommand(p!(StopCommandRef))),
            13 => Ok(Self::SetLoadout(p!(SetLoadoutRef))),
            14 => Ok(Self::InitComplete),
            15 => Ok(Self::ControllableTeamInfo(p!(ControllableTeamInfoRef))),
            _ => Err(PacketParseError::InvalidDataType(data_type)),
        }
    }
}

pub struct RLBotConnection {
    stream: TcpStream,
    builder: planus::Builder,
    recv_buf: [u8; u16::MAX as usize],
}

impl RLBotConnection {
    fn send_packet_enum(&mut self, packet: Packet) -> Result<(), RLBotError> {
        let data_type_bin = packet.data_type().to_be_bytes().to_vec();
        let payload = packet.build(&mut self.builder);
        let data_len_bin = (payload.len() as u16).to_be_bytes().to_vec();

        // Join so we make sure everything gets written in the right order
        let joined = [data_type_bin, data_len_bin, payload].concat();

        self.stream.write_all(&joined)?;
        self.stream.flush()?;
        Ok(())
    }

    pub fn send_packet(&mut self, packet: impl Into<Packet>) -> Result<(), RLBotError> {
        self.send_packet_enum(packet.into())
    }

    pub fn recv_packet(&mut self) -> Result<Packet, RLBotError> {
        let mut buf = [0u8; 4];

        self.stream.read_exact(&mut buf)?;

        let data_type = u16::from_be_bytes([buf[0], buf[1]]);
        let data_len = u16::from_be_bytes([buf[2], buf[3]]);

        let buf = &mut self.recv_buf[0..data_len as usize];

        self.stream.read_exact(buf)?;

        let packet = Packet::from_payload(data_type, buf)?;

        Ok(packet)
    }

    pub fn new(addr: &str) -> Result<RLBotConnection, RLBotError> {
        let stream = TcpStream::connect(SocketAddr::from_str(addr)?)?;

        stream.set_nodelay(true)?;

        Ok(RLBotConnection {
            stream,
            builder: planus::Builder::with_capacity(1024),
            recv_buf: [0u8; u16::MAX as usize],
        })
    }
}

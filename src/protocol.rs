use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::model::{AgentId, ExitInfo, StatusReport};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_LEN: usize = 32 * 1024 * 1024;

const KIND_HELLO: u8 = 1;
const KIND_ATTACH: u8 = 2;
const KIND_DETACH: u8 = 3;
const KIND_INPUT: u8 = 4;
const KIND_RESIZE: u8 = 5;
const KIND_KILL: u8 = 6;
const KIND_SEEN: u8 = 7;
const KIND_WELCOME: u8 = 64;
const KIND_STATUS: u8 = 65;
const KIND_SNAPSHOT: u8 = 66;
const KIND_OUTPUT: u8 = 67;
const KIND_EXITED: u8 = 68;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermSize {
    pub rows: u16,
    pub cols: u16,
}

impl TermSize {
    pub const MIN_ROWS: u16 = 2;
    pub const MIN_COLS: u16 = 10;

    pub fn new(rows: u16, cols: u16) -> Self {
        Self { rows: rows.max(Self::MIN_ROWS), cols: cols.max(Self::MIN_COLS) }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub token: String,
    pub protocol: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    pub agent_id: AgentId,
    pub protocol: u32,
    pub version: String,
    pub holder_pid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub size: TermSize,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMsg {
    Hello(Hello),
    Attach(TermSize),
    Detach,
    Input(Vec<u8>),
    Resize(TermSize),
    Kill,
    Seen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HolderMsg {
    Welcome(Welcome),
    Status(StatusReport),
    Snapshot(Snapshot),
    Output(Vec<u8>),
    Exited(ExitInfo),
}

pub fn encode_frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len() + 1).unwrap_or(u32::MAX);
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.extend_from_slice(&len.to_le_bytes());
    frame.push(kind);
    frame.extend_from_slice(payload);
    frame
}

pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid frame length"));
    }
    let mut kind = [0u8; 1];
    reader.read_exact(&mut kind)?;
    let mut payload = vec![0u8; len - 1];
    reader.read_exact(&mut payload)?;
    Ok((kind[0], payload))
}

fn json<T: Serialize>(kind: u8, value: &T) -> Vec<u8> {
    let payload = serde_json::to_vec(value).unwrap_or_default();
    encode_frame(kind, &payload)
}

fn parse<T: DeserializeOwned>(payload: &[u8]) -> io::Result<T> {
    serde_json::from_slice(payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn unknown(kind: u8) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("unknown message kind {kind}"))
}

impl ClientMsg {
    pub fn to_frame(&self) -> Vec<u8> {
        match self {
            ClientMsg::Hello(hello) => json(KIND_HELLO, hello),
            ClientMsg::Attach(size) => json(KIND_ATTACH, size),
            ClientMsg::Detach => encode_frame(KIND_DETACH, &[]),
            ClientMsg::Input(bytes) => encode_frame(KIND_INPUT, bytes),
            ClientMsg::Resize(size) => json(KIND_RESIZE, size),
            ClientMsg::Kill => encode_frame(KIND_KILL, &[]),
            ClientMsg::Seen => encode_frame(KIND_SEEN, &[]),
        }
    }

    pub fn from_frame(kind: u8, payload: Vec<u8>) -> io::Result<Self> {
        Ok(match kind {
            KIND_HELLO => ClientMsg::Hello(parse(&payload)?),
            KIND_ATTACH => ClientMsg::Attach(parse(&payload)?),
            KIND_DETACH => ClientMsg::Detach,
            KIND_INPUT => ClientMsg::Input(payload),
            KIND_RESIZE => ClientMsg::Resize(parse(&payload)?),
            KIND_KILL => ClientMsg::Kill,
            KIND_SEEN => ClientMsg::Seen,
            other => return Err(unknown(other)),
        })
    }

    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let (kind, payload) = read_frame(reader)?;
        Self::from_frame(kind, payload)
    }

    pub fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_frame())?;
        writer.flush()
    }
}

impl HolderMsg {
    pub fn to_frame(&self) -> Vec<u8> {
        match self {
            HolderMsg::Welcome(welcome) => json(KIND_WELCOME, welcome),
            HolderMsg::Status(status) => json(KIND_STATUS, status),
            HolderMsg::Snapshot(snapshot) => {
                let mut payload = Vec::with_capacity(snapshot.data.len() + 4);
                payload.extend_from_slice(&snapshot.size.rows.to_le_bytes());
                payload.extend_from_slice(&snapshot.size.cols.to_le_bytes());
                payload.extend_from_slice(&snapshot.data);
                encode_frame(KIND_SNAPSHOT, &payload)
            }
            HolderMsg::Output(bytes) => encode_frame(KIND_OUTPUT, bytes),
            HolderMsg::Exited(info) => json(KIND_EXITED, info),
        }
    }

    pub fn from_frame(kind: u8, payload: Vec<u8>) -> io::Result<Self> {
        Ok(match kind {
            KIND_WELCOME => HolderMsg::Welcome(parse(&payload)?),
            KIND_STATUS => HolderMsg::Status(parse(&payload)?),
            KIND_SNAPSHOT => {
                if payload.len() < 4 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "short snapshot"));
                }
                let rows = u16::from_le_bytes([payload[0], payload[1]]);
                let cols = u16::from_le_bytes([payload[2], payload[3]]);
                HolderMsg::Snapshot(Snapshot { size: TermSize { rows, cols }, data: payload[4..].to_vec() })
            }
            KIND_OUTPUT => HolderMsg::Output(payload),
            KIND_EXITED => HolderMsg::Exited(parse(&payload)?),
            other => return Err(unknown(other)),
        })
    }

    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let (kind, payload) = read_frame(reader)?;
        Self::from_frame(kind, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentKind, AgentState, NestedAgent};

    fn round_trip_client(msg: ClientMsg) {
        let frame = msg.to_frame();
        let decoded = ClientMsg::read(&mut frame.as_slice()).unwrap();
        assert_eq!(decoded, msg);
    }

    fn round_trip_holder(msg: HolderMsg) {
        let frame = msg.to_frame();
        let decoded = HolderMsg::read(&mut frame.as_slice()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn client_messages_round_trip() {
        round_trip_client(ClientMsg::Hello(Hello { token: "t".into(), protocol: PROTOCOL_VERSION }));
        round_trip_client(ClientMsg::Attach(TermSize { rows: 40, cols: 120 }));
        round_trip_client(ClientMsg::Detach);
        round_trip_client(ClientMsg::Input(b"ls -la\r".to_vec()));
        round_trip_client(ClientMsg::Resize(TermSize { rows: 10, cols: 20 }));
        round_trip_client(ClientMsg::Kill);
        round_trip_client(ClientMsg::Seen);
    }

    #[test]
    fn holder_messages_round_trip() {
        round_trip_holder(HolderMsg::Welcome(Welcome {
            agent_id: 4,
            protocol: PROTOCOL_VERSION,
            version: "0.1.0".into(),
            holder_pid: 99,
        }));
        round_trip_holder(HolderMsg::Status(StatusReport {
            state: AgentState::Working,
            state_since_ms: 10,
            turn_started_ms: Some(5),
            last_turn_ms: None,
            started_at_ms: 1,
            title: "⠂ Fixing tests".into(),
            activity: "Fixing tests".into(),
            rows: 24,
            cols: 80,
            exit: None,
            nested: Some(NestedAgent {
                kind: AgentKind::Claude,
                pid: 4242,
                conversation_id: Some("5bd0ccb0-0d09-47ca-9524-6eb740341fa9".into()),
                cwd: Some("/work/app".into()),
                since_ms: 3,
            }),
            viewers: Some(2),
            test: None,
            input_at_ms: 8,
        }));
        round_trip_holder(HolderMsg::Snapshot(Snapshot {
            size: TermSize { rows: 3, cols: 4 },
            data: b"\x1b[Hhi".to_vec(),
        }));
        round_trip_holder(HolderMsg::Output(vec![0, 1, 2, 255]));
        round_trip_holder(HolderMsg::Exited(ExitInfo { code: Some(1), error: None, at_ms: 7 }));
    }

    #[test]
    fn rejects_oversized_frames() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&u32::try_from(MAX_FRAME_LEN + 1).unwrap().to_le_bytes());
        frame.push(KIND_INPUT);
        let err = read_frame(&mut frame.as_slice()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_zero_length_and_unknown_kind() {
        let zero = 0u32.to_le_bytes();
        assert!(read_frame(&mut zero.as_slice()).is_err());
        let frame = encode_frame(200, b"x");
        assert!(ClientMsg::read(&mut frame.as_slice()).is_err());
        assert!(HolderMsg::read(&mut frame.as_slice()).is_err());
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let frame = ClientMsg::Input(b"hello".to_vec()).to_frame();
        let err = ClientMsg::read(&mut &frame[..frame.len() - 1]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn term_size_enforces_minimum() {
        assert_eq!(TermSize::new(0, 0), TermSize { rows: 2, cols: 10 });
    }
}

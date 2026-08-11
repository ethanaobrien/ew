// Wire format for the multi-live WebSocket relay, per docs/multi-live-ws-protocol.md.
//
// The C# client rewrite encodes/decodes the exact same bytes, so this module is a
// straight transcription of the spec tables rather than idiomatic Rust: every field is
// written in declaration order, all integers are little-endian, strings are UTF-8 behind
// a u16 length, and counts are u8. Nothing here is serde-driven - the type tags mirror
// Photon's Hashtable boxing (an Int32 must arrive as Int32 or GetCurrentRoomEventId's
// exact `is int` check fails), and a derive would hide that.
//
// Both directions are implemented in both halves (encode + decode for client messages
// AND for server messages) even though the relay only ever decodes the former and
// encodes the latter. The unused halves are what the round-trip tests exercise, and they
// double as the reference the C# side is written against.

use std::fmt;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Value tags (see "Value encoding" in the spec).
pub const TAG_NULL: u8 = 0;
pub const TAG_INT: u8 = 1;
pub const TAG_STRING: u8 = 2;
pub const TAG_BOOL: u8 = 3;
pub const TAG_DOUBLE: u8 = 4;

// Client -> server opcodes.
pub const OP_AUTH: u8 = 1;
pub const OP_JOIN_LOBBY: u8 = 2;
pub const OP_LEAVE_LOBBY: u8 = 3;
pub const OP_CREATE_ROOM: u8 = 4;
pub const OP_JOIN_ROOM: u8 = 5;
pub const OP_JOIN_RANDOM: u8 = 6;
pub const OP_LEAVE_ROOM: u8 = 7;
pub const OP_REJOIN: u8 = 8;
pub const OP_SET_PLAYER_PROPS: u8 = 9;
pub const OP_SET_ROOM_PROPS: u8 = 10;
pub const OP_RPC: u8 = 11;
pub const OP_PING: u8 = 12;

// Server -> client opcodes.
pub const OP_AUTH_OK: u8 = 20;
pub const OP_JOINED_LOBBY: u8 = 21;
pub const OP_LEFT_LOBBY: u8 = 22;
pub const OP_JOINED_ROOM: u8 = 23;
pub const OP_CREATE_FAILED: u8 = 24;
pub const OP_JOIN_FAILED: u8 = 25;
pub const OP_PLAYER_ENTERED: u8 = 26;
pub const OP_PLAYER_LEFT: u8 = 27;
pub const OP_LEFT_ROOM: u8 = 28;
pub const OP_ROOM_PROPS_CHANGED: u8 = 29;
pub const OP_PLAYER_PROPS_CHANGED: u8 = 30;
pub const OP_MASTER_SWITCHED: u8 = 31;
// Distinct name from the client-side OP_RPC (11); same message, opposite direction.
pub const OP_RPC_BROADCAST: u8 = 32;
pub const OP_PONG: u8 = 33;
pub const OP_KICKED: u8 = 34;

// Close codes. The spec names 4001 and 4002; 4000 covers the framing errors it
// describes ("Text frames are a protocol error -> close", malformed frame, unknown
// opcode) without naming a code for them.
pub const CLOSE_PROTOCOL: u16 = 4000;
pub const CLOSE_UNAUTHENTICATED: u16 = 4001;
pub const CLOSE_RATE_LIMIT: u16 = 4002;
// Added with the liveness sweep: nothing was heard from this connection for three ping
// intervals, so the relay stops holding its seat. See LIVENESS_TIMEOUT in ws.rs.
pub const CLOSE_IDLE_TIMEOUT: u16 = 4003;

// Photon ErrorCode mirrors, so the client's existing logging keeps its meaning.
pub const ERR_GAME_ID_ALREADY_EXISTS: i32 = 32766;
pub const ERR_GAME_FULL: i32 = 32765;
pub const ERR_GAME_CLOSED: i32 = 32764;
pub const ERR_NO_RANDOM_MATCH_FOUND: i32 = 32760;
pub const ERR_GAME_DOES_NOT_EXIST: i32 = 32758;

// Kicked causes. A room only dies once nobody is left to be told, so cause 1 is still
// reserved — encoded, decoded and tested so the C# side can be written against it now.
#[allow(dead_code)]
pub const KICK_ROOM_DESTROYED: i32 = 1;
// Cause 2 is real: the liveness sweep sends it immediately before close 4003, so a client
// that is alive enough to read (a suspended app coming back, a stalled network) can say
// why it was dropped instead of showing a bare socket error.
pub const KICK_IDLE_TIMEOUT: i32 = 2;

// ---------------------------------------------------------------------------
// Values and maps
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i32),
    Str(String),
    Bool(bool),
    Double(f64),
}

impl Value {
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(v) => Some(v),
            _ => None,
        }
    }
}

// An ordered key -> value bag. Ordering is not semantically meaningful on the wire, but
// keeping insertion order makes the changed-subset broadcasts arrive in the order the
// sender wrote them, which is one less thing for the client to reason about (and makes
// the tests deterministic).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(Vec<(String, Value)>);

impl Map {
    pub fn new() -> Self {
        Map(Vec::new())
    }

    // Duplicate keys collapse last-writer-wins, exactly like the Hashtable this mirrors.
    // Convenience for the tests and for any caller building a bag from scratch; the
    // relay itself only ever merges into an existing bag.
    #[allow(dead_code)]
    pub fn from_pairs<I, K>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, Value)>,
        K: Into<String>,
    {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.set(key, value);
        }
        map
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_int(&self, key: &str) -> Option<i32> {
        self.get(key).and_then(Value::as_int)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    // Last-writer-wins in place: an existing key keeps its position.
    pub fn set<K: Into<String>>(&mut self, key: K, value: Value) {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    // Last-writer-wins merge of `other` INTO self. The relay uses it for property updates
    // and for seeding/merging a joiner's bag out of the seating op's `playerProps`.
    pub fn merge(&mut self, other: &Map) {
        for (key, value) in other.iter() {
            self.set(key, value.clone());
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum ClientMsg {
    Auth { user_id: String, token: String },
    JoinLobby { name: String },
    LeaveLobby,
    // `props` is the ROOM bag; `player_props` is the joiner's own bag, mirroring Photon's
    // OpCreateRoom/OpJoinRoom actorProperties. See docs/multi-live-ws-protocol.md,
    // "Player properties at join time".
    CreateRoom {
        name: String,
        max_players: u8,
        visible: bool,
        open: bool,
        props: Map,
        lobby_prop_keys: Vec<String>,
        player_props: Map,
    },
    JoinRoom { name: String, player_props: Map },
    JoinRandom { power_min: i32, power_max: i32, levels: Vec<Value>, player_props: Map },
    LeaveRoom,
    Rejoin { name: String, player_props: Map },
    SetPlayerProps { props: Map },
    SetRoomProps { props: Map },
    Rpc { name: String, params: Vec<Value> },
    Ping,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServerMsg {
    AuthOk { actorless_time_ms: f64 },
    JoinedLobby,
    LeftLobby,
    JoinedRoom {
        room_name: String,
        your_actor: i32,
        master_actor: i32,
        room_props: Map,
        players: Vec<(i32, Map)>,
    },
    CreateFailed { code: i32, msg: String },
    JoinFailed { code: i32, msg: String },
    PlayerEntered { actor: i32, props: Map },
    PlayerLeft { actor: i32, inactive: bool },
    LeftRoom,
    RoomPropsChanged { props: Map },
    PlayerPropsChanged { actor: i32, props: Map },
    MasterSwitched { new_master_actor: i32 },
    Rpc { sender_actor: i32, name: String, params: Vec<Value> },
    Pong { server_time_ms: f64 },
    // Sent by the liveness sweep with KICK_IDLE_TIMEOUT; KICK_ROOM_DESTROYED is reserved.
    Kicked { cause: i32 },
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum DecodeError {
    // Frame ran out before the field did.
    Truncated,
    // Zero-length frame: there is not even an opcode.
    Empty,
    UnknownOp(u8),
    UnknownTag(u8),
    BadUtf8,
    // "One frame = one message" - anything after the message is a framing bug.
    TrailingBytes,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "truncated frame"),
            DecodeError::Empty => write!(f, "empty frame"),
            DecodeError::UnknownOp(op) => write!(f, "unknown opcode {}", op),
            DecodeError::UnknownTag(tag) => write!(f, "unknown value tag {}", tag),
            DecodeError::BadUtf8 => write!(f, "string is not valid utf-8"),
            DecodeError::TrailingBytes => write!(f, "trailing bytes after message"),
        }
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        if end > self.buf.len() {
            return Err(DecodeError::Truncated);
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, DecodeError> {
        // Photon booleans are one byte; anything non-zero is true.
        Ok(self.u8()? != 0)
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f64(&mut self) -> Result<f64, DecodeError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn string(&mut self) -> Result<String, DecodeError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::BadUtf8)
    }

    fn value(&mut self) -> Result<Value, DecodeError> {
        let tag = self.u8()?;
        match tag {
            TAG_NULL => Ok(Value::Null),
            TAG_INT => Ok(Value::Int(self.i32()?)),
            TAG_STRING => Ok(Value::Str(self.string()?)),
            TAG_BOOL => Ok(Value::Bool(self.bool()?)),
            TAG_DOUBLE => Ok(Value::Double(self.f64()?)),
            other => Err(DecodeError::UnknownTag(other)),
        }
    }

    fn map(&mut self) -> Result<Map, DecodeError> {
        let count = self.u8()?;
        let mut map = Map::new();
        for _ in 0..count {
            let key = self.string()?;
            let value = self.value()?;
            map.set(key, value);
        }
        Ok(map)
    }

    fn value_list(&mut self) -> Result<Vec<Value>, DecodeError> {
        let count = self.u8()?;
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            out.push(self.value()?);
        }
        Ok(out)
    }

    fn string_list(&mut self) -> Result<Vec<String>, DecodeError> {
        let count = self.u8()?;
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            out.push(self.string()?);
        }
        Ok(out)
    }

    fn finish<T>(&self, value: T) -> Result<T, DecodeError> {
        if self.pos != self.buf.len() {
            return Err(DecodeError::TrailingBytes);
        }
        Ok(value)
    }
}

pub fn decode_client(buf: &[u8]) -> Result<ClientMsg, DecodeError> {
    let mut r = Reader::new(buf);
    let op = r.u8().map_err(|_| DecodeError::Empty)?;
    let msg = match op {
        OP_AUTH => ClientMsg::Auth { user_id: r.string()?, token: r.string()? },
        OP_JOIN_LOBBY => ClientMsg::JoinLobby { name: r.string()? },
        OP_LEAVE_LOBBY => ClientMsg::LeaveLobby,
        OP_CREATE_ROOM => ClientMsg::CreateRoom {
            name: r.string()?,
            max_players: r.u8()?,
            visible: r.bool()?,
            open: r.bool()?,
            props: r.map()?,
            lobby_prop_keys: r.string_list()?,
            player_props: r.map()?,
        },
        OP_JOIN_ROOM => ClientMsg::JoinRoom { name: r.string()?, player_props: r.map()? },
        OP_JOIN_RANDOM => ClientMsg::JoinRandom {
            power_min: r.i32()?,
            power_max: r.i32()?,
            levels: r.value_list()?,
            player_props: r.map()?,
        },
        OP_LEAVE_ROOM => ClientMsg::LeaveRoom,
        OP_REJOIN => ClientMsg::Rejoin { name: r.string()?, player_props: r.map()? },
        OP_SET_PLAYER_PROPS => ClientMsg::SetPlayerProps { props: r.map()? },
        OP_SET_ROOM_PROPS => ClientMsg::SetRoomProps { props: r.map()? },
        OP_RPC => ClientMsg::Rpc { name: r.string()?, params: r.value_list()? },
        OP_PING => ClientMsg::Ping,
        other => return Err(DecodeError::UnknownOp(other)),
    };
    r.finish(msg)
}

// The client's half of the codec. The relay never calls it; it is the executable
// reference the C# rewrite is written against, and what the round-trip tests drive.
#[allow(dead_code)]
pub fn decode_server(buf: &[u8]) -> Result<ServerMsg, DecodeError> {
    let mut r = Reader::new(buf);
    let op = r.u8().map_err(|_| DecodeError::Empty)?;
    let msg = match op {
        OP_AUTH_OK => ServerMsg::AuthOk { actorless_time_ms: r.f64()? },
        OP_JOINED_LOBBY => ServerMsg::JoinedLobby,
        OP_LEFT_LOBBY => ServerMsg::LeftLobby,
        OP_JOINED_ROOM => {
            let room_name = r.string()?;
            let your_actor = r.i32()?;
            let master_actor = r.i32()?;
            let room_props = r.map()?;
            let count = r.u8()?;
            let mut players = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let actor = r.i32()?;
                players.push((actor, r.map()?));
            }
            ServerMsg::JoinedRoom { room_name, your_actor, master_actor, room_props, players }
        }
        OP_CREATE_FAILED => ServerMsg::CreateFailed { code: r.i32()?, msg: r.string()? },
        OP_JOIN_FAILED => ServerMsg::JoinFailed { code: r.i32()?, msg: r.string()? },
        OP_PLAYER_ENTERED => ServerMsg::PlayerEntered { actor: r.i32()?, props: r.map()? },
        OP_PLAYER_LEFT => ServerMsg::PlayerLeft { actor: r.i32()?, inactive: r.bool()? },
        OP_LEFT_ROOM => ServerMsg::LeftRoom,
        OP_ROOM_PROPS_CHANGED => ServerMsg::RoomPropsChanged { props: r.map()? },
        OP_PLAYER_PROPS_CHANGED => {
            ServerMsg::PlayerPropsChanged { actor: r.i32()?, props: r.map()? }
        }
        OP_MASTER_SWITCHED => ServerMsg::MasterSwitched { new_master_actor: r.i32()? },
        OP_RPC_BROADCAST => ServerMsg::Rpc {
            sender_actor: r.i32()?,
            name: r.string()?,
            params: r.value_list()?,
        },
        OP_PONG => ServerMsg::Pong { server_time_ms: r.f64()? },
        OP_KICKED => ServerMsg::Kicked { cause: r.i32()? },
        other => return Err(DecodeError::UnknownOp(other)),
    };
    r.finish(msg)
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

// The relay never legitimately produces a string past 64KiB or a count past 255 (keys
// are one or two characters, rooms hold four players, RPC params are a handful of ints),
// so encoding is infallible and the guards below only exist to keep a bug from emitting
// a frame the peer would mis-frame. They log and clamp rather than panic in a handler.
fn put_str(out: &mut Vec<u8>, s: &str) {
    let mut bytes = s.as_bytes();
    if bytes.len() > u16::MAX as usize {
        println!("multi_live/ws: string of {} bytes truncated to fit u16 length", bytes.len());
        let mut end = u16::MAX as usize;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        bytes = &s.as_bytes()[..end];
    }
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn put_count(out: &mut Vec<u8>, count: usize, what: &str) -> usize {
    let clamped = if count > u8::MAX as usize {
        println!("multi_live/ws: {} count {} clamped to 255", what, count);
        u8::MAX as usize
    } else {
        count
    };
    out.push(clamped as u8);
    clamped
}

fn put_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(TAG_NULL),
        Value::Int(v) => {
            out.push(TAG_INT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Str(v) => {
            out.push(TAG_STRING);
            put_str(out, v);
        }
        Value::Bool(v) => {
            out.push(TAG_BOOL);
            out.push(if *v { 1 } else { 0 });
        }
        Value::Double(v) => {
            out.push(TAG_DOUBLE);
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}

fn put_map(out: &mut Vec<u8>, map: &Map) {
    let count = put_count(out, map.len(), "map");
    for (key, value) in map.iter().take(count) {
        put_str(out, key);
        put_value(out, value);
    }
}

fn put_value_list(out: &mut Vec<u8>, values: &[Value]) {
    let count = put_count(out, values.len(), "value list");
    for value in values.iter().take(count) {
        put_value(out, value);
    }
}

#[allow(dead_code)]
fn put_string_list(out: &mut Vec<u8>, values: &[String]) {
    let count = put_count(out, values.len(), "string list");
    for value in values.iter().take(count) {
        put_str(out, value);
    }
}

// See decode_server: the client's half, kept honest by the round-trip tests.
#[allow(dead_code)]
pub fn encode_client(msg: &ClientMsg) -> Vec<u8> {
    let mut out = Vec::new();
    match msg {
        ClientMsg::Auth { user_id, token } => {
            out.push(OP_AUTH);
            put_str(&mut out, user_id);
            put_str(&mut out, token);
        }
        ClientMsg::JoinLobby { name } => {
            out.push(OP_JOIN_LOBBY);
            put_str(&mut out, name);
        }
        ClientMsg::LeaveLobby => out.push(OP_LEAVE_LOBBY),
        ClientMsg::CreateRoom { name, max_players, visible, open, props, lobby_prop_keys, player_props } => {
            out.push(OP_CREATE_ROOM);
            put_str(&mut out, name);
            out.push(*max_players);
            out.push(if *visible { 1 } else { 0 });
            out.push(if *open { 1 } else { 0 });
            put_map(&mut out, props);
            put_string_list(&mut out, lobby_prop_keys);
            put_map(&mut out, player_props);
        }
        ClientMsg::JoinRoom { name, player_props } => {
            out.push(OP_JOIN_ROOM);
            put_str(&mut out, name);
            put_map(&mut out, player_props);
        }
        ClientMsg::JoinRandom { power_min, power_max, levels, player_props } => {
            out.push(OP_JOIN_RANDOM);
            out.extend_from_slice(&power_min.to_le_bytes());
            out.extend_from_slice(&power_max.to_le_bytes());
            put_value_list(&mut out, levels);
            put_map(&mut out, player_props);
        }
        ClientMsg::LeaveRoom => out.push(OP_LEAVE_ROOM),
        ClientMsg::Rejoin { name, player_props } => {
            out.push(OP_REJOIN);
            put_str(&mut out, name);
            put_map(&mut out, player_props);
        }
        ClientMsg::SetPlayerProps { props } => {
            out.push(OP_SET_PLAYER_PROPS);
            put_map(&mut out, props);
        }
        ClientMsg::SetRoomProps { props } => {
            out.push(OP_SET_ROOM_PROPS);
            put_map(&mut out, props);
        }
        ClientMsg::Rpc { name, params } => {
            out.push(OP_RPC);
            put_str(&mut out, name);
            put_value_list(&mut out, params);
        }
        ClientMsg::Ping => out.push(OP_PING),
    }
    out
}

pub fn encode_server(msg: &ServerMsg) -> Vec<u8> {
    let mut out = Vec::new();
    match msg {
        ServerMsg::AuthOk { actorless_time_ms } => {
            out.push(OP_AUTH_OK);
            out.extend_from_slice(&actorless_time_ms.to_le_bytes());
        }
        ServerMsg::JoinedLobby => out.push(OP_JOINED_LOBBY),
        ServerMsg::LeftLobby => out.push(OP_LEFT_LOBBY),
        ServerMsg::JoinedRoom { room_name, your_actor, master_actor, room_props, players } => {
            out.push(OP_JOINED_ROOM);
            put_str(&mut out, room_name);
            out.extend_from_slice(&your_actor.to_le_bytes());
            out.extend_from_slice(&master_actor.to_le_bytes());
            put_map(&mut out, room_props);
            let count = put_count(&mut out, players.len(), "player");
            for (actor, props) in players.iter().take(count) {
                out.extend_from_slice(&actor.to_le_bytes());
                put_map(&mut out, props);
            }
        }
        ServerMsg::CreateFailed { code, msg } => {
            out.push(OP_CREATE_FAILED);
            out.extend_from_slice(&code.to_le_bytes());
            put_str(&mut out, msg);
        }
        ServerMsg::JoinFailed { code, msg } => {
            out.push(OP_JOIN_FAILED);
            out.extend_from_slice(&code.to_le_bytes());
            put_str(&mut out, msg);
        }
        ServerMsg::PlayerEntered { actor, props } => {
            out.push(OP_PLAYER_ENTERED);
            out.extend_from_slice(&actor.to_le_bytes());
            put_map(&mut out, props);
        }
        ServerMsg::PlayerLeft { actor, inactive } => {
            out.push(OP_PLAYER_LEFT);
            out.extend_from_slice(&actor.to_le_bytes());
            out.push(if *inactive { 1 } else { 0 });
        }
        ServerMsg::LeftRoom => out.push(OP_LEFT_ROOM),
        ServerMsg::RoomPropsChanged { props } => {
            out.push(OP_ROOM_PROPS_CHANGED);
            put_map(&mut out, props);
        }
        ServerMsg::PlayerPropsChanged { actor, props } => {
            out.push(OP_PLAYER_PROPS_CHANGED);
            out.extend_from_slice(&actor.to_le_bytes());
            put_map(&mut out, props);
        }
        ServerMsg::MasterSwitched { new_master_actor } => {
            out.push(OP_MASTER_SWITCHED);
            out.extend_from_slice(&new_master_actor.to_le_bytes());
        }
        ServerMsg::Rpc { sender_actor, name, params } => {
            out.push(OP_RPC_BROADCAST);
            out.extend_from_slice(&sender_actor.to_le_bytes());
            put_str(&mut out, name);
            put_value_list(&mut out, params);
        }
        ServerMsg::Pong { server_time_ms } => {
            out.push(OP_PONG);
            out.extend_from_slice(&server_time_ms.to_le_bytes());
        }
        ServerMsg::Kicked { cause } => {
            out.push(OP_KICKED);
            out.extend_from_slice(&cause.to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_client(msg: ClientMsg) {
        let bytes = encode_client(&msg);
        let back = decode_client(&bytes).expect("client message decodes");
        assert_eq!(back, msg);
        assert_eq!(encode_client(&back), bytes);
    }

    fn round_server(msg: ServerMsg) {
        let bytes = encode_server(&msg);
        let back = decode_server(&bytes).expect("server message decodes");
        assert_eq!(back, msg);
        assert_eq!(encode_server(&back), bytes);
    }

    fn sample_map() -> Map {
        Map::from_pairs(vec![
            ("A", Value::Int(-7)),
            ("B", Value::Str("ラブライブ".to_string())),
            ("C", Value::Bool(true)),
            ("D", Value::Double(0.5)),
            ("E", Value::Null),
        ])
    }

    #[test]
    fn every_client_message_round_trips() {
        round_client(ClientMsg::Auth { user_id: "12345".into(), token: "a-uuid".into() });
        round_client(ClientMsg::JoinLobby { name: "MultiEventLobby_31".into() });
        round_client(ClientMsg::LeaveLobby);
        round_client(ClientMsg::CreateRoom {
            name: "123456".into(),
            max_players: 4,
            visible: false,
            open: true,
            props: sample_map(),
            lobby_prop_keys: vec!["C0".into(), "C1".into()],
            player_props: sample_map(),
        });
        round_client(ClientMsg::JoinRoom {
            name: "042719".into(),
            player_props: sample_map(),
        });
        round_client(ClientMsg::JoinRandom {
            power_min: 0,
            power_max: i32::MAX,
            levels: vec![Value::Int(1), Value::Int(4)],
            player_props: sample_map(),
        });
        round_client(ClientMsg::LeaveRoom);
        round_client(ClientMsg::Rejoin {
            name: "1000000".into(),
            player_props: sample_map(),
        });
        round_client(ClientMsg::SetPlayerProps { props: sample_map() });
        round_client(ClientMsg::SetRoomProps { props: sample_map() });
        round_client(ClientMsg::Rpc {
            name: "SendStamp".into(),
            params: vec![Value::Int(3), Value::Str("hi".into())],
        });
        round_client(ClientMsg::Ping);
    }

    #[test]
    fn every_server_message_round_trips() {
        round_server(ServerMsg::AuthOk { actorless_time_ms: 1_754_500_000_123.0 });
        round_server(ServerMsg::JoinedLobby);
        round_server(ServerMsg::LeftLobby);
        round_server(ServerMsg::JoinedRoom {
            room_name: "1000000".into(),
            your_actor: 2,
            master_actor: 1,
            room_props: sample_map(),
            players: vec![(1, sample_map()), (2, Map::new())],
        });
        round_server(ServerMsg::CreateFailed {
            code: ERR_GAME_ID_ALREADY_EXISTS,
            msg: "GameIdAlreadyExists".into(),
        });
        round_server(ServerMsg::JoinFailed { code: ERR_GAME_FULL, msg: "GameFull".into() });
        round_server(ServerMsg::PlayerEntered { actor: 3, props: sample_map() });
        round_server(ServerMsg::PlayerLeft { actor: 3, inactive: true });
        round_server(ServerMsg::PlayerLeft { actor: 3, inactive: false });
        round_server(ServerMsg::LeftRoom);
        round_server(ServerMsg::RoomPropsChanged { props: sample_map() });
        round_server(ServerMsg::PlayerPropsChanged { actor: 4, props: sample_map() });
        round_server(ServerMsg::MasterSwitched { new_master_actor: 2 });
        round_server(ServerMsg::Rpc {
            sender_actor: 1,
            name: "_MoveScene".into(),
            params: vec![Value::Null],
        });
        round_server(ServerMsg::Pong { server_time_ms: -0.0 });
        round_server(ServerMsg::Kicked { cause: KICK_ROOM_DESTROYED });
    }

    #[test]
    fn empty_map_and_empty_strings_round_trip() {
        round_client(ClientMsg::Auth { user_id: String::new(), token: String::new() });
        round_client(ClientMsg::JoinLobby { name: String::new() });
        round_client(ClientMsg::SetPlayerProps { props: Map::new() });
        round_client(ClientMsg::Rpc { name: String::new(), params: Vec::new() });
        round_client(ClientMsg::CreateRoom {
            name: String::new(),
            max_players: 0,
            visible: true,
            open: false,
            props: Map::new(),
            lobby_prop_keys: Vec::new(),
            player_props: Map::new(),
        });
        // A joiner that has committed nothing yet sends an empty bag on every seating op.
        round_client(ClientMsg::JoinRoom { name: String::new(), player_props: Map::new() });
        round_client(ClientMsg::Rejoin { name: String::new(), player_props: Map::new() });
        round_client(ClientMsg::JoinRandom {
            power_min: 0,
            power_max: 0,
            levels: Vec::new(),
            player_props: Map::new(),
        });
        // The trailing playerProps of a JoinRoom is exactly one byte when empty.
        assert_eq!(
            encode_client(&ClientMsg::JoinRoom { name: String::new(), player_props: Map::new() }),
            vec![OP_JOIN_ROOM, 0, 0, 0]
        );
        round_server(ServerMsg::JoinedRoom {
            room_name: String::new(),
            your_actor: 1,
            master_actor: 1,
            room_props: Map::new(),
            players: Vec::new(),
        });
        // An empty map is exactly one byte of payload.
        assert_eq!(encode_client(&ClientMsg::SetRoomProps { props: Map::new() }), vec![OP_SET_ROOM_PROPS, 0]);
    }

    #[test]
    fn max_u8_counts_round_trip() {
        let map = Map::from_pairs((0..255).map(|i| (format!("k{}", i), Value::Int(i))));
        assert_eq!(map.len(), 255);
        round_client(ClientMsg::SetPlayerProps { props: map.clone() });

        let params: Vec<Value> = (0..255).map(Value::Int).collect();
        round_client(ClientMsg::Rpc { name: "SendStamp".into(), params });

        let keys: Vec<String> = (0..255).map(|i| format!("k{}", i)).collect();
        round_client(ClientMsg::CreateRoom {
            name: "n".into(),
            max_players: 255,
            visible: true,
            open: true,
            props: map.clone(),
            lobby_prop_keys: keys,
            player_props: map.clone(),
        });
        // A full 255-key bag on each of the other three seating ops.
        round_client(ClientMsg::JoinRoom { name: "n".into(), player_props: map.clone() });
        round_client(ClientMsg::Rejoin { name: "n".into(), player_props: map.clone() });
        round_client(ClientMsg::JoinRandom {
            power_min: 0,
            power_max: 1,
            levels: vec![Value::Int(4)],
            player_props: map,
        });

        let players: Vec<(i32, Map)> = (0..255).map(|i| (i, Map::new())).collect();
        round_server(ServerMsg::JoinedRoom {
            room_name: "n".into(),
            your_actor: 1,
            master_actor: 1,
            room_props: Map::new(),
            players,
        });
    }

    #[test]
    fn integer_and_double_extremes_survive() {
        round_client(ClientMsg::JoinRandom {
            power_min: i32::MIN,
            power_max: i32::MAX,
            levels: vec![Value::Int(i32::MIN), Value::Int(i32::MAX), Value::Int(0)],
            player_props: Map::new(),
        });
        round_server(ServerMsg::PlayerEntered {
            actor: i32::MIN,
            props: Map::from_pairs(vec![
                ("a", Value::Double(f64::MAX)),
                ("b", Value::Double(f64::MIN_POSITIVE)),
                ("c", Value::Double(f64::INFINITY)),
            ]),
        });
    }

    #[test]
    fn wire_layout_is_byte_exact() {
        // Guard the framing itself: little-endian everywhere, u16 string lengths,
        // u8 counts, tag-then-payload values. If this test needs updating, the C#
        // client needs updating too.
        assert_eq!(
            encode_client(&ClientMsg::Auth { user_id: "7".into(), token: "ab".into() }),
            vec![OP_AUTH, 1, 0, b'7', 2, 0, b'a', b'b']
        );
        assert_eq!(
            encode_client(&ClientMsg::JoinRandom {
                power_min: 1,
                power_max: 256,
                levels: vec![Value::Int(4)],
                player_props: Map::new(),
            }),
            // ... levels ValueList ..., then the trailing empty playerProps map (count 0).
            vec![OP_JOIN_RANDOM, 1, 0, 0, 0, 0, 1, 0, 0, 1, TAG_INT, 4, 0, 0, 0, 0]
        );
        // The seating ops' trailing bag is an ordinary Map: count:u8 then (key,value)*.
        assert_eq!(
            encode_client(&ClientMsg::JoinRoom {
                name: "42".into(),
                player_props: Map::from_pairs(vec![("B", Value::Str("7".into()))]),
            }),
            vec![OP_JOIN_ROOM, 2, 0, b'4', b'2', 1, 1, 0, b'B', TAG_STRING, 1, 0, b'7']
        );
        assert_eq!(
            encode_client(&ClientMsg::Rejoin {
                name: "42".into(),
                player_props: Map::from_pairs(vec![("F", Value::Int(5))]),
            }),
            vec![OP_REJOIN, 2, 0, b'4', b'2', 1, 1, 0, b'F', TAG_INT, 5, 0, 0, 0]
        );
        assert_eq!(
            encode_client(&ClientMsg::SetPlayerProps {
                props: Map::from_pairs(vec![("LB", Value::Int(12))]),
            }),
            vec![OP_SET_PLAYER_PROPS, 1, 2, 0, b'L', b'B', TAG_INT, 12, 0, 0, 0]
        );
        assert_eq!(
            encode_server(&ServerMsg::PlayerLeft { actor: 2, inactive: true }),
            vec![OP_PLAYER_LEFT, 2, 0, 0, 0, 1]
        );
        assert_eq!(
            encode_server(&ServerMsg::AuthOk { actorless_time_ms: 1.0 }),
            vec![OP_AUTH_OK, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f]
        );
    }

    #[test]
    fn malformed_frames_are_rejected() {
        assert_eq!(decode_client(&[]), Err(DecodeError::Empty));
        assert_eq!(decode_client(&[99]), Err(DecodeError::UnknownOp(99)));
        assert_eq!(decode_server(&[99]), Err(DecodeError::UnknownOp(99)));
        // JoinLobby with a length prefix longer than the payload.
        assert_eq!(decode_client(&[OP_JOIN_LOBBY, 4, 0, b'a']), Err(DecodeError::Truncated));
        // Ping carries nothing.
        assert_eq!(decode_client(&[OP_PING, 0]), Err(DecodeError::TrailingBytes));
        // Unknown value tag inside a map.
        assert_eq!(
            decode_client(&[OP_SET_ROOM_PROPS, 1, 1, 0, b'A', 9]),
            Err(DecodeError::UnknownTag(9))
        );
        // A map that promises two entries but carries one.
        assert_eq!(
            decode_client(&[OP_SET_ROOM_PROPS, 2, 1, 0, b'A', TAG_NULL]),
            Err(DecodeError::Truncated)
        );
        // Invalid UTF-8 in a string.
        assert_eq!(decode_client(&[OP_JOIN_ROOM, 1, 0, 0xff]), Err(DecodeError::BadUtf8));
    }

    #[test]
    fn duplicate_map_keys_collapse_last_writer_wins() {
        let bytes = vec![
            OP_SET_ROOM_PROPS, 2,
            1, 0, b'A', TAG_INT, 1, 0, 0, 0,
            1, 0, b'A', TAG_INT, 2, 0, 0, 0,
        ];
        let ClientMsg::SetRoomProps { props } = decode_client(&bytes).unwrap() else {
            panic!("expected SetRoomProps");
        };
        assert_eq!(props.len(), 1);
        assert_eq!(props.get_int("A"), Some(2));
    }

    #[test]
    fn map_set_is_last_writer_wins_in_place() {
        let mut map = Map::from_pairs(vec![("A", Value::Int(1)), ("B", Value::Int(2))]);
        map.set("A", Value::Int(9));
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        assert_eq!(map.get_int("A"), Some(9));
    }

    #[test]
    fn bool_payload_accepts_any_nonzero() {
        // Photon writes 0/1; be lenient reading, strict writing.
        assert_eq!(
            decode_server(&[OP_PLAYER_LEFT, 1, 0, 0, 0, 7]),
            Ok(ServerMsg::PlayerLeft { actor: 1, inactive: true })
        );
        assert_eq!(
            encode_server(&ServerMsg::PlayerLeft { actor: 1, inactive: true })[5],
            1
        );
    }
}

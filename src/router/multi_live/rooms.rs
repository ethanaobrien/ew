// Room/lobby registry for the multi-live relay, per docs/multi-live-ws-protocol.md.
//
// Concurrency shape (the spec's "Ordering & concurrency" section):
//
//   Every mutation goes through the single global REGISTRY mutex, and every outbound
//   message an operation produces is pushed into the recipients' per-connection FIFO
//   channels before that mutex is released. That is a total order over the whole relay,
//   which is a strict superset of the per-room total order the spec asks for, which is in
//   turn a strict superset of the per-sender ordering the live-start/live-end barriers
//   need. The critical sections never await and never block: the per-connection queues
//   are unbounded tokio mpsc senders, so pushing is a wait-free append, and the socket
//   writes happen in each connection's own writer task after the lock is gone.
//
//   Nothing in this file is async, and nothing here knows about WebSockets. That is what
//   makes the room semantics testable without a socket - the tests below drive the same
//   entry points ws.rs does and read the frames straight out of the channels.
//
// Time is always passed in as an explicit `now` so the 60s expiry rules can be tested
// without sleeping.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lazy_static::lazy_static;
use rand::RngExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::lock_onto_mutex;
use super::proto::{
    ClientMsg, Map, ServerMsg, Value,
    CLOSE_IDLE_TIMEOUT, ERR_GAME_CLOSED, ERR_GAME_DOES_NOT_EXIST, ERR_GAME_FULL,
    ERR_GAME_ID_ALREADY_EXISTS, ERR_NO_RANDOM_MATCH_FOUND, KICK_IDLE_TIMEOUT,
};

pub type ConnId = u64;

// Unclean disconnect holds the actor slot and its props this long (spec: 60s, sized for
// MultiRestart's ReconnectAndRejoin plus the MULTI_LIVE_TIME_OUT=15s ForceDisconnectWait).
pub const INACTIVE_TTL: Duration = Duration::from_secs(60);
// A room with zero ACTIVE players is destroyed after this long.
pub const EMPTY_ROOM_TTL: Duration = Duration::from_secs(60);

// How long a connection may go without saying ANYTHING before the relay stops believing
// in it. A client whose process is killed (or whose phone drops off the network) leaves a
// socket the OS never FINs, so the reader task sits in recv() forever: the actor stays
// ACTIVE, its seat is never converted into a held slot, INACTIVE_TTL never starts and
// EMPTY_ROOM_TTL never starts either — a zombie holds a quarter of a room for the life of
// the process. AUTH_TIMEOUT does not help; it only covers the first frame.
//
// The client sends op-12 Ping every 30s, so 90s is three missed pings — long enough that a
// stalled-but-alive connection is not punished for one lost packet, short enough that a
// dead one frees its seat before anybody waits long for a match.
pub const LIVENESS_TIMEOUT: Duration = Duration::from_secs(90);

// Room props the matcher reads. C1 is the party power, C0 the live level; both are set
// by the room creator and mirrored to everyone, so the relay needs no masterdata.
const PROP_ROOM_LEVEL: &str = "C0";
const PROP_ROOM_POWER: &str = "C1";

// MultiPlayProtocol.RoomConst.Token — "{userId}.{guid}", minted by the master client in
// MultiEventMatchingScene right before the live starts (CommitCurrentRoomToken +
// PushCurrentRoomData) and mirrored to the whole party. Every member posts it as the
// `token` field of /multi_live/start|end, so it is the only handle the HTTP side has on
// which room a finishing live belongs to. The relay never writes it, only reads it back.
const PROP_ROOM_TOKEN: &str = "C";

// Photon's well-known room properties are BYTE keys living in the same bag as the
// game's string keys; the client transport encodes a byte key as "#" + decimal.
// GamePropertyKey.IsOpen is 253 and IsVisible is 254, and the surviving Photon.Realtime
// `Room.IsOpen` / `Room.IsVisible` setters push exactly `{253: bool}` / `{254: bool}`
// through OpSetPropertiesOfRoom - which is how the game closes and hides a room after
// creation (MultiMatchingView.SetRoomOpenVisible / SetRoomOpenHide,
// MultiPlayManager.SendCurrentRoomIsOpen).
//
// The relay has to do BOTH things with them: mirror them in the bag like any other
// property (non-master clients poll their own CurrentRoom.IsOpen mirror to advance) AND
// interpret them, because JoinRandom and JoinRoom filter on the flags. Relaying them
// opaquely would let a closed room keep being matched into.
const PROP_ROOM_IS_OPEN: &str = "#253";
const PROP_ROOM_IS_VISIBLE: &str = "#254";

// Folds any "#253"/"#254" carried by a property update into the room's flags. Anything
// that is not a Bool - a missing key, a stray Int, a Null - leaves the flag alone.
fn apply_flag_props(props: &Map, visible: &mut bool, open: &mut bool) {
    if let Some(Value::Bool(value)) = props.get(PROP_ROOM_IS_OPEN) {
        *open = *value;
    }
    if let Some(Value::Bool(value)) = props.get(PROP_ROOM_IS_VISIBLE) {
        *visible = *value;
    }
}

// What a connection's writer task pulls out of its queue. Pong is the WebSocket-level
// keepalive reply (not the protocol's op 33) and rides the same queue so it cannot
// overtake a broadcast.
#[derive(Clone, Debug, PartialEq)]
pub enum Outbound {
    Msg(ServerMsg),
    Pong(Vec<u8>),
    Close(u16),
}

pub type Sink = UnboundedSender<Outbound>;

struct Conn {
    user_id: i64,
    tx: Sink,
    lobby: Option<String>,
    room: Option<String>,
    // When the last inbound frame of any kind arrived. Bumped by `touch`; read only by
    // the liveness sweep.
    last_seen: Instant,
}

struct Player {
    actor: i32,
    user_id: i64,
    // None once the socket dropped without LeaveRoom: the slot and props are held.
    conn: Option<ConnId>,
    props: Map,
    inactive_since: Option<Instant>,
}

impl Player {
    fn is_active(&self) -> bool {
        self.conn.is_some()
    }
}

struct Room {
    lobby: String,
    max_players: u8,
    visible: bool,
    open: bool,
    // Whether this room was created as a PRIVATE (join-by-code) party, latched once at
    // creation and never touched again. /multi_live/end branches the score-board write on
    // it, so it must NOT be re-derived from `visible` later: the game hides a room the
    // moment its members are decided (MultiMatchingView.SetRoomOpenVisible(false)), which
    // makes a public room mid-live indistinguishable from a private one by the live flags.
    //
    // The signal is the creation-time visibility, because that is exactly what separates
    // the client's two create paths: MultiPlayManager.CreatePublicRoom issues
    // CreateRoom("", 4, visible: true, open: true) and lets the server name the room, while
    // CreatePrivateRoom issues CreateRoom(code, 4, visible: false, open: true) — a private
    // party is by definition the one that is never listed for random matching.
    private: bool,
    props: Map,
    // Kept only so a future lobby-listing op can honour what the creator asked to
    // publish; the matcher reads C0/C1 straight off the room bag like Photon's SQL did.
    #[allow(dead_code)]
    lobby_prop_keys: Vec<String>,
    players: Vec<Player>,
    // Actor numbers are 1..n and never reused for the life of the room.
    next_actor: i32,
    master: i32,
    // Creation order, for JoinRandom's "then oldest" tiebreak.
    seq: u64,
    empty_since: Option<Instant>,
}

impl Room {
    fn active_count(&self) -> usize {
        self.players.iter().filter(|p| p.is_active()).count()
    }

    // Held slots count against capacity: an inactive player still owns its seat until the
    // 60s rejoin window closes, so a full-with-one-dropout room is still full.
    fn occupied(&self) -> usize {
        self.players.len()
    }

    fn is_full(&self) -> bool {
        // Photon treats maxPlayers 0 as "no limit".
        self.max_players != 0 && self.occupied() >= self.max_players as usize
    }

    fn player_by_conn(&self, conn: ConnId) -> Option<&Player> {
        self.players.iter().find(|p| p.conn == Some(conn))
    }

    fn player_by_conn_mut(&mut self, conn: ConnId) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.conn == Some(conn))
    }

    fn active_conns(&self) -> Vec<ConnId> {
        self.players.iter().filter_map(|p| p.conn).collect()
    }

    fn snapshot(&self) -> Vec<(i32, Map)> {
        // Includes held (inactive) slots: a fresh joiner has to see the seat as taken for
        // the later Rejoin's PlayerEntered to make sense, and PUN's Room.Players lists
        // inactive actors the same way.
        self.players.iter().map(|p| (p.actor, p.props.clone())).collect()
    }

    // Photon only moves the master when the current one stops being an active player;
    // a rejoining low-numbered actor does not steal it back.
    fn reelect_master(&mut self) -> Option<i32> {
        if self.players.iter().any(|p| p.actor == self.master && p.is_active()) {
            return None;
        }
        let lowest = self
            .players
            .iter()
            .filter(|p| p.is_active())
            .map(|p| p.actor)
            .min()?;
        if lowest == self.master {
            return None;
        }
        self.master = lowest;
        Some(lowest)
    }

    fn touch_empty(&mut self, now: Instant) {
        if self.active_count() == 0 {
            if self.empty_since.is_none() {
                self.empty_since = Some(now);
            }
        } else {
            self.empty_since = None;
        }
    }
}

pub struct Registry {
    next_conn: ConnId,
    next_seq: u64,
    conns: HashMap<ConnId, Conn>,
    rooms: HashMap<String, Room>,
}

lazy_static! {
    static ref REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
}

// The one lock. Held for the whole of an operation, released before any socket write.
pub fn registry() -> std::sync::MutexGuard<'static, Registry> {
    lock_onto_mutex!(REGISTRY)
}

// Server clock for AuthOk/Pong. Milliseconds since the epoch fits an f64 exactly for
// another 200-odd millennia.
pub fn server_time_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

type Outbox = Vec<(ConnId, ServerMsg)>;

impl Registry {
    fn new() -> Self {
        Registry {
            next_conn: 1,
            next_seq: 1,
            conns: HashMap::new(),
            rooms: HashMap::new(),
        }
    }

    // --- connection lifecycle -------------------------------------------------

    pub fn connect(&mut self, user_id: i64, tx: Sink, now: Instant) -> ConnId {
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(id, Conn { user_id, tx, lobby: None, room: None, last_seen: now });
        id
    }

    // "Something arrived on this socket." Called for every inbound frame, including the
    // ones that never reach `handle` (WebSocket-level ping/pong), which is what makes the
    // liveness window a statement about the SOCKET rather than about protocol traffic.
    pub fn touch(&mut self, id: ConnId, now: Instant) {
        if let Some(conn) = self.conns.get_mut(&id) {
            conn.last_seen = now;
        }
    }

    // Socket dropped without LeaveRoom: the actor goes inactive and keeps its slot and
    // props for INACTIVE_TTL so Rejoin can reclaim them.
    pub fn disconnect(&mut self, id: ConnId, now: Instant) {
        let Some(conn) = self.conns.remove(&id) else {
            return;
        };
        let Some(room_name) = conn.room else {
            println!("multi_live/ws: conn {} disconnected (uid {}, not in a room)", id, conn.user_id);
            return;
        };
        println!(
            "multi_live/ws: conn {} disconnected (uid {}, room '{}' — slot held for rejoin)",
            id, conn.user_id, room_name
        );
        let mut out: Outbox = Vec::new();
        if let Some(room) = self.rooms.get_mut(&room_name) {
            if let Some(player) = room.player_by_conn_mut(id) {
                player.conn = None;
                player.inactive_since = Some(now);
                let actor = player.actor;
                for target in room.active_conns() {
                    out.push((target, ServerMsg::PlayerLeft { actor, inactive: true }));
                }
                if let Some(master) = room.reelect_master() {
                    for target in room.active_conns() {
                        out.push((target, ServerMsg::MasterSwitched { new_master_actor: master }));
                    }
                }
            }
            room.touch_empty(now);
        }
        self.flush(out);
    }

    // --- dispatch -------------------------------------------------------------

    // Every op except Auth, which ws.rs handles before the connection is registered.
    pub fn handle(&mut self, id: ConnId, msg: ClientMsg, now: Instant) {
        // Any op at all is proof of life, op 12 Ping included — that is the whole point of
        // the client's 30s ping.
        self.touch(id, now);
        // Lobby/room lifecycle ops are logged for diagnosis; the chatty per-live traffic
        // (SetPlayerProps/Rpc/Ping) is deliberately not.
        match &msg {
            ClientMsg::JoinLobby { name } => println!("multi_live/ws: conn {} JoinLobby {}", id, name),
            ClientMsg::CreateRoom { name, .. } => println!("multi_live/ws: conn {} CreateRoom '{}'", id, name),
            ClientMsg::JoinRoom { name, .. } => println!("multi_live/ws: conn {} JoinRoom {}", id, name),
            ClientMsg::JoinRandom { power_min, power_max, .. } => {
                println!("multi_live/ws: conn {} JoinRandom power {}..{}", id, power_min, power_max)
            }
            ClientMsg::Rejoin { name, .. } => println!("multi_live/ws: conn {} Rejoin {}", id, name),
            ClientMsg::LeaveRoom => println!("multi_live/ws: conn {} LeaveRoom", id),
            // RPCs are rare (stamps, scene moves) — log the name, not the params.
            ClientMsg::Rpc { name, params } => {
                println!("multi_live/ws: conn {} Rpc {} ({} params)", id, name, params.len())
            }
            _ => {}
        }
        let mut out: Outbox = Vec::new();
        match msg {
            ClientMsg::Auth { .. } => {
                // ws.rs rejects a second Auth as a protocol error before we get here.
            }
            ClientMsg::JoinLobby { name } => {
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.lobby = Some(name);
                    out.push((id, ServerMsg::JoinedLobby));
                }
            }
            ClientMsg::LeaveLobby => {
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.lobby = None;
                    out.push((id, ServerMsg::LeftLobby));
                }
            }
            ClientMsg::CreateRoom { name, max_players, visible, open, props, lobby_prop_keys, player_props } => {
                self.create_room(id, name, max_players, visible, open, props, lobby_prop_keys, player_props, now, &mut out);
            }
            ClientMsg::JoinRoom { name, player_props } => self.join_room(id, &name, player_props, now, &mut out),
            ClientMsg::JoinRandom { power_min, power_max, levels, player_props } => {
                self.join_random(id, power_min, power_max, &levels, player_props, now, &mut out);
            }
            ClientMsg::LeaveRoom => self.leave_room(id, true, now, &mut out),
            ClientMsg::Rejoin { name, player_props } => self.rejoin(id, &name, player_props, now, &mut out),
            ClientMsg::SetPlayerProps { props } => self.set_player_props(id, props, &mut out),
            ClientMsg::SetRoomProps { props } => self.set_room_props(id, props, &mut out),
            ClientMsg::Rpc { name, params } => self.rpc(id, name, params, &mut out),
            ClientMsg::Ping => {
                out.push((id, ServerMsg::Pong { server_time_ms: server_time_ms() }));
            }
        }
        self.flush(out);
    }

    // --- room operations ------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn create_room(
        &mut self,
        id: ConnId,
        name: String,
        max_players: u8,
        visible: bool,
        open: bool,
        props: Map,
        lobby_prop_keys: Vec<String>,
        player_props: Map,
        now: Instant,
        out: &mut Outbox,
    ) {
        // A client that creates while still seated is a client bug; free the old seat
        // rather than leak it. The LeftRoom confirmation is suppressed because the
        // caller is about to get JoinedRoom or CreateFailed instead.
        self.leave_room(id, false, now, out);

        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let lobby = conn.lobby.clone().unwrap_or_default();
        let user_id = conn.user_id;

        let name = if name.is_empty() {
            match self.generate_room_name() {
                Some(name) => name,
                None => {
                    println!("multi_live/ws: could not find a free room id");
                    out.push((id, ServerMsg::CreateFailed {
                        code: ERR_GAME_ID_ALREADY_EXISTS,
                        msg: "GameIdAlreadyExists".to_string(),
                    }));
                    return;
                }
            }
        } else {
            name
        };

        if self.rooms.contains_key(&name) {
            out.push((id, ServerMsg::CreateFailed {
                code: ERR_GAME_ID_ALREADY_EXISTS,
                msg: "GameIdAlreadyExists".to_string(),
            }));
            return;
        }

        let seq = self.next_seq;
        self.next_seq += 1;
        // The transport seeds #253/#254 into the initial bag alongside the op-4 fields so
        // a later joiner's Room mirror starts with the right values. If the two ever
        // disagree the bag wins, because the bag is what every client polls.
        let (mut visible, mut open) = (visible, open);
        apply_flag_props(&props, &mut visible, &mut open);
        // Latched here, from the settled creation-time visibility, and never updated.
        let private = !visible;
        // The creator is master and takes actor 1.
        let room = Room {
            lobby,
            max_players,
            visible,
            open,
            private,
            props,
            lobby_prop_keys,
            // The creator has nobody to notify, but its bag is seeded from the op-4
            // playerProps all the same: the snapshot the NEXT joiner receives in
            // JoinedRoom must already carry it, which is what Photon's OpCreateRoom
            // actorProperties did.
            players: vec![Player {
                actor: 1,
                user_id,
                conn: Some(id),
                props: player_props,
                inactive_since: None,
            }],
            next_actor: 2,
            master: 1,
            seq,
            empty_since: None,
        };
        let joined = ServerMsg::JoinedRoom {
            room_name: name.clone(),
            your_actor: 1,
            master_actor: 1,
            room_props: room.props.clone(),
            players: room.snapshot(),
        };
        self.rooms.insert(name.clone(), room);
        if let Some(conn) = self.conns.get_mut(&id) {
            conn.room = Some(name);
        }
        out.push((id, joined));
    }

    fn generate_room_name(&self) -> Option<String> {
        let min = super::const_value("MULTI_ROOM_MIN_ID", 1000000);
        let max = super::const_value("MULTI_ROOM_MAX_ID", 99999999).max(min);
        let mut rng = rand::rng();
        for _ in 0..64 {
            let candidate = rng.random_range(min..max + 1).to_string();
            if !self.rooms.contains_key(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn join_room(&mut self, id: ConnId, name: &str, player_props: Map, now: Instant, out: &mut Outbox) {
        // Join-by-code is not lobby scoped: room names are globally unique, exactly like
        // a Photon room name inside one region.
        //
        // One account may hold several seats. Photon keyed a seat on the CONNECTION, not
        // on the account, so two clients signed in to one account were two players — and
        // that is what makes testing a party from one account possible here, so it is kept
        // deliberately. The consequence is that "full" is only ever about occupancy: a
        // second connection from the room's own creator is refused with GameFull when, and
        // only when, the room really is at maxPlayers (held slots included, see is_full).
        // GameFull is therefore truthful in every case — there is no self-join collision
        // dressed up as a full room.
        let Some(room) = self.rooms.get(name) else {
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "GameDoesNotExist".to_string(),
            }));
            return;
        };
        if !room.open {
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_GAME_CLOSED,
                msg: "GameClosed".to_string(),
            }));
            return;
        }
        if room.is_full() {
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_GAME_FULL,
                msg: "GameFull".to_string(),
            }));
            return;
        }
        self.seat(id, &name.to_string(), player_props, now, out);
    }

    fn join_random(
        &mut self,
        id: ConnId,
        power_min: i32,
        power_max: i32,
        levels: &[Value],
        player_props: Map,
        now: Instant,
        out: &mut Outbox,
    ) {
        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let lobby = conn.lobby.clone().unwrap_or_default();
        // Empty levels means no level filter - the helper lane is the only caller that
        // sends one.
        let levels: Vec<i32> = levels.iter().filter_map(Value::as_int).collect();

        let mut best: Option<(&String, &Room)> = None;
        for (name, room) in &self.rooms {
            if room.lobby != lobby || !room.visible || !room.open || room.is_full() {
                continue;
            }
            // Exclusive bounds on both ends, matching the official
            // `C1 > {min} AND C1 < {max}` SQL lobby filter.
            let Some(power) = room.props.get_int(PROP_ROOM_POWER) else {
                continue;
            };
            if power <= power_min || power >= power_max {
                continue;
            }
            if !levels.is_empty() {
                match room.props.get_int(PROP_ROOM_LEVEL) {
                    Some(level) if levels.contains(&level) => {}
                    _ => continue,
                }
            }
            // FillRoom: most populated first, then oldest.
            let better = match best {
                None => true,
                Some((_, current)) => {
                    (room.occupied(), std::cmp::Reverse(room.seq))
                        > (current.occupied(), std::cmp::Reverse(current.seq))
                }
            };
            if better {
                best = Some((name, room));
            }
        }

        let Some((name, _)) = best else {
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_NO_RANDOM_MATCH_FOUND,
                msg: "NoRandomMatchFound".to_string(),
            }));
            return;
        };
        let name = name.clone();
        self.seat(id, &name, player_props, now, out);
    }

    // Adds a fresh actor to an already-validated room. `player_props` is the joiner's own
    // bag off the seating op; it is seeded BEFORE the snapshot and the PlayerEntered
    // broadcast are built, so neither can ever show an empty newcomer.
    fn seat(&mut self, id: ConnId, name: &String, player_props: Map, now: Instant, out: &mut Outbox) {
        self.leave_room(id, false, now, out);

        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let user_id = conn.user_id;
        let Some(room) = self.rooms.get_mut(name) else {
            // The implicit leave above can destroy the room being joined if the caller
            // was its last active player.
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "GameDoesNotExist".to_string(),
            }));
            return;
        };

        let actor = room.next_actor;
        room.next_actor += 1;
        room.players.push(Player {
            actor,
            user_id,
            conn: Some(id),
            props: player_props,
            inactive_since: None,
        });
        room.touch_empty(now);
        // Cloned out of the seated Player so the snapshot below and this broadcast are
        // provably the same bag.
        let entered_props = room
            .players
            .last()
            .map(|p| p.props.clone())
            .unwrap_or_default();

        let others: Vec<ConnId> = room.active_conns().into_iter().filter(|c| *c != id).collect();
        let joined = ServerMsg::JoinedRoom {
            room_name: name.clone(),
            your_actor: actor,
            master_actor: room.master,
            room_props: room.props.clone(),
            players: room.snapshot(),
        };
        let master_change = room.reelect_master();
        let everyone = room.active_conns();

        if let Some(conn) = self.conns.get_mut(&id) {
            conn.room = Some(name.clone());
        }
        out.push((id, joined));
        for target in others {
            out.push((target, ServerMsg::PlayerEntered { actor, props: entered_props.clone() }));
        }
        if let Some(master) = master_change {
            for target in everyone {
                out.push((target, ServerMsg::MasterSwitched { new_master_actor: master }));
            }
        }
    }

    fn rejoin(&mut self, id: ConnId, name: &str, player_props: Map, now: Instant, out: &mut Outbox) {
        self.leave_room(id, false, now, out);

        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let user_id = conn.user_id;
        let Some(room) = self.rooms.get_mut(name) else {
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "GameDoesNotExist".to_string(),
            }));
            return;
        };
        // The held slot is matched on the account, not on the socket: the old socket is
        // gone by definition.
        let Some(player) = room
            .players
            .iter_mut()
            .find(|p| p.user_id == user_id && p.conn.is_none())
        else {
            // The spec's code list has no Photon JoinFailedWithRejoinerNotFound (32748);
            // the slot being gone is indistinguishable from the room being gone as far as
            // the client's retry loop cares, so it reuses GameDoesNotExist.
            out.push((id, ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "RejoinerNotFound".to_string(),
            }));
            return;
        };
        player.conn = Some(id);
        player.inactive_since = None;
        // Held props are restored by virtue of never having been dropped; the op-8 bag is
        // merged OVER them, join-time values winning per key. A rejoiner's F/LB/LC have
        // moved on since the drop, while keys it no longer writes keep their held values.
        player.props.merge(&player_props);
        let actor = player.actor;
        let props = player.props.clone();
        room.touch_empty(now);

        let others: Vec<ConnId> = room.active_conns().into_iter().filter(|c| *c != id).collect();
        let joined = ServerMsg::JoinedRoom {
            room_name: name.to_string(),
            your_actor: actor,
            master_actor: room.master,
            room_props: room.props.clone(),
            players: room.snapshot(),
        };
        let master_change = room.reelect_master();
        let everyone = room.active_conns();

        if let Some(conn) = self.conns.get_mut(&id) {
            conn.room = Some(name.to_string());
        }
        out.push((id, joined));
        for target in others {
            out.push((target, ServerMsg::PlayerEntered { actor, props: props.clone() }));
        }
        if let Some(master) = master_change {
            for target in everyone {
                out.push((target, ServerMsg::MasterSwitched { new_master_actor: master }));
            }
        }
    }

    // Clean leave: the slot is released outright, no rejoin window.
    fn leave_room(&mut self, id: ConnId, notify_self: bool, now: Instant, out: &mut Outbox) {
        let Some(conn) = self.conns.get_mut(&id) else {
            return;
        };
        let Some(room_name) = conn.room.take() else {
            return;
        };
        if notify_self {
            out.push((id, ServerMsg::LeftRoom));
        }
        let Some(room) = self.rooms.get_mut(&room_name) else {
            return;
        };
        let Some(index) = room.players.iter().position(|p| p.conn == Some(id)) else {
            return;
        };
        let actor = room.players.remove(index).actor;
        room.touch_empty(now);

        for target in room.active_conns() {
            out.push((target, ServerMsg::PlayerLeft { actor, inactive: false }));
        }
        // "Room is destroyed when the last ACTIVE player leaves cleanly" - held slots go
        // with it, deliberately.
        if room.active_count() == 0 {
            self.rooms.remove(&room_name);
            return;
        }
        if let Some(master) = room.reelect_master() {
            for target in room.active_conns() {
                out.push((target, ServerMsg::MasterSwitched { new_master_actor: master }));
            }
        }
    }

    fn set_player_props(&mut self, id: ConnId, props: Map, out: &mut Outbox) {
        let Some(room) = self.room_of(id) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room) else {
            return;
        };
        let Some(player) = room.player_by_conn_mut(id) else {
            return;
        };
        // Last-writer-wins per key, applied atomically as one message.
        for (key, value) in props.iter() {
            player.props.set(key, value.clone());
        }
        let actor = player.actor;
        // PUN semantics: the sender already applied these locally at send time, so the
        // broadcast carries the changed subset to everyone else only.
        for target in room.active_conns().into_iter().filter(|c| *c != id) {
            out.push((target, ServerMsg::PlayerPropsChanged { actor, props: props.clone() }));
        }
    }

    fn set_room_props(&mut self, id: ConnId, props: Map, out: &mut Outbox) {
        let Some(room) = self.room_of(id) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room) else {
            return;
        };
        if room.player_by_conn(id).is_none() {
            return;
        }
        for (key, value) in props.iter() {
            room.props.set(key, value.clone());
        }
        // The room stays open/visible per its own bag: the game flips these mid-session
        // (matching complete, live starting) and the matcher must follow.
        apply_flag_props(&props, &mut room.visible, &mut room.open);
        // Not master-gated: Photon did not enforce it either, the game gates on
        // IsMasterClient client-side.
        for target in room.active_conns().into_iter().filter(|c| *c != id) {
            out.push((target, ServerMsg::RoomPropsChanged { props: props.clone() }));
        }
    }

    fn rpc(&mut self, id: ConnId, name: String, params: Vec<Value>, out: &mut Outbox) {
        let Some(room) = self.room_of(id) else {
            return;
        };
        let Some(room) = self.rooms.get(&room) else {
            return;
        };
        let Some(player) = room.player_by_conn(id) else {
            return;
        };
        let sender_actor = player.actor;
        // AllViaServer: the sender executes on the echo, not at send time, so it is a
        // recipient too.
        for target in room.active_conns() {
            out.push((target, ServerMsg::Rpc {
                sender_actor,
                name: name.clone(),
                params: params.clone(),
            }));
        }
    }

    fn room_of(&self, id: ConnId) -> Option<String> {
        self.conns.get(&id).and_then(|c| c.room.clone())
    }

    // --- timers ---------------------------------------------------------------

    // Closes connections silent past LIVENESS_TIMEOUT, expires held slots past
    // INACTIVE_TTL and destroys rooms that have had no active player for EMPTY_ROOM_TTL.
    // Called once a second by the sweeper task.
    pub fn sweep(&mut self, now: Instant) {
        self.reap_idle_connections(now);
        let mut out: Outbox = Vec::new();
        for room in self.rooms.values_mut() {
            let expired: Vec<i32> = room
                .players
                .iter()
                .filter(|p| {
                    p.inactive_since
                        .is_some_and(|since| now.duration_since(since) >= INACTIVE_TTL)
                })
                .map(|p| p.actor)
                .collect();
            if expired.is_empty() {
                continue;
            }
            room.players.retain(|p| !expired.contains(&p.actor));
            // The rejoin window closed: the slot is gone for good, which is a plain
            // (non-inactive) leave as far as the remaining mirrors are concerned.
            for actor in expired {
                for target in room.active_conns() {
                    out.push((target, ServerMsg::PlayerLeft { actor, inactive: false }));
                }
            }
            if let Some(master) = room.reelect_master() {
                for target in room.active_conns() {
                    out.push((target, ServerMsg::MasterSwitched { new_master_actor: master }));
                }
            }
            room.touch_empty(now);
        }

        let dead: Vec<String> = self
            .rooms
            .iter()
            .filter(|(_, room)| {
                room.empty_since
                    .is_some_and(|since| now.duration_since(since) >= EMPTY_ROOM_TTL)
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in dead {
            self.rooms.remove(&name);
        }
        self.flush(out);
    }

    // A connection nothing has been heard from for LIVENESS_TIMEOUT is treated as gone:
    // it is told why, closed, and then run through the ORDINARY disconnect path, so its
    // actor goes inactive, its seat is held for the usual 60s rejoin window and the room
    // empties and dies on the usual timers. Nothing here is special-cased — a zombie is
    // just a client that dropped without the TCP layer ever admitting it.
    fn reap_idle_connections(&mut self, now: Instant) {
        let idle: Vec<ConnId> = self
            .conns
            .iter()
            .filter(|(_, conn)| now.duration_since(conn.last_seen) >= LIVENESS_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        for id in idle {
            println!(
                "multi_live/ws: conn {} silent for {}s — closing (liveness)",
                id,
                LIVENESS_TIMEOUT.as_secs()
            );
            // Queued before disconnect, which drops the connection: the writer task closes
            // the socket on Close and its reader's stream then ends, so the reader's own
            // disconnect call finds the connection already gone and does nothing.
            self.send(id, Outbound::Msg(ServerMsg::Kicked { cause: KICK_IDLE_TIMEOUT }));
            self.send(id, Outbound::Close(CLOSE_IDLE_TIMEOUT));
            self.disconnect(id, now);
        }
    }

    // --- plumbing -------------------------------------------------------------

    // Still under the registry lock: pushing into an unbounded channel cannot block, so
    // the whole broadcast lands in FIFO order before any other operation can interleave.
    fn flush(&mut self, out: Outbox) {
        for (target, msg) in out {
            self.send(target, Outbound::Msg(msg));
        }
    }

    pub fn send(&mut self, target: ConnId, msg: Outbound) {
        if let Some(conn) = self.conns.get(&target) {
            // A send error only means the writer task is already gone; the disconnect
            // path will clean the connection up.
            let _ = conn.tx.send(msg);
        }
    }

    // --- introspection (tests, and a future webui panel) ----------------------

    // Whether the room carrying this live token is a private (join-by-code) party, or None
    // when no live room claims the token.
    //
    // The token is matched against the ROOM bag rather than against the poster's account:
    // all up to four party members post the same token, and only the master ever wrote it,
    // so the bag is the one place the HTTP and relay halves meet. Room names are globally
    // unique but a token is not addressable by name, so this is a scan — the registry holds
    // at most a handful of live rooms and this runs once per /multi_live/end.
    pub fn token_room_is_private(&self, token: &str) -> Option<bool> {
        if token.is_empty() {
            // An unset room prop reads back as "" on the client, which would otherwise
            // match every room that never had a token pushed.
            return None;
        }
        self.rooms
            .values()
            .find(|room| room.props.get_str(PROP_ROOM_TOKEN) == Some(token))
            .map(|room| room.private)
    }

    #[allow(dead_code)]
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    #[allow(dead_code)]
    pub fn connection_count(&self) -> usize {
        self.conns.len()
    }
}

// What /multi_live/start asks, while the room is certainly still alive: is this a private
// party, or does no live room claim the token at all? The distinction matters to the
// caller — an answer is recorded on the start record, a miss is not (see
// multi_live::record_room_privacy).
pub fn token_room_privacy(token: &str) -> Option<bool> {
    registry().token_room_is_private(token)
}

// The same question with the miss folded away, for a start record from before the answer
// was recorded at start time.
//
// A token no live room claims answers `false`. The room is torn down as soon as its last
// active player leaves cleanly, and an end can arrive after that (a client that quit the
// room before its POST landed, or a server restart), so "unknown" has to resolve to
// something — and public is the official behaviour, which is the safe side to be wrong on.
pub fn token_is_private_room(token: &str) -> bool {
    token_room_privacy(token).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    // Each test drives its own Registry rather than the global one, so they can run in
    // parallel without sharing rooms.
    struct Harness {
        reg: Registry,
        now: Instant,
        rx: HashMap<ConnId, UnboundedReceiver<Outbound>>,
    }

    impl Harness {
        fn new() -> Self {
            Harness { reg: Registry::new(), now: Instant::now(), rx: HashMap::new() }
        }

        fn connect(&mut self, user_id: i64) -> ConnId {
            let (tx, rx) = unbounded_channel();
            let id = self.reg.connect(user_id, tx, self.now);
            self.rx.insert(id, rx);
            id
        }

        fn send(&mut self, id: ConnId, msg: ClientMsg) {
            self.reg.handle(id, msg, self.now);
        }

        fn advance(&mut self, by: Duration) {
            self.now += by;
        }

        fn sweep(&mut self) {
            self.reg.sweep(self.now);
        }

        // Everything queued for this connection since the last drain, closes and pongs
        // included — the liveness sweep's answer is a Kicked followed by a Close, and
        // `drain` throws both away.
        fn drain_raw(&mut self, id: ConnId) -> Vec<Outbound> {
            let mut out = Vec::new();
            let rx = self.rx.get_mut(&id).expect("known connection");
            while let Ok(msg) = rx.try_recv() {
                out.push(msg);
            }
            out
        }

        // Everything queued for this connection since the last drain.
        fn drain(&mut self, id: ConnId) -> Vec<ServerMsg> {
            let mut out = Vec::new();
            let rx = self.rx.get_mut(&id).expect("known connection");
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    Outbound::Msg(msg) => out.push(msg),
                    Outbound::Pong(_) | Outbound::Close(_) => {}
                }
            }
            out
        }

        fn clear(&mut self, ids: &[ConnId]) {
            for id in ids {
                let _ = self.drain(*id);
            }
        }

        fn create(&mut self, id: ConnId, name: &str, props: Vec<(&str, Value)>) {
            self.send(id, ClientMsg::CreateRoom {
                name: name.to_string(),
                max_players: 4,
                visible: true,
                open: true,
                props: Map::from_pairs(props),
                lobby_prop_keys: vec![],
                player_props: Map::new(),
            });
        }
    }

    fn joined_room(msgs: &[ServerMsg]) -> (String, i32, i32, Vec<(i32, Map)>) {
        for msg in msgs {
            if let ServerMsg::JoinedRoom { room_name, your_actor, master_actor, players, .. } = msg {
                return (room_name.clone(), *your_actor, *master_actor, players.clone());
            }
        }
        panic!("expected JoinedRoom in {:?}", msgs);
    }

    #[test]
    fn create_seats_the_creator_as_master_actor_one() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.send(a, ClientMsg::JoinLobby { name: "MultiEventLobby_31".into() });
        assert_eq!(h.drain(a), vec![ServerMsg::JoinedLobby]);

        h.create(a, "room", vec![]);
        let (name, actor, master, players) = joined_room(&h.drain(a));
        assert_eq!(name, "room");
        assert_eq!(actor, 1);
        assert_eq!(master, 1);
        assert_eq!(players.len(), 1);
        assert_eq!(h.reg.room_count(), 1);
    }

    #[test]
    fn empty_create_name_generates_one_in_the_official_range() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.send(a, ClientMsg::CreateRoom {
            name: String::new(),
            max_players: 4,
            visible: true,
            open: true,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        let (name, _, _, _) = joined_room(&h.drain(a));
        let id: i64 = name.parse().expect("generated room name is numeric");
        assert!((1000000..=99999999).contains(&id), "{} out of range", id);
    }

    #[test]
    fn duplicate_room_name_fails_with_photon_code() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "dup", vec![]);
        h.create(b, "dup", vec![]);
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::CreateFailed {
                code: ERR_GAME_ID_ALREADY_EXISTS,
                msg: "GameIdAlreadyExists".into(),
            }]
        );
    }

    #[test]
    fn join_reports_absent_full_and_closed_rooms() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);

        h.send(b, ClientMsg::JoinRoom { name: "nope".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "GameDoesNotExist".into(),
            }]
        );

        h.send(a, ClientMsg::CreateRoom {
            name: "closed".into(),
            max_players: 4,
            visible: true,
            open: false,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.send(b, ClientMsg::JoinRoom { name: "closed".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_CLOSED, msg: "GameClosed".into() }]
        );

        let c = h.connect(3);
        h.send(c, ClientMsg::CreateRoom {
            name: "solo".into(),
            max_players: 1,
            visible: true,
            open: true,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.send(b, ClientMsg::JoinRoom { name: "solo".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_FULL, msg: "GameFull".into() }]
        );
    }

    #[test]
    fn joiners_get_a_snapshot_and_everyone_else_gets_player_entered() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.create(a, "room", vec![("C0", Value::Int(4))]);
        h.clear(&[a]);

        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let (_, actor_b, master, players) = joined_room(&h.drain(b));
        assert_eq!(actor_b, 2);
        assert_eq!(master, 1);
        assert_eq!(players.iter().map(|(a, _)| *a).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            h.drain(a),
            vec![ServerMsg::PlayerEntered { actor: 2, props: Map::new() }]
        );

        h.send(c, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let (_, actor_c, _, _) = joined_room(&h.drain(c));
        assert_eq!(actor_c, 3);
        assert_eq!(
            h.drain(a),
            vec![ServerMsg::PlayerEntered { actor: 3, props: Map::new() }]
        );
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::PlayerEntered { actor: 3, props: Map::new() }]
        );
    }

    // One account, several connections, several seats — kept deliberately so a party can
    // be tested from a single account (Photon keyed a seat on the connection too). The
    // only thing that can refuse the second connection is real occupancy, so GameFull
    // never stands in for a self-join collision.
    #[test]
    fn one_account_may_take_several_seats() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let a2 = h.connect(1);
        h.create(a, "room", vec![]);
        h.clear(&[a]);

        h.send(a2, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let (_, actor, master, players) = joined_room(&h.drain(a2));
        assert_eq!(actor, 2, "the creator's own account gets a second actor");
        assert_eq!(master, 1);
        assert_eq!(players.len(), 2);

        // ...until the room is genuinely at capacity, which is what GameFull means.
        let c = h.connect(1);
        h.send(c, ClientMsg::CreateRoom {
            name: "solo".into(),
            max_players: 1,
            visible: true,
            open: true,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        let c2 = h.connect(1);
        h.send(c2, ClientMsg::JoinRoom { name: "solo".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(c2),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_FULL, msg: "GameFull".into() }]
        );
    }

    #[test]
    fn actor_numbers_are_never_reused() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.send(b, ClientMsg::LeaveRoom);
        h.clear(&[a, b]);

        h.send(c, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let (_, actor_c, _, _) = joined_room(&h.drain(c));
        assert_eq!(actor_c, 3, "actor 2 must not be handed out twice");
    }

    #[test]
    fn clean_leave_hands_the_master_to_the_lowest_remaining_actor() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.send(c, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b, c]);

        h.send(a, ClientMsg::LeaveRoom);
        assert_eq!(h.drain(a), vec![ServerMsg::LeftRoom]);
        let expected = vec![
            ServerMsg::PlayerLeft { actor: 1, inactive: false },
            ServerMsg::MasterSwitched { new_master_actor: 2 },
        ];
        assert_eq!(h.drain(b), expected);
        assert_eq!(h.drain(c), expected);
    }

    #[test]
    fn last_active_clean_leave_destroys_the_room() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.create(a, "room", vec![]);
        h.clear(&[a]);
        h.send(a, ClientMsg::LeaveRoom);
        assert_eq!(h.drain(a), vec![ServerMsg::LeftRoom]);
        assert_eq!(h.reg.room_count(), 0);
    }

    #[test]
    fn unclean_disconnect_holds_the_slot_then_expires_it() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        let now = h.now;
        h.reg.disconnect(a, now);
        assert_eq!(
            h.drain(b),
            vec![
                ServerMsg::PlayerLeft { actor: 1, inactive: true },
                ServerMsg::MasterSwitched { new_master_actor: 2 },
            ]
        );
        assert_eq!(h.reg.room_count(), 1);

        // Still inside the window.
        h.advance(Duration::from_secs(59));
        h.sweep();
        assert_eq!(h.drain(b), vec![]);

        // Window closed: the seat is released for good.
        h.advance(Duration::from_secs(2));
        h.sweep();
        assert_eq!(h.drain(b), vec![ServerMsg::PlayerLeft { actor: 1, inactive: false }]);
        assert_eq!(h.reg.room_count(), 1);
    }

    #[test]
    fn rejoin_restores_the_held_actor_and_props() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.send(a, ClientMsg::SetPlayerProps {
            props: Map::from_pairs(vec![("A", Value::Int(42))]),
        });
        h.clear(&[a, b]);

        let now = h.now;
        h.reg.disconnect(a, now);
        h.clear(&[b]);

        // Reconnect: a new socket for the same account reclaims actor 1 with its props.
        let a2 = h.connect(1);
        h.send(a2, ClientMsg::Rejoin { name: "room".into(), player_props: Map::new() });
        let (_, actor, master, players) = joined_room(&h.drain(a2));
        assert_eq!(actor, 1);
        // Mastership stays with actor 2: Photon does not hand it back on rejoin.
        assert_eq!(master, 2);
        assert_eq!(players.len(), 2);
        assert_eq!(players[0].1.get_int("A"), Some(42));
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::PlayerEntered {
                actor: 1,
                props: Map::from_pairs(vec![("A", Value::Int(42))]),
            }]
        );
    }

    // --- join-time player properties (docs "Player properties at join time") -------
    //
    // Photon's OpJoinRoom carried actorProperties, so a peer's OnPlayerEnteredRoom fired
    // with the joiner's bag already populated and the official client parses it right
    // there (MemberContent.InitializePlayer -> uint.Parse on player key "B"). An empty
    // bag is an ArgumentNullException on every existing client, not a cosmetic delay.

    fn player_bag() -> Map {
        Map::from_pairs(vec![
            ("A", Value::Str("Nozomi".into())),
            ("B", Value::Str("1001".into())),
            ("E", Value::Int(120)),
        ])
    }

    #[test]
    fn player_entered_carries_the_joiners_props() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.clear(&[a]);

        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: player_bag() });

        // The sitting client must see the real bag on the entry event itself.
        assert_eq!(
            h.drain(a),
            vec![ServerMsg::PlayerEntered { actor: 2, props: player_bag() }]
        );
    }

    #[test]
    fn the_joined_room_snapshot_agrees_with_player_entered() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);

        // The creator's own bag is seeded too, so it is correct in the snapshot that a
        // LATER joiner receives.
        h.send(a, ClientMsg::CreateRoom {
            name: "room".into(),
            max_players: 4,
            visible: true,
            open: true,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::from_pairs(vec![("B", Value::Str("7".into()))]),
        });
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: player_bag() });
        h.clear(&[a, b]);

        h.send(c, ClientMsg::JoinRoom {
            name: "room".into(),
            player_props: Map::from_pairs(vec![("B", Value::Str("9".into()))]),
        });

        let (_, actor, _, players) = joined_room(&h.drain(c));
        assert_eq!(actor, 3);
        assert_eq!(players.len(), 3);
        // Nobody in the snapshot has an empty bag, including the newcomer itself.
        assert_eq!(players[0].1.get("B"), Some(&Value::Str("7".into())));
        assert_eq!(players[1].1, player_bag());
        assert_eq!(players[2].1.get("B"), Some(&Value::Str("9".into())));

        // ...and what the sitting clients were told matches that snapshot exactly.
        let entered = ServerMsg::PlayerEntered {
            actor: 3,
            props: Map::from_pairs(vec![("B", Value::Str("9".into()))]),
        };
        assert_eq!(h.drain(a), vec![entered.clone()]);
        assert_eq!(h.drain(b), vec![entered]);
    }

    #[test]
    fn join_random_and_create_seed_the_bag_too() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.send(a, ClientMsg::JoinLobby { name: "L".into() });
        h.send(b, ClientMsg::JoinLobby { name: "L".into() });
        h.create(a, "room", vec![("C1", Value::Int(5000)), ("C0", Value::Int(4))]);
        h.clear(&[a, b]);

        h.send(b, ClientMsg::JoinRandom {
            power_min: 0,
            power_max: 10000,
            levels: vec![Value::Int(4)],
            player_props: player_bag(),
        });

        assert_eq!(
            h.drain(a),
            vec![ServerMsg::PlayerEntered { actor: 2, props: player_bag() }]
        );
    }

    #[test]
    fn an_empty_join_bag_leaves_the_actor_empty() {
        // A client that has committed nothing yet is legal and must not be special-cased.
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.clear(&[a]);

        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(a),
            vec![ServerMsg::PlayerEntered { actor: 2, props: Map::new() }]
        );
    }

    #[test]
    fn rejoin_merges_the_fresh_bag_over_the_held_one() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        // Actor 1's state before the drop: a name, a title and a status.
        h.send(a, ClientMsg::SetPlayerProps {
            props: Map::from_pairs(vec![
                ("A", Value::Str("Nozomi".into())),
                ("B", Value::Str("1001".into())),
                ("F", Value::Int(1)),
            ]),
        });
        h.clear(&[a, b]);

        let now = h.now;
        h.reg.disconnect(a, now);
        h.clear(&[b]);

        // The rejoiner's status has moved on; its name is unchanged and it no longer
        // writes the title, which must therefore survive from the held bag.
        let a2 = h.connect(1);
        h.send(a2, ClientMsg::Rejoin {
            name: "room".into(),
            player_props: Map::from_pairs(vec![("F", Value::Int(5))]),
        });

        let merged = Map::from_pairs(vec![
            ("A", Value::Str("Nozomi".into())),
            ("B", Value::Str("1001".into())),
            ("F", Value::Int(5)),
        ]);
        let (_, actor, _, players) = joined_room(&h.drain(a2));
        assert_eq!(actor, 1);
        assert_eq!(players[0].1, merged);
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::PlayerEntered { actor: 1, props: merged }]
        );
    }

    #[test]
    fn rejoin_after_the_window_or_into_nothing_fails() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        let now = h.now;
        h.reg.disconnect(a, now);
        h.advance(Duration::from_secs(61));
        h.sweep();

        let a2 = h.connect(1);
        h.send(a2, ClientMsg::Rejoin { name: "room".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(a2),
            vec![ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "RejoinerNotFound".into(),
            }]
        );

        let c = h.connect(9);
        h.send(c, ClientMsg::Rejoin { name: "gone".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(c),
            vec![ServerMsg::JoinFailed {
                code: ERR_GAME_DOES_NOT_EXIST,
                msg: "GameDoesNotExist".into(),
            }]
        );
    }

    #[test]
    fn rejoin_into_an_emptied_room_takes_the_master() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        let now = h.now;
        h.reg.disconnect(b, now);
        h.reg.disconnect(a, now);
        assert_eq!(h.reg.room_count(), 1);

        let a2 = h.connect(1);
        h.send(a2, ClientMsg::Rejoin { name: "room".into(), player_props: Map::new() });
        let (_, actor, master, _) = joined_room(&h.drain(a2));
        assert_eq!(actor, 1);
        assert_eq!(master, 1);
    }

    // --- liveness (the zombie-socket sweep) ----------------------------------------
    //
    // A killed client leaves a socket the OS never FINs: the reader task never returns,
    // so disconnect is never called and the seat is held ACTIVE forever. These pin the
    // sweep that ends that, and pin that it ends it through the ORDINARY disconnect path.

    #[test]
    fn a_silent_connection_is_closed_and_its_seat_falls_to_the_rejoin_window() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        // 45s of silence from both, then b pings (op 12) and a does not.
        h.advance(Duration::from_secs(45));
        h.send(b, ClientMsg::Ping);
        h.sweep();
        assert_eq!(h.drain_raw(a), vec![], "45s is well inside the window");

        // a is now 91s silent, b only 46s.
        h.advance(Duration::from_secs(46));
        h.clear(&[b]);
        h.sweep();

        // a is told why and closed...
        assert_eq!(
            h.drain_raw(a),
            vec![
                Outbound::Msg(ServerMsg::Kicked { cause: KICK_IDLE_TIMEOUT }),
                Outbound::Close(CLOSE_IDLE_TIMEOUT),
            ]
        );
        // ...and the room saw exactly what it sees for any unclean drop: a held slot and
        // a new master. Nothing about the zombie is special-cased.
        assert_eq!(
            h.drain(b),
            vec![
                ServerMsg::PlayerLeft { actor: 1, inactive: true },
                ServerMsg::MasterSwitched { new_master_actor: 2 },
            ]
        );
        assert_eq!(h.reg.room_count(), 1);
        assert_eq!(h.reg.connection_count(), 1, "the dead connection is deregistered");

        // And the seat then expires on the ordinary rejoin timer.
        h.advance(INACTIVE_TTL + Duration::from_secs(1));
        h.send(b, ClientMsg::Ping);
        h.clear(&[b]);
        h.sweep();
        assert_eq!(h.drain(b), vec![ServerMsg::PlayerLeft { actor: 1, inactive: false }]);
    }

    #[test]
    fn a_pinging_connection_is_never_reaped() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.create(a, "room", vec![]);
        h.clear(&[a]);

        // The client's 30s ping, for ten minutes of a long live.
        for _ in 0..20 {
            h.advance(Duration::from_secs(30));
            h.send(a, ClientMsg::Ping);
            h.sweep();
        }
        assert_eq!(h.reg.connection_count(), 1);
        assert_eq!(h.reg.room_count(), 1);
        // Nothing but the pongs it asked for.
        let out = h.drain_raw(a);
        assert_eq!(out.len(), 20);
        assert!(
            out.iter().all(|m| matches!(m, Outbound::Msg(ServerMsg::Pong { .. }))),
            "a live connection must see no Kicked and no Close: {:?}",
            out
        );
    }

    #[test]
    fn a_silent_connection_in_no_room_is_reaped_too() {
        // Sockets that authenticated and then went quiet in the lobby leak just as well.
        let mut h = Harness::new();
        let a = h.connect(1);
        h.send(a, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[a]);

        h.advance(LIVENESS_TIMEOUT + Duration::from_secs(1));
        h.sweep();
        assert_eq!(h.reg.connection_count(), 0);
        assert_eq!(h.drain_raw(a), vec![
            Outbound::Msg(ServerMsg::Kicked { cause: KICK_IDLE_TIMEOUT }),
            Outbound::Close(CLOSE_IDLE_TIMEOUT),
        ]);
    }

    // Any inbound traffic counts, not just Ping: a client mid-live is writing properties
    // several times a second and must never be reaped for not pinging on top of that.
    #[test]
    fn ordinary_traffic_keeps_a_connection_alive() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.create(a, "room", vec![]);
        h.clear(&[a]);

        for _ in 0..4 {
            h.advance(Duration::from_secs(60));
            h.send(a, ClientMsg::SetPlayerProps {
                props: Map::from_pairs(vec![("LB", Value::Int(7))]),
            });
            h.sweep();
        }
        assert_eq!(h.reg.connection_count(), 1);
        assert_eq!(h.drain_raw(a), vec![]);
    }

    #[test]
    fn a_room_with_no_active_players_dies_after_sixty_seconds() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.create(a, "room", vec![]);
        let now = h.now;
        h.reg.disconnect(a, now);
        assert_eq!(h.reg.room_count(), 1);

        h.advance(Duration::from_secs(59));
        h.sweep();
        assert_eq!(h.reg.room_count(), 1);

        h.advance(Duration::from_secs(2));
        h.sweep();
        assert_eq!(h.reg.room_count(), 0);
    }

    #[test]
    fn player_props_merge_and_broadcast_the_changed_subset_to_others() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        h.send(a, ClientMsg::SetPlayerProps {
            props: Map::from_pairs(vec![("A", Value::Int(1)), ("B", Value::Str("x".into()))]),
        });
        // The sender applied it locally at send time; no echo.
        assert_eq!(h.drain(a), vec![]);
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::PlayerPropsChanged {
                actor: 1,
                props: Map::from_pairs(vec![("A", Value::Int(1)), ("B", Value::Str("x".into()))]),
            }]
        );

        // A second write merges: only the touched key travels, the bag keeps both.
        h.send(a, ClientMsg::SetPlayerProps {
            props: Map::from_pairs(vec![("A", Value::Int(2))]),
        });
        assert_eq!(
            h.drain(b),
            vec![ServerMsg::PlayerPropsChanged {
                actor: 1,
                props: Map::from_pairs(vec![("A", Value::Int(2))]),
            }]
        );

        let c = h.connect(3);
        h.send(c, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let (_, _, _, players) = joined_room(&h.drain(c));
        let bag = &players.iter().find(|(actor, _)| *actor == 1).unwrap().1;
        assert_eq!(bag.get_int("A"), Some(2));
        assert_eq!(bag.get("B"), Some(&Value::Str("x".into())));
    }

    #[test]
    fn room_props_merge_and_are_not_master_gated() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![("C0", Value::Int(4)), ("C1", Value::Int(50000))]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        // Actor 2 is not the master and is still allowed to write.
        h.send(b, ClientMsg::SetRoomProps {
            props: Map::from_pairs(vec![("A", Value::Int(7))]),
        });
        assert_eq!(h.drain(b), vec![]);
        assert_eq!(
            h.drain(a),
            vec![ServerMsg::RoomPropsChanged {
                props: Map::from_pairs(vec![("A", Value::Int(7))]),
            }]
        );

        let c = h.connect(3);
        h.send(c, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let msgs = h.drain(c);
        let ServerMsg::JoinedRoom { room_props, .. } = &msgs[0] else {
            panic!("expected JoinedRoom");
        };
        assert_eq!(room_props.get_int("C0"), Some(4));
        assert_eq!(room_props.get_int("C1"), Some(50000));
        assert_eq!(room_props.get_int("A"), Some(7));
    }

    #[test]
    fn rpc_echoes_to_every_active_player_including_the_sender() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b]);

        h.send(b, ClientMsg::Rpc {
            name: "SendStamp".into(),
            params: vec![Value::Int(5)],
        });
        let expected = vec![ServerMsg::Rpc {
            sender_actor: 2,
            name: "SendStamp".into(),
            params: vec![Value::Int(5)],
        }];
        assert_eq!(h.drain(a), expected);
        assert_eq!(h.drain(b), expected, "AllViaServer: the sender runs on the echo");
    }

    #[test]
    fn one_senders_updates_keep_their_order_when_interleaved() {
        // The live-start/live-end barriers poll mirrors of remote properties; two writes
        // from one sender arriving out of order deadlocks them. Interleave three senders
        // and assert each receiver still sees every sender's writes in send order.
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.create(a, "room", vec![]);
        h.send(b, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.send(c, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[a, b, c]);

        for step in 0..20 {
            for (conn, actor) in [(a, 1), (b, 2), (c, 3)] {
                h.send(conn, ClientMsg::SetPlayerProps {
                    props: Map::from_pairs(vec![("LB", Value::Int(step * 10 + actor))]),
                });
            }
            // A room prop and an RPC in the middle of the stream must not reorder it.
            h.send(a, ClientMsg::SetRoomProps {
                props: Map::from_pairs(vec![("A", Value::Int(step))]),
            });
            h.send(b, ClientMsg::Rpc { name: "_CountStop".into(), params: vec![] });
        }

        for receiver in [a, b, c] {
            let msgs = h.drain(receiver);
            for (actor, offset) in [(1, 1), (2, 2), (3, 3)] {
                let seen: Vec<i32> = msgs
                    .iter()
                    .filter_map(|m| match m {
                        ServerMsg::PlayerPropsChanged { actor: got, props } if *got == actor => {
                            props.get_int("LB")
                        }
                        _ => None,
                    })
                    .collect();
                let expected: Vec<i32> = (0..20).map(|step| step * 10 + offset).collect();
                if receiver == [a, b, c][(actor - 1) as usize] {
                    // Own writes are never echoed back.
                    assert!(seen.is_empty(), "actor {} saw its own props", actor);
                } else {
                    assert_eq!(seen, expected, "actor {} order seen by {}", actor, receiver);
                }
            }
            let room_props: Vec<i32> = msgs
                .iter()
                .filter_map(|m| match m {
                    ServerMsg::RoomPropsChanged { props } => props.get_int("A"),
                    _ => None,
                })
                .collect();
            if receiver == a {
                assert!(room_props.is_empty());
            } else {
                assert_eq!(room_props, (0..20).collect::<Vec<i32>>());
            }
        }
    }

    // --- JoinRandom -----------------------------------------------------------

    fn lobby_room(h: &mut Harness, user: i64, lobby: &str, name: &str, level: i32, power: i32) -> ConnId {
        let id = h.connect(user);
        h.send(id, ClientMsg::JoinLobby { name: lobby.to_string() });
        h.create(id, name, vec![("C0", Value::Int(level)), ("C1", Value::Int(power))]);
        h.clear(&[id]);
        id
    }

    fn join_random(h: &mut Harness, id: ConnId, min: i32, max: i32, levels: &[i32]) -> Vec<ServerMsg> {
        h.send(id, ClientMsg::JoinRandom {
            power_min: min,
            power_max: max,
            levels: levels.iter().map(|l| Value::Int(*l)).collect(),
            player_props: Map::new(),
        });
        h.drain(id)
    }

    #[test]
    fn join_random_power_window_is_exclusive_at_both_ends() {
        let mut h = Harness::new();
        lobby_room(&mut h, 1, "L", "exact_low", 4, 100);
        lobby_room(&mut h, 2, "L", "exact_high", 4, 200);

        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        // Both rooms sit exactly on a bound, which the official SQL excludes.
        let msgs = join_random(&mut h, seeker, 100, 200, &[]);
        assert_eq!(
            msgs,
            vec![ServerMsg::JoinFailed {
                code: ERR_NO_RANDOM_MATCH_FOUND,
                msg: "NoRandomMatchFound".into(),
            }]
        );

        // Widen by one on each side and both become eligible.
        let msgs = join_random(&mut h, seeker, 99, 201, &[]);
        let (name, _, _, _) = joined_room(&msgs);
        assert!(name == "exact_low" || name == "exact_high");
    }

    #[test]
    fn join_random_honours_the_level_filter_and_empty_means_any() {
        let mut h = Harness::new();
        lobby_room(&mut h, 1, "L", "hard", 4, 500);

        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        let msgs = join_random(&mut h, seeker, 0, 1000, &[1, 2]);
        assert!(matches!(msgs[0], ServerMsg::JoinFailed { code: ERR_NO_RANDOM_MATCH_FOUND, .. }));

        let msgs = join_random(&mut h, seeker, 0, 1000, &[3, 4]);
        assert_eq!(joined_room(&msgs).0, "hard");
        h.send(seeker, ClientMsg::LeaveRoom);
        h.clear(&[seeker]);

        // The helper lane sends no levels at all: everything matches.
        let msgs = join_random(&mut h, seeker, 0, 1000, &[]);
        assert_eq!(joined_room(&msgs).0, "hard");
    }

    #[test]
    fn join_random_skips_invisible_closed_full_and_other_lobbies() {
        let mut h = Harness::new();
        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        // Wrong lobby.
        lobby_room(&mut h, 1, "OTHER", "elsewhere", 4, 500);

        // Invisible (a private room joined by its 6 character code).
        let priv_host = h.connect(2);
        h.send(priv_host, ClientMsg::JoinLobby { name: "L".into() });
        h.send(priv_host, ClientMsg::CreateRoom {
            name: "042719".into(),
            max_players: 4,
            visible: false,
            open: true,
            props: Map::from_pairs(vec![("C0", Value::Int(4)), ("C1", Value::Int(500))]),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.clear(&[priv_host]);

        // Closed.
        let closed_host = h.connect(3);
        h.send(closed_host, ClientMsg::JoinLobby { name: "L".into() });
        h.send(closed_host, ClientMsg::CreateRoom {
            name: "closed".into(),
            max_players: 4,
            visible: true,
            open: false,
            props: Map::from_pairs(vec![("C0", Value::Int(4)), ("C1", Value::Int(500))]),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.clear(&[closed_host]);

        // Full.
        let full_host = h.connect(4);
        h.send(full_host, ClientMsg::JoinLobby { name: "L".into() });
        h.send(full_host, ClientMsg::CreateRoom {
            name: "full".into(),
            max_players: 1,
            visible: true,
            open: true,
            props: Map::from_pairs(vec![("C0", Value::Int(4)), ("C1", Value::Int(500))]),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.clear(&[full_host]);

        // A room with no C1 at all cannot satisfy the power window.
        let bare_host = h.connect(5);
        h.send(bare_host, ClientMsg::JoinLobby { name: "L".into() });
        h.create(bare_host, "bare", vec![("C0", Value::Int(4))]);
        h.clear(&[bare_host]);

        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(
            msgs,
            vec![ServerMsg::JoinFailed {
                code: ERR_NO_RANDOM_MATCH_FOUND,
                msg: "NoRandomMatchFound".into(),
            }]
        );

        // A held (inactive) slot still counts against capacity.
        let two_host = h.connect(6);
        h.send(two_host, ClientMsg::JoinLobby { name: "L".into() });
        h.send(two_host, ClientMsg::CreateRoom {
            name: "two".into(),
            max_players: 2,
            visible: true,
            open: true,
            props: Map::from_pairs(vec![("C0", Value::Int(4)), ("C1", Value::Int(500))]),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        let guest = h.connect(7);
        h.send(guest, ClientMsg::JoinRoom { name: "two".into(), player_props: Map::new() });
        h.clear(&[two_host, guest]);
        let now = h.now;
        h.reg.disconnect(guest, now);
        h.clear(&[two_host]);

        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert!(matches!(msgs[0], ServerMsg::JoinFailed { code: ERR_NO_RANDOM_MATCH_FOUND, .. }));
    }

    #[test]
    fn join_random_fills_the_most_populated_room_then_the_oldest() {
        let mut h = Harness::new();
        // Three eligible rooms, created oldest first.
        let host1 = lobby_room(&mut h, 1, "L", "oldest", 4, 500);
        let _host2 = lobby_room(&mut h, 2, "L", "middle", 4, 500);
        let host3 = lobby_room(&mut h, 3, "L", "newest", 4, 500);

        // Put a second body in "newest" so it is the most populated.
        let extra = h.connect(4);
        h.send(extra, ClientMsg::JoinRoom { name: "newest".into(), player_props: Map::new() });
        h.clear(&[host1, host3, extra]);

        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(joined_room(&msgs).0, "newest", "most populated wins");
        h.send(seeker, ClientMsg::LeaveRoom);
        h.clear(&[seeker]);

        // "newest" is now full-ish but not full; make it full so the tie between the two
        // one-player rooms decides on age.
        let f1 = h.connect(10);
        let f2 = h.connect(11);
        h.send(f1, ClientMsg::JoinRoom { name: "newest".into(), player_props: Map::new() });
        h.send(f2, ClientMsg::JoinRoom { name: "newest".into(), player_props: Map::new() });
        h.clear(&[f1, f2]);

        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(joined_room(&msgs).0, "oldest", "equal population falls back to oldest");
    }

    #[test]
    fn join_random_is_scoped_to_the_current_lobby() {
        let mut h = Harness::new();
        lobby_room(&mut h, 1, "MultiEventLobby_31", "a", 4, 500);
        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "MultiEventLobby_32".into() });
        h.clear(&[seeker]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[]);
        assert!(matches!(msgs[0], ServerMsg::JoinFailed { code: ERR_NO_RANDOM_MATCH_FOUND, .. }));

        h.send(seeker, ClientMsg::JoinLobby { name: "MultiEventLobby_31".into() });
        h.clear(&[seeker]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[]);
        assert_eq!(joined_room(&msgs).0, "a");
    }

    #[test]
    fn join_by_code_ignores_visibility_and_the_lobby() {
        let mut h = Harness::new();
        let host = h.connect(1);
        h.send(host, ClientMsg::JoinLobby { name: "L".into() });
        h.send(host, ClientMsg::CreateRoom {
            name: "042719".into(),
            max_players: 4,
            visible: false,
            open: true,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.clear(&[host]);

        let guest = h.connect(2);
        h.send(guest, ClientMsg::JoinRoom { name: "042719".into(), player_props: Map::new() });
        assert_eq!(joined_room(&h.drain(guest)).0, "042719");
    }

    // --- #253 / #254 (Photon's IsOpen / IsVisible byte keys) ------------------

    fn set_flags(h: &mut Harness, id: ConnId, pairs: Vec<(&str, Value)>) {
        h.send(id, ClientMsg::SetRoomProps { props: Map::from_pairs(pairs) });
    }

    #[test]
    fn closing_a_room_after_creation_stops_matching_and_joining() {
        let mut h = Harness::new();
        let host = lobby_room(&mut h, 1, "L", "room", 4, 500);

        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        // Matching completes: MultiMatchingView.SetRoomOpenVisible(false) pushes
        // {253: false} and {254: false} through Room.IsOpen / Room.IsVisible.
        set_flags(&mut h, host, vec![
            (PROP_ROOM_IS_OPEN, Value::Bool(false)),
            (PROP_ROOM_IS_VISIBLE, Value::Bool(false)),
        ]);

        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(
            msgs,
            vec![ServerMsg::JoinFailed {
                code: ERR_NO_RANDOM_MATCH_FOUND,
                msg: "NoRandomMatchFound".into(),
            }]
        );

        h.send(seeker, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(seeker),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_CLOSED, msg: "GameClosed".into() }]
        );
    }

    #[test]
    fn hiding_a_room_stops_matching_but_leaves_the_code_working() {
        let mut h = Harness::new();
        let host = lobby_room(&mut h, 1, "L", "042719", 4, 500);

        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        // SetRoomOpenHide: still open, no longer listed.
        set_flags(&mut h, host, vec![
            (PROP_ROOM_IS_OPEN, Value::Bool(true)),
            (PROP_ROOM_IS_VISIBLE, Value::Bool(false)),
        ]);

        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert!(matches!(msgs[0], ServerMsg::JoinFailed { code: ERR_NO_RANDOM_MATCH_FOUND, .. }));

        // The 6 character code still gets you in.
        h.send(seeker, ClientMsg::JoinRoom { name: "042719".into(), player_props: Map::new() });
        assert_eq!(joined_room(&h.drain(seeker)).0, "042719");
    }

    #[test]
    fn reopening_a_room_puts_it_back_in_matching() {
        let mut h = Harness::new();
        let host = lobby_room(&mut h, 1, "L", "room", 4, 500);
        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        set_flags(&mut h, host, vec![
            (PROP_ROOM_IS_OPEN, Value::Bool(false)),
            (PROP_ROOM_IS_VISIBLE, Value::Bool(false)),
        ]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert!(matches!(msgs[0], ServerMsg::JoinFailed { code: ERR_NO_RANDOM_MATCH_FOUND, .. }));

        // A cancelled match reopens the room.
        set_flags(&mut h, host, vec![
            (PROP_ROOM_IS_OPEN, Value::Bool(true)),
            (PROP_ROOM_IS_VISIBLE, Value::Bool(true)),
        ]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(joined_room(&msgs).0, "room");
    }

    #[test]
    fn the_flags_are_mirrored_in_the_bag_as_well_as_interpreted() {
        // Non-master clients poll their own CurrentRoom.IsOpen mirror, which is fed by
        // RoomPropsChanged and by the JoinedRoom snapshot - so the keys have to survive
        // in the bag, not just move the server's flags.
        let mut h = Harness::new();
        let host = lobby_room(&mut h, 1, "L", "room", 4, 500);
        let guest = h.connect(2);
        h.send(guest, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        h.clear(&[host, guest]);

        set_flags(&mut h, host, vec![(PROP_ROOM_IS_OPEN, Value::Bool(false))]);
        assert_eq!(
            h.drain(guest),
            vec![ServerMsg::RoomPropsChanged {
                props: Map::from_pairs(vec![(PROP_ROOM_IS_OPEN, Value::Bool(false))]),
            }]
        );

        // Nobody can join a closed room, but a rejoiner still sees the flag in the
        // snapshot; check the bag through the room state the sweep-free path exposes.
        let late = h.connect(3);
        h.send(late, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(late),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_CLOSED, msg: "GameClosed".into() }]
        );

        set_flags(&mut h, host, vec![(PROP_ROOM_IS_OPEN, Value::Bool(true))]);
        h.clear(&[guest]);
        h.send(late, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        let msgs = h.drain(late);
        let ServerMsg::JoinedRoom { room_props, .. } = &msgs[0] else {
            panic!("expected JoinedRoom");
        };
        assert_eq!(room_props.get(PROP_ROOM_IS_OPEN), Some(&Value::Bool(true)));
    }

    #[test]
    fn create_room_flag_props_override_the_message_fields() {
        let mut h = Harness::new();
        let host = h.connect(1);
        h.send(host, ClientMsg::JoinLobby { name: "L".into() });
        // Message fields say visible+open; the seeded bag says closed and hidden.
        h.send(host, ClientMsg::CreateRoom {
            name: "room".into(),
            max_players: 4,
            visible: true,
            open: true,
            props: Map::from_pairs(vec![
                ("C0", Value::Int(4)),
                ("C1", Value::Int(500)),
                (PROP_ROOM_IS_OPEN, Value::Bool(false)),
                (PROP_ROOM_IS_VISIBLE, Value::Bool(false)),
            ]),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
        h.clear(&[host]);

        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert!(matches!(msgs[0], ServerMsg::JoinFailed { code: ERR_NO_RANDOM_MATCH_FOUND, .. }));
        h.send(seeker, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(seeker),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_CLOSED, msg: "GameClosed".into() }]
        );
    }

    #[test]
    fn non_bool_or_absent_flag_values_leave_the_room_alone() {
        let mut h = Harness::new();
        let host = lobby_room(&mut h, 1, "L", "room", 4, 500);
        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        // Photon's boxing contract says these are bools; anything else is a client bug
        // and must not silently close the room.
        set_flags(&mut h, host, vec![
            (PROP_ROOM_IS_OPEN, Value::Int(0)),
            (PROP_ROOM_IS_VISIBLE, Value::Null),
            ("A", Value::Str("unrelated".into())),
        ]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(joined_room(&msgs).0, "room", "a non-Bool must not move the flags");
        h.send(seeker, ClientMsg::LeaveRoom);
        h.clear(&[seeker]);

        // An update that mentions neither key leaves both alone too.
        set_flags(&mut h, host, vec![("B", Value::Int(1))]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(joined_room(&msgs).0, "room");
    }

    #[test]
    fn only_the_open_flag_moves_when_only_it_is_sent() {
        let mut h = Harness::new();
        let host = lobby_room(&mut h, 1, "L", "room", 4, 500);
        let seeker = h.connect(9);
        h.send(seeker, ClientMsg::JoinLobby { name: "L".into() });
        h.clear(&[seeker]);

        // MultiPlayManager.SendCurrentRoomIsOpen(false) touches 253 only.
        set_flags(&mut h, host, vec![(PROP_ROOM_IS_OPEN, Value::Bool(false))]);
        h.send(seeker, ClientMsg::JoinRoom { name: "room".into(), player_props: Map::new() });
        assert_eq!(
            h.drain(seeker),
            vec![ServerMsg::JoinFailed { code: ERR_GAME_CLOSED, msg: "GameClosed".into() }]
        );

        // Visible is untouched, so reopening alone is enough to be matchable again.
        set_flags(&mut h, host, vec![(PROP_ROOM_IS_OPEN, Value::Bool(true))]);
        let msgs = join_random(&mut h, seeker, 0, 1000, &[4]);
        assert_eq!(joined_room(&msgs).0, "room");
    }

    #[test]
    fn props_and_rpcs_outside_a_room_are_ignored() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.send(a, ClientMsg::SetPlayerProps {
            props: Map::from_pairs(vec![("A", Value::Int(1))]),
        });
        h.send(a, ClientMsg::SetRoomProps {
            props: Map::from_pairs(vec![("A", Value::Int(1))]),
        });
        h.send(a, ClientMsg::Rpc { name: "SendStamp".into(), params: vec![] });
        h.send(a, ClientMsg::LeaveRoom);
        assert_eq!(h.drain(a), vec![]);
    }

    #[test]
    fn ping_answers_with_the_server_clock() {
        let mut h = Harness::new();
        let a = h.connect(1);
        h.send(a, ClientMsg::Ping);
        match h.drain(a).as_slice() {
            [ServerMsg::Pong { server_time_ms }] => assert!(*server_time_ms > 0.0),
            other => panic!("expected Pong, got {:?}", other),
        }
    }

    // --- live-token lookup (the /multi_live/end score-board branch) -----------------
    //
    // The end handler has nothing but the room token to go on, and has to tell a private
    // party from public matchmaking with it. The trap is that the live flags cannot answer
    // the question: the game hides a PUBLIC room as well, the moment its members are
    // decided, so privacy is latched at creation instead.

    // MultiPlayManager.CreatePrivateRoom: the 6-digit code is the room name, and the room
    // is created hidden so random matching can never land in it.
    fn create_private(h: &mut Harness, id: ConnId, code: &str) {
        h.send(id, ClientMsg::CreateRoom {
            name: code.to_string(),
            max_players: 4,
            visible: false,
            open: true,
            props: Map::new(),
            lobby_prop_keys: vec![],
            player_props: Map::new(),
        });
    }

    // MultiEventMatchingScene: CommitCurrentRoomToken then PushCurrentRoomData, once, just
    // before the live starts.
    fn push_token(h: &mut Harness, id: ConnId, token: &str) {
        h.send(id, ClientMsg::SetRoomProps {
            props: Map::from_pairs(vec![(PROP_ROOM_TOKEN, Value::Str(token.to_string()))]),
        });
    }

    #[test]
    fn a_private_room_is_found_by_the_live_token_it_carries() {
        let mut h = Harness::new();
        let a = h.connect(1);
        let b = h.connect(2);
        create_private(&mut h, a, "042719");
        h.send(b, ClientMsg::JoinRoom { name: "042719".into(), player_props: Map::new() });
        push_token(&mut h, a, "1.abcd");

        // Every party member posts this same token, so both ends resolve to the one room.
        assert_eq!(h.reg.token_room_is_private("1.abcd"), Some(true));
    }

    #[test]
    fn a_public_room_stays_public_after_the_game_hides_it() {
        // The regression this whole field exists for: MultiMatchingView.SetRoomOpenVisible
        // (false) closes and hides a public room once its members are decided, so by the
        // time the live ends the current flags look exactly like a private room's.
        let mut h = Harness::new();
        let a = h.connect(1);
        h.create(a, "1234567", vec![]);
        push_token(&mut h, a, "1.efgh");
        assert_eq!(h.reg.token_room_is_private("1.efgh"), Some(false));

        set_flags(&mut h, a, vec![
            (PROP_ROOM_IS_OPEN, Value::Bool(false)),
            (PROP_ROOM_IS_VISIBLE, Value::Bool(false)),
        ]);
        assert_eq!(
            h.reg.token_room_is_private("1.efgh"),
            Some(false),
            "a hidden public room must not be mistaken for a private party"
        );
    }

    #[test]
    fn an_unknown_or_empty_token_matches_no_room() {
        let mut h = Harness::new();
        let a = h.connect(1);
        create_private(&mut h, a, "042719");
        push_token(&mut h, a, "1.abcd");

        assert_eq!(h.reg.token_room_is_private("2.nope"), None);
        // A room whose master never pushed a token reads back "" on the client; that must
        // not match the first room in the registry.
        assert_eq!(h.reg.token_room_is_private(""), None);

        // And once the last active player leaves, the room — and the answer — is gone.
        h.send(a, ClientMsg::LeaveRoom);
        assert_eq!(h.reg.room_count(), 0);
        assert_eq!(h.reg.token_room_is_private("1.abcd"), None);
    }

    #[test]
    fn disconnect_drops_the_connection_record() {
        let mut h = Harness::new();
        let a = h.connect(1);
        assert_eq!(h.reg.connection_count(), 1);
        let now = h.now;
        h.reg.disconnect(a, now);
        assert_eq!(h.reg.connection_count(), 0);
        // Idempotent: the ws task always calls it, even on a clean close.
        h.reg.disconnect(a, now);
        assert_eq!(h.reg.connection_count(), 0);
    }
}

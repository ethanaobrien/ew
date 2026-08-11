// WebSocket end of the multi-live relay: GET /api/multi_live/ws.
//
// One task per connection reads frames, decodes them and drives the (synchronous,
// lock-serialised) registry in rooms.rs. A second task per connection drains that
// connection's unbounded queue into the socket. Nothing that touches the registry ever
// awaits while the lock is held, which is what lets the registry hand out a total order
// over every broadcast - see the header comment in rooms.rs.
//
// Everything the client can get wrong ends in a close: 4000 for a framing/protocol
// error, 4001 for an unauthenticated or unauthenticated-first frame, 4002 for blowing
// the inbound rate cap, 4003 for going silent long enough to be presumed dead (the
// liveness sweep - see LIVENESS_TIMEOUT in rooms.rs, and start_sweeper below).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use actix_web::rt;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::{AggregatedMessage, AggregatedMessageStream, CloseCode, CloseReason, Session};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::router::userdata;
use super::proto::{
    self, ClientMsg, DecodeError, ServerMsg, CLOSE_PROTOCOL, CLOSE_RATE_LIMIT,
    CLOSE_UNAUTHENTICATED,
};
use super::rooms::{self, ConnId, Outbound};

// "Server closes idle unauthenticated connections after 10s."
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
// "a per-connection cap of 30 inbound messages/sec ... over the cap -> close 4002".
// The honest client peaks at 3-5/sec.
const RATE_LIMIT_PER_SEC: usize = 30;
const RATE_WINDOW: Duration = Duration::from_secs(1);
// Property bags and RPC params are tens of bytes; nothing legitimate comes close.
const MAX_FRAME_BYTES: usize = 64 * 1024;
// How often the inactive-actor, empty-room and connection-liveness expiries are checked.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

pub async fn ws(req: HttpRequest, body: web::Payload) -> Result<HttpResponse, actix_web::Error> {
    // Older client builds carry incompatible multi implementations (pre-relay wire
    // framing), so the upgrade itself is gated on the extended-protocol version — a
    // non-101 answer makes the client's ConnectAsync fail cleanly into its official
    // disconnect flow instead of mis-framing against the relay.
    let protocol = crate::router::global::client_protocol_version(&req);
    if protocol < super::PROTOCOL_VERSION {
        println!(
            "multi_live/ws: upgrade refused, X-Protocol-Version {} < {}",
            protocol,
            super::PROTOCOL_VERSION
        );
        return Ok(HttpResponse::UpgradeRequired().finish());
    }
    let (response, session, stream) = match actix_ws::handle(&req, body) {
        Ok(parts) => {
            println!("multi_live/ws: upgrade accepted, awaiting Auth");
            parts
        }
        Err(err) => {
            println!("multi_live/ws: upgrade REJECTED (not a websocket handshake?): {}", err);
            return Err(err);
        }
    };
    let stream = stream
        .max_frame_size(MAX_FRAME_BYTES)
        .aggregate_continuations()
        .max_continuation_size(MAX_FRAME_BYTES);

    let (tx, rx) = unbounded_channel::<Outbound>();
    rt::spawn(writer(session, rx));
    rt::spawn(reader(stream, tx));
    Ok(response)
}

// The expiry timers: held-slot TTL, empty-room TTL and the connection liveness window.
//
// One task for the whole process, started from run_server so it lives on the system
// arbiter. It used to be lazily started by the first WebSocket upgrade instead, which put
// it on whichever HTTP worker happened to serve that upgrade: a panic anywhere in that
// worker took the sweeper down with it, permanently and silently, and `Once` guaranteed
// nothing would ever start it again. Every room in the process would then hold its seats
// forever.
//
// Called once per run_server, unconditionally and not behind a "started already" flag:
// each run_server builds its own actix System, and a task spawned on the previous one died
// with it (the mobile path stops and restarts the server in-process). A flag would leave
// the restarted server with no sweeper at all; two sweepers, if they ever overlapped,
// would only mean the idempotent sweep runs twice a second.
pub fn start_sweeper() {
    rt::spawn(async {
        let mut ticker = rt::time::interval(SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            // The guard is a temporary: it is released before the next await.
            rooms::registry().sweep(Instant::now());
        }
    });
}

// Drains this connection's FIFO queue to the socket. The queue is unbounded so that the
// registry can push a whole broadcast while holding its lock without ever blocking.
async fn writer(mut session: Session, mut rx: UnboundedReceiver<Outbound>) {
    while let Some(out) = rx.recv().await {
        match out {
            Outbound::Msg(msg) => {
                if session.binary(proto::encode_server(&msg)).await.is_err() {
                    return;
                }
            }
            Outbound::Pong(bytes) => {
                if session.pong(&bytes).await.is_err() {
                    return;
                }
            }
            Outbound::Close(code) => {
                let _ = session
                    .close(Some(CloseReason { code: CloseCode::Other(code), description: None }))
                    .await;
                return;
            }
        }
    }
    // Queue closed: the connection was deregistered, so the socket goes with it.
    let _ = session.close(None).await;
}

async fn reader(mut stream: AggregatedMessageStream, tx: UnboundedSender<Outbound>) {
    let Some(id) = authenticate(&mut stream, &tx).await else {
        return;
    };

    let mut rate = RateLimiter::new();
    loop {
        let Some(frame) = stream.recv().await else {
            break;
        };
        let payload = match frame {
            Ok(AggregatedMessage::Binary(bytes)) => bytes,
            Ok(AggregatedMessage::Ping(bytes)) => {
                // WebSocket-level keepalive, unrelated to the protocol's Ping op. It is
                // still proof the socket is alive, so it counts for the liveness window
                // (the protocol ops are touched inside registry.handle).
                rooms::registry().touch(id, Instant::now());
                let _ = tx.send(Outbound::Pong(bytes.to_vec()));
                continue;
            }
            Ok(AggregatedMessage::Pong(_)) => {
                rooms::registry().touch(id, Instant::now());
                continue;
            }
            Ok(AggregatedMessage::Close(_)) => break,
            Ok(AggregatedMessage::Text(_)) => {
                // "binary frames only ... Text frames are a protocol error -> close".
                println!("multi_live/ws: text frame from conn {}", id);
                close(&tx, CLOSE_PROTOCOL);
                break;
            }
            Err(err) => {
                println!("multi_live/ws: websocket error on conn {}: {}", id, err);
                break;
            }
        };

        let now = Instant::now();
        if !rate.allow(now) {
            println!("multi_live/ws: conn {} exceeded {} msg/sec", id, RATE_LIMIT_PER_SEC);
            close(&tx, CLOSE_RATE_LIMIT);
            break;
        }

        let msg = match proto::decode_client(&payload) {
            Ok(msg) => msg,
            Err(err) => {
                println!("multi_live/ws: bad frame from conn {}: {}", id, err);
                close(&tx, close_code_for(&err));
                break;
            }
        };
        if matches!(msg, ClientMsg::Auth { .. }) {
            // Auth is the first message and only the first message.
            println!("multi_live/ws: repeat Auth on conn {}", id);
            close(&tx, CLOSE_PROTOCOL);
            break;
        }
        rooms::registry().handle(id, msg, now);
    }

    rooms::registry().disconnect(id, Instant::now());
}

// Reads the mandatory first frame and registers the connection. Returns None when the
// connection was closed instead.
async fn authenticate(
    stream: &mut AggregatedMessageStream,
    tx: &UnboundedSender<Outbound>,
) -> Option<ConnId> {
    let first = match rt::time::timeout(AUTH_TIMEOUT, stream.recv()).await {
        Ok(Some(Ok(frame))) => frame,
        Ok(Some(Err(_))) | Ok(None) => return None,
        Err(_) => {
            println!("multi_live/ws: no Auth within {}s", AUTH_TIMEOUT.as_secs());
            close(tx, CLOSE_UNAUTHENTICATED);
            return None;
        }
    };

    let AggregatedMessage::Binary(payload) = first else {
        // "First message on a connection MUST be Auth; anything else -> close 4001."
        close(tx, CLOSE_UNAUTHENTICATED);
        return None;
    };
    let Ok(ClientMsg::Auth { user_id, token }) = proto::decode_client(&payload) else {
        println!("multi_live/ws: first message was not a decodable Auth frame");
        close(tx, CLOSE_UNAUTHENTICATED);
        return None;
    };

    let Some(uid) = resolve_user(&user_id, &token) else {
        // Never print the token itself (it is the login credential); empty-vs-unknown is
        // the diagnostic that matters: empty means the client sent no credential at all
        // (the pre-fix Android builds sent UserSaveData.m_uuid, which device builds never
        // populate), unknown means a credential the tokens table has no row for.
        println!(
            "multi_live/ws: rejecting uid {}: bad session ({})",
            user_id,
            if token.is_empty() { "empty token" } else { "unknown token" }
        );
        close(tx, CLOSE_UNAUTHENTICATED);
        return None;
    };

    let id = rooms::registry().connect(uid, tx.clone(), Instant::now());
    println!("multi_live/ws: auth ok, uid {} connected as conn {}", uid, id);
    let _ = tx.send(Outbound::Msg(ServerMsg::AuthOk {
        actorless_time_ms: rooms::server_time_ms(),
    }));
    Some(id)
}

// The relay validates the same credential the HTTP layer does. Over HTTP the login token
// arrives inside the `a6573cbe` header (global::get_login) and every handler then keys
// userdata off it; here it arrives as the Auth payload instead, and the tokens table maps
// it back to the account. The claimed userId only has to agree with the token, so a
// client cannot relay as somebody else - which is a little stricter than the HTTP layer,
// where an unknown token simply resolves to an empty account.
fn resolve_user(user_id: &str, token: &str) -> Option<i64> {
    if token.is_empty() {
        return None;
    }
    match_claim(user_id, userdata::uid_from_login_token(token))
}

// The claim check, split out from the lookup so it can be tested without a database.
// `uid` is 0 when the token is unknown.
fn match_claim(user_id: &str, uid: i64) -> Option<i64> {
    if uid == 0 {
        return None;
    }
    match user_id.parse::<i64>() {
        // A blank or unparsable userId is tolerated: the token is the authority.
        Err(_) => Some(uid),
        Ok(claimed) if claimed == 0 || claimed == uid => Some(uid),
        Ok(_) => None,
    }
}

fn close_code_for(err: &DecodeError) -> u16 {
    // The spec names 4001 and 4002 only; every framing failure shares 4000.
    match err {
        DecodeError::Empty
        | DecodeError::Truncated
        | DecodeError::TrailingBytes
        | DecodeError::BadUtf8
        | DecodeError::UnknownOp(_)
        | DecodeError::UnknownTag(_) => CLOSE_PROTOCOL,
    }
}

fn close(tx: &UnboundedSender<Outbound>, code: u16) {
    let _ = tx.send(Outbound::Close(code));
}

// Sliding one second window rather than a fixed bucket, so 30 messages either side of a
// second boundary still trips the cap.
struct RateLimiter {
    seen: VecDeque<Instant>,
}

impl RateLimiter {
    fn new() -> Self {
        RateLimiter { seen: VecDeque::with_capacity(RATE_LIMIT_PER_SEC + 1) }
    }

    fn allow(&mut self, now: Instant) -> bool {
        while let Some(front) = self.seen.front() {
            if now.duration_since(*front) >= RATE_WINDOW {
                self.seen.pop_front();
            } else {
                break;
            }
        }
        if self.seen.len() >= RATE_LIMIT_PER_SEC {
            return false;
        }
        self.seen.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_the_cap_and_rejects_the_next() {
        let mut rate = RateLimiter::new();
        let start = Instant::now();
        for i in 0..RATE_LIMIT_PER_SEC {
            assert!(rate.allow(start + Duration::from_millis(i as u64)), "message {} refused", i);
        }
        assert!(!rate.allow(start + Duration::from_millis(999)));
        // The window slides: once the first message ages out there is room again.
        assert!(rate.allow(start + Duration::from_millis(1001)));
    }

    #[test]
    fn rate_limiter_catches_a_burst_straddling_a_second_boundary() {
        let mut rate = RateLimiter::new();
        let start = Instant::now();
        // 29 messages at the end of one second...
        for i in 0..RATE_LIMIT_PER_SEC - 1 {
            assert!(rate.allow(start + Duration::from_millis(900 + i as u64)));
        }
        // ...and two more just after the boundary is still 31 inside one second.
        assert!(rate.allow(start + Duration::from_millis(1000)));
        assert!(!rate.allow(start + Duration::from_millis(1001)));
    }

    #[test]
    fn every_decode_failure_closes_with_the_protocol_code() {
        for err in [
            DecodeError::Empty,
            DecodeError::Truncated,
            DecodeError::TrailingBytes,
            DecodeError::BadUtf8,
            DecodeError::UnknownOp(200),
            DecodeError::UnknownTag(9),
        ] {
            assert_eq!(close_code_for(&err), CLOSE_PROTOCOL);
        }
    }

    #[test]
    fn auth_needs_a_token_that_agrees_with_the_claimed_user() {
        // An empty token never reaches the database.
        assert_eq!(resolve_user("1", ""), None);
        // An unknown token resolves to uid 0.
        assert_eq!(match_claim("1", 0), None);
        assert_eq!(match_claim("42", 42), Some(42));
        // Claiming somebody else's account with your own token is refused.
        assert_eq!(match_claim("43", 42), None);
        // A blank, zero or unparsable userId defers to the token.
        assert_eq!(match_claim("", 42), Some(42));
        assert_eq!(match_claim("0", 42), Some(42));
        assert_eq!(match_claim("not-a-number", 42), Some(42));
    }
}

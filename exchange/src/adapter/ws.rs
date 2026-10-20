use fastwebsockets::{FragmentCollectorRead, Frame, OpCode, WebSocketWrite};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;

use crate::adapter::MarketKind;

/// Maximum number of queued WebSocket frames to drain in a single tick.
pub const MAX_DRAIN_PER_TICK: usize = 256;

pub const HYPERLIQUID_PING_PAYLOAD: &[u8] = br#"{"method":"ping"}"#;
pub const HYPERLIQUID_HEARTBEAT: HeartbeatPolicy = HeartbeatPolicy::ping_after_idle_text(
    HYPERLIQUID_PING_PAYLOAD,
    Duration::from_secs(30),
    Duration::from_secs(30),
);

pub const BYBIT_PING_PAYLOAD: &[u8] = br#"{"op":"ping"}"#;
pub const BYBIT_HEARTBEAT: HeartbeatPolicy = HeartbeatPolicy::ping_after_idle_text(
    BYBIT_PING_PAYLOAD,
    Duration::from_secs(20),
    Duration::from_secs(20),
);

pub const OKX_PING_PAYLOAD: &[u8] = b"ping";
pub const OKX_HEARTBEAT: HeartbeatPolicy = HeartbeatPolicy::ping_after_idle_text(
    OKX_PING_PAYLOAD,
    Duration::from_secs(20),
    Duration::from_secs(20),
);

pub fn binance_heartbeat_policy(market: MarketKind) -> HeartbeatPolicy {
    let silence_timeout = match market {
        MarketKind::Spot => Duration::from_secs(45),
        MarketKind::LinearPerps | MarketKind::InversePerps => Duration::from_secs(240),
    };
    HeartbeatPolicy::server_driven(silence_timeout)
}

/// Outbound heartbeat payload specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PingPayload {
    Text(&'static [u8]),
    OpCode(&'static [u8]),
}

/// Venue-aware keepalive strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatPolicy {
    PeriodicClientPing {
        ping: PingPayload,
        every: Duration,
        silence_timeout: Duration,
    },
    PingAfterIdle {
        ping: PingPayload,
        idle_for: Duration,
        response_timeout: Duration,
    },
    ServerDriven {
        silence_timeout: Duration,
    },
}

impl HeartbeatPolicy {
    pub const fn periodic_text(
        payload: &'static [u8],
        every: Duration,
        silence_timeout: Duration,
    ) -> Self {
        Self::PeriodicClientPing {
            ping: PingPayload::Text(payload),
            every,
            silence_timeout,
        }
    }

    pub const fn periodic_opcode(
        payload: &'static [u8],
        every: Duration,
        silence_timeout: Duration,
    ) -> Self {
        Self::PeriodicClientPing {
            ping: PingPayload::OpCode(payload),
            every,
            silence_timeout,
        }
    }

    pub const fn ping_after_idle_text(
        payload: &'static [u8],
        idle_for: Duration,
        response_timeout: Duration,
    ) -> Self {
        Self::PingAfterIdle {
            ping: PingPayload::Text(payload),
            idle_for,
            response_timeout,
        }
    }

    pub const fn server_driven(silence_timeout: Duration) -> Self {
        Self::ServerDriven { silence_timeout }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatAction {
    SendPing(PingPayload),
    Timeout,
    Wait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeartbeatState {
    Periodic { next_ping: Instant },
    IdleMonitoring,
    IdleAwaitingResponse { deadline: Instant },
    ServerDriven,
}

pub struct WsHeartbeat {
    policy: HeartbeatPolicy,
    last_activity: Instant,
    state: HeartbeatState,
}

impl WsHeartbeat {
    pub fn new(policy: HeartbeatPolicy, now: Instant) -> Self {
        let state = match policy {
            HeartbeatPolicy::PeriodicClientPing { every, .. } => HeartbeatState::Periodic {
                next_ping: now + every,
            },
            HeartbeatPolicy::PingAfterIdle { .. } => HeartbeatState::IdleMonitoring,
            HeartbeatPolicy::ServerDriven { .. } => HeartbeatState::ServerDriven,
        };

        Self {
            policy,
            last_activity: now,
            state,
        }
    }

    pub fn next_deadline(&self) -> Instant {
        match (self.policy, &self.state) {
            (
                HeartbeatPolicy::PeriodicClientPing {
                    silence_timeout, ..
                },
                HeartbeatState::Periodic { next_ping },
            ) => (self.last_activity + silence_timeout).min(*next_ping),
            (HeartbeatPolicy::PingAfterIdle { idle_for, .. }, HeartbeatState::IdleMonitoring) => {
                self.last_activity + idle_for
            }
            (
                HeartbeatPolicy::PingAfterIdle { .. },
                HeartbeatState::IdleAwaitingResponse { deadline },
            ) => *deadline,
            (HeartbeatPolicy::ServerDriven { silence_timeout }, HeartbeatState::ServerDriven) => {
                self.last_activity + silence_timeout
            }
            (
                HeartbeatPolicy::PeriodicClientPing {
                    silence_timeout, ..
                },
                _,
            ) => self.last_activity + silence_timeout,
            (HeartbeatPolicy::PingAfterIdle { idle_for, .. }, _) => self.last_activity + idle_for,
            (HeartbeatPolicy::ServerDriven { silence_timeout }, _) => {
                self.last_activity + silence_timeout
            }
        }
    }

    pub fn deadline_action(&self, now: Instant) -> HeartbeatAction {
        match (self.policy, &self.state) {
            (
                HeartbeatPolicy::PeriodicClientPing {
                    ping,
                    silence_timeout,
                    ..
                },
                HeartbeatState::Periodic { next_ping },
            ) => {
                if now >= self.last_activity + silence_timeout {
                    HeartbeatAction::Timeout
                } else if now >= *next_ping {
                    HeartbeatAction::SendPing(ping)
                } else {
                    HeartbeatAction::Wait
                }
            }
            (
                HeartbeatPolicy::PingAfterIdle { ping, idle_for, .. },
                HeartbeatState::IdleMonitoring,
            ) => {
                if now >= self.last_activity + idle_for {
                    HeartbeatAction::SendPing(ping)
                } else {
                    HeartbeatAction::Wait
                }
            }
            (
                HeartbeatPolicy::PingAfterIdle { .. },
                HeartbeatState::IdleAwaitingResponse { deadline },
            ) => {
                if now >= *deadline {
                    HeartbeatAction::Timeout
                } else {
                    HeartbeatAction::Wait
                }
            }
            (HeartbeatPolicy::ServerDriven { silence_timeout }, HeartbeatState::ServerDriven) => {
                if now >= self.last_activity + silence_timeout {
                    HeartbeatAction::Timeout
                } else {
                    HeartbeatAction::Wait
                }
            }
            _ => HeartbeatAction::Wait,
        }
    }

    pub fn mark_activity(&mut self, now: Instant) {
        self.last_activity = now;
        if matches!(self.state, HeartbeatState::IdleAwaitingResponse { .. }) {
            self.state = HeartbeatState::IdleMonitoring;
        }
    }

    pub fn record_ping_sent(&mut self, now: Instant) {
        match (self.policy, &self.state) {
            (
                HeartbeatPolicy::PeriodicClientPing { every, .. },
                HeartbeatState::Periodic { .. },
            ) => {
                self.state = HeartbeatState::Periodic {
                    next_ping: now + every,
                };
            }
            (
                HeartbeatPolicy::PingAfterIdle {
                    response_timeout, ..
                },
                HeartbeatState::IdleMonitoring,
            ) => {
                self.state = HeartbeatState::IdleAwaitingResponse {
                    deadline: now + response_timeout,
                };
            }
            _ => {}
        }
    }
}

/// Drains up to `max_items` queued messages from an unbounded receiver without blocking.
pub fn drain_unbounded_batch<T>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<T>,
    max_items: usize,
) -> Vec<T> {
    let mut batch = Vec::with_capacity(max_items.min(64));
    while batch.len() < max_items {
        match rx.try_recv() {
            Ok(item) => batch.push(item),
            Err(_) => break,
        }
    }
    batch
}

/// Cancellation-safe WebSocket frame read loop with separated writer half for heartbeat keepalive.
pub async fn run_ws_connection<R, W>(
    mut reader: FragmentCollectorRead<R>,
    mut writer: WebSocketWrite<W>,
    policy: HeartbeatPolicy,
    frame_tx: tokio::sync::mpsc::UnboundedSender<Result<Vec<u8>, String>>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut heartbeat = WsHeartbeat::new(policy, Instant::now());

    loop {
        let mut no_control_send =
            |_frame: Frame<'_>| std::future::ready(Ok::<(), std::io::Error>(()));
        let mut read_future = Box::pin(reader.read_frame(&mut no_control_send));

        let outcome = loop {
            let deadline = heartbeat.next_deadline();

            tokio::select! {
                biased;
                result = &mut read_future => break Ok(result),
                _ = tokio::time::sleep_until(deadline) => {
                    match heartbeat.deadline_action(Instant::now()) {
                        HeartbeatAction::SendPing(ping) => {
                            let frame = match ping {
                                PingPayload::Text(p) => Frame::text(fastwebsockets::Payload::Borrowed(p)),
                                PingPayload::OpCode(p) => Frame::new(true, OpCode::Ping, None, fastwebsockets::Payload::Borrowed(p)),
                            };
                            if writer.write_frame(frame).await.is_err() {
                                break Err("Failed to send heartbeat ping");
                            }
                            heartbeat.record_ping_sent(Instant::now());
                        }
                        HeartbeatAction::Timeout => {
                            break Err("Heartbeat timeout (no websocket activity)");
                        }
                        HeartbeatAction::Wait => {}
                    }
                }
            }
        };

        drop(read_future);

        let message = match outcome {
            Ok(Ok(msg)) => msg,
            Ok(Err(err)) => {
                let _ = frame_tx.send(Err(format!("Error reading frame: {err}")));
                break;
            }
            Err(reason) => {
                let _ = frame_tx.send(Err(reason.to_string()));
                break;
            }
        };

        heartbeat.mark_activity(Instant::now());

        let keep_running = match message.opcode {
            OpCode::Text => frame_tx.send(Ok(message.payload.to_vec())).is_ok(),
            OpCode::Ping => {
                let _ = writer
                    .write_frame(Frame::pong(fastwebsockets::Payload::Owned(
                        message.payload.to_vec(),
                    )))
                    .await;
                true
            }
            OpCode::Close => {
                let _ = writer
                    .write_frame(Frame::close_raw(fastwebsockets::Payload::Owned(
                        message.payload.to_vec(),
                    )))
                    .await;
                let _ = frame_tx.send(Err("Connection closed by peer".to_string()));
                false
            }
            _ => true,
        };

        if !keep_running {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_idle_policy_deadlines_and_actions() {
        let now = Instant::now();
        let mut hb = WsHeartbeat::new(HYPERLIQUID_HEARTBEAT, now);

        // Initially in IdleMonitoring: next deadline is now + 30s
        assert_eq!(hb.next_deadline(), now + Duration::from_secs(30));
        assert_eq!(
            hb.deadline_action(now + Duration::from_secs(10)),
            HeartbeatAction::Wait
        );

        // At 30s idle, it sends a ping
        assert_eq!(
            hb.deadline_action(now + Duration::from_secs(30)),
            HeartbeatAction::SendPing(PingPayload::Text(HYPERLIQUID_PING_PAYLOAD))
        );

        // After recording ping sent, it transitions to awaiting response (deadline: +30s)
        hb.record_ping_sent(now + Duration::from_secs(30));
        assert_eq!(hb.next_deadline(), now + Duration::from_secs(60));

        // If no activity by 60s, it times out
        assert_eq!(
            hb.deadline_action(now + Duration::from_secs(60)),
            HeartbeatAction::Timeout
        );

        // If activity arrives before timeout, it resets back to IdleMonitoring
        hb.mark_activity(now + Duration::from_secs(45));
        assert_eq!(hb.next_deadline(), now + Duration::from_secs(75));
        assert_eq!(
            hb.deadline_action(now + Duration::from_secs(50)),
            HeartbeatAction::Wait
        );
    }

    #[test]
    fn test_drain_unbounded_batch_caps_at_max() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for i in 0..500 {
            tx.send(i).unwrap();
        }

        let batch1 = drain_unbounded_batch(&mut rx, MAX_DRAIN_PER_TICK);
        assert_eq!(batch1.len(), MAX_DRAIN_PER_TICK);
        assert_eq!(batch1[0], 0);
        assert_eq!(batch1[MAX_DRAIN_PER_TICK - 1], 255);

        let batch2 = drain_unbounded_batch(&mut rx, MAX_DRAIN_PER_TICK);
        assert_eq!(batch2.len(), 500 - MAX_DRAIN_PER_TICK);
        assert_eq!(batch2[0], 256);

        let batch3 = drain_unbounded_batch(&mut rx, MAX_DRAIN_PER_TICK);
        assert!(batch3.is_empty());
    }
}

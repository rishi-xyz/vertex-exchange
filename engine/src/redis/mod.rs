use fred::{
    clients::Client,
    interfaces::{ClientLike, StreamsInterface},
    types::{
        config::Config,
        streams::{XCapKind, XCapTrim},
    },
};
use tokio::sync::mpsc;
use tracing;

use crate::{
    trade::{Trade, Trades},
    trading_pair::TradingPair,
};

pub struct FillEvent {
    pair: String,
    trades: Vec<Trade>,
}

pub struct FillPublisher {
    sender: Option<mpsc::Sender<FillEvent>>,
}

const FILLS_STREAM: &str = "vertex:fills";
const MAX_LEN: u64 = 10_000;

impl FillPublisher {
    pub async fn new() -> Self {
        let redis_url = match std::env::var("REDIS_URL") {
            Ok(url) => url,
            Err(_) => {
                tracing::info!("REDIS_URL not set - fills distribution disabled");
                return Self { sender: None };
            }
        };
        let config = match Config::from_url(&redis_url) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Invalid REDIS_URL ({e}) - fills distribution disabled");
                return Self { sender: None };
            }
        };

        let client = Client::new(config, None, None, None);
        client.connect();
        if let Err(e) = client.wait_for_connect().await {
            tracing::warn!("Failed to connect to Redis ({e}) - fills distribution disabled");
            return Self { sender: None };
        }

        tracing::info!(
            "Connected to Redis — fills will be published to {}",
            FILLS_STREAM
        );

        let (sender, mut receiver) = mpsc::channel::<FillEvent>(1024);

        // Drain and publish in batches via a pipeline, rather than one
        // awaited XADD round trip per trade: under a burst of concurrent
        // crossing orders, sequential one-at-a-time publishing here becomes
        // exactly the kind of queueing bottleneck fixed on the gateway's
        // fills consumer (internal/fills/consumer.go) — a fill sitting late
        // in this channel would otherwise wait for every earlier one to
        // individually round-trip to Redis before it's even sent.
        tokio::spawn(async move {
            'outer: loop {
                let first = match receiver.recv().await {
                    Some(event) => event,
                    None => break 'outer,
                };
                let mut batch = vec![first];
                while let Ok(event) = receiver.try_recv() {
                    batch.push(event);
                }

                let pipeline = client.pipeline();
                let mut queued = 0usize;
                for event in &batch {
                    for trade in &event.trades {
                        let fields: Vec<(&str, String)> = vec![
                            ("trade_id", trade.get_trade_id().to_string()),
                            ("timestamp", trade.get_timestamp().to_string()),
                            ("pair", event.pair.clone()),
                            (
                                "bid_order_id",
                                trade.get_bid_trade_info().get_order_id().to_string(),
                            ),
                            (
                                "bid_user_id",
                                trade.get_bid_trade_info().get_user_id().to_string(),
                            ),
                            (
                                "bid_price",
                                trade.get_bid_trade_info().get_price().to_string(),
                            ),
                            (
                                "bid_quantity",
                                trade.get_bid_trade_info().get_quantity().to_string(),
                            ),
                            (
                                "ask_order_id",
                                trade.get_ask_trade_info().get_order_id().to_string(),
                            ),
                            (
                                "ask_user_id",
                                trade.get_ask_trade_info().get_user_id().to_string(),
                            ),
                            (
                                "ask_price",
                                trade.get_ask_trade_info().get_price().to_string(),
                            ),
                            (
                                "ask_quantity",
                                trade.get_ask_trade_info().get_quantity().to_string(),
                            ),
                        ];

                        // Queuing onto a Pipeline only buffers the command
                        // locally (see fred's Pipeline::send_command) — it
                        // cannot fail here; real errors surface from `.all()`
                        // once the batch is actually sent below.
                        let _ = pipeline
                            .xadd::<(), &str, (XCapKind, XCapTrim, u64), &str, Vec<(&str, std::string::String)>>(
                                FILLS_STREAM,
                                false,
                                (XCapKind::MaxLen, XCapTrim::AlmostExact, MAX_LEN),
                                "*",
                                fields,
                            )
                            .await;
                        queued += 1;
                    }
                }
                if queued == 0 {
                    continue;
                }
                if let Err(err) = pipeline.all::<()>().await {
                    tracing::error!(
                        "Failed to publish fill batch ({queued} fills) to Redis: {err}"
                    );
                }
            }
            tracing::warn!("Redis publisher worker exited");
        });

        Self {
            sender: Some(sender),
        }
    }

    pub async fn publish_fills(&self, pair: &TradingPair, trades: &Trades) {
        if let Some(sender) = &self.sender {
            let event = FillEvent {
                pair: pair.to_string(),
                trades: trades.iter().cloned().collect(),
            };
            if let Err(e) = sender.send(event).await {
                tracing::warn!("Redis publisher channel closed: {e}");
            }
        }
    }
}

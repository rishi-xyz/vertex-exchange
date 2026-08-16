pub mod proto {
    tonic::include_proto!("vertex_engine");
}

use std::collections::HashMap;

use tokio::sync::{mpsc, oneshot};
use tonic::{Request, Response, Status};
use tracing;
use uuid::Uuid;

use crate::{
    engine::{
        EngineWrapper,
        trade_def::{ExchangeEngine, UsersEngine},
    },
    grpc::proto::{
        AddUserResponse, DepositBalanceResponse, GetBalanceResponse, GetTotalBalanceResponse,
        RemoveUserResponse, WithdrawBalanceResponse, engine_services_server::EngineServices,
        user_services_server::UserServices,
    },
    level_info::OrderBookLevelInfo,
    order::Order,
    order_modify::OrderModify,
    redis::FillPublisher,
    trade::{self, Trades},
    trading_pair::TradingPair,
    types::{Asset, OrderError, OrderId, OrderStatus, OrderType, Price, Quantity, Side, UserError},
};

/// A failure surfaced from the engine task, mapped to a tonic [`Status`].
///
/// A [`Panic`](EngineError::Panic) variant means the engine panicked while
/// processing the command; `run_engine` replies with it and then initiates a
/// fail-fast shutdown (the process exits rather than continuing in a corrupt
/// state).
#[derive(Debug)]
pub enum EngineError {
    /// The engine panicked while processing the command.
    Panic(Box<dyn std::any::Any + Send>),
    /// The command was rejected with an order-level error.
    Order(OrderError),
    /// The command was rejected with a user/balance-level error.
    User(UserError),
}

impl EngineError {
    fn to_status(&self) -> Status {
        match self {
            EngineError::Panic(_) => Status::internal("internal engine error"),
            EngineError::Order(e) => match e {
                OrderError::NoSuchPair => Status::not_found("trading pair not found"),
                OrderError::NoSuchUser => Status::not_found("user not found"),
                OrderError::InsufficientBalance => {
                    Status::failed_precondition("insufficient balance")
                }
                OrderError::InvalidOrder => Status::invalid_argument("invalid order"),
            },
            EngineError::User(e) => match e {
                UserError::NoSuchUser => Status::not_found("user not found"),
                UserError::InsufficientBalance => {
                    Status::failed_precondition("insufficient balance")
                }
                UserError::BalanceOverflow => Status::failed_precondition("balance overflow"),
            },
        }
    }
}

pub enum EngineCommand {
    // Engine service commands
    SubmitOrder {
        pair: TradingPair,
        order_type: OrderType,
        side: Side,
        price: Price,
        quantity: Quantity,
        user_id: Uuid,
        reply: oneshot::Sender<Result<(OrderId, Option<Trades>), EngineError>>,
    },
    CancelOrder {
        pair: TradingPair,
        order_id: OrderId,
        reply: oneshot::Sender<Result<bool, EngineError>>,
    },
    ModifyOrder {
        pair: TradingPair,
        modify: OrderModify,
        reply: oneshot::Sender<Result<Option<Trades>, EngineError>>,
    },
    GetOrderBook {
        pair: TradingPair,
        reply: oneshot::Sender<Result<OrderBookLevelInfo, EngineError>>,
    },
    AddTradingPair {
        pair: TradingPair,
        reply: oneshot::Sender<Result<(), EngineError>>,
    },
    // user service commands
    AddUser {
        user_id: Uuid,
        reply: oneshot::Sender<Result<(), EngineError>>,
    },
    RemoveUser {
        user_id: Uuid,
        reply: oneshot::Sender<Result<HashMap<Asset, Quantity>, EngineError>>,
    },
    DepositBalance {
        user_id: Uuid,
        asset: Asset,
        quantity: Quantity,
        reply: oneshot::Sender<Result<(), EngineError>>,
    },
    WithdrawBalance {
        user_id: Uuid,
        asset: Asset,
        quantity: Quantity,
        reply: oneshot::Sender<Result<(), EngineError>>,
    },
    GetBalance {
        user_id: Uuid,
        asset: Asset,
        reply: oneshot::Sender<Result<Quantity, EngineError>>,
    },
    GetTotalBalance {
        user_id: Uuid,
        asset: Asset,
        reply: oneshot::Sender<Result<Quantity, EngineError>>,
    },
}

#[derive(Debug, Clone)]
pub struct EngineService {
    tx: mpsc::Sender<EngineCommand>,
}

impl EngineService {
    pub fn new(tx: mpsc::Sender<EngineCommand>) -> Self {
        Self { tx }
    }
}

pub async fn run_engine(
    mut rx: mpsc::Receiver<EngineCommand>,
    mut engine: EngineWrapper,
    publisher: FillPublisher,
    fatal_tx: oneshot::Sender<()>,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            EngineCommand::SubmitOrder {
                pair,
                order_type,
                side,
                price,
                quantity,
                user_id,
                reply,
            } => {
                tracing::debug!(%pair, ?side, ?order_type, price, quantity, %user_id, "submit_order");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let order_id = engine.next_id();
                    let order = Order::new(
                        order_id,
                        order_type,
                        side,
                        OrderStatus::Empty,
                        price,
                        quantity,
                        user_id,
                    );
                    let trades = engine
                        .add_order(&pair, &order)
                        .map_err(EngineError::Order)?;
                    Ok::<_, EngineError>((order_id, trades))
                }));
                match result {
                    Ok(res) => {
                        if let Ok((_order_id, Some(ref trades))) = res {
                            publisher.publish_fills(&pair, trades).await;
                        }
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("submit_order panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::CancelOrder {
                pair,
                order_id,
                reply,
            } => {
                tracing::debug!(%pair, order_id, "cancel_order");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Ok::<_, EngineError>(engine.cancel_order(&pair, &order_id))
                }));
                match result {
                    Ok(res) => {
                        tracing::debug!(order_id, "cancel_order complete");
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("cancel_order panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::ModifyOrder {
                pair,
                modify,
                reply,
            } => {
                tracing::debug!(%pair, "modify_order");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let trades = engine
                        .modify_order(&pair, modify)
                        .map_err(EngineError::Order)?;
                    Ok::<_, EngineError>(trades)
                }));
                match result {
                    Ok(res) => {
                        if let Ok(Some(ref trades)) = res {
                            publisher.publish_fills(&pair, trades).await;
                        }
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("modify_order panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::GetOrderBook { pair, reply } => {
                tracing::debug!(%pair, "get_order_book");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine
                        .get_order_info(&pair)
                        .ok_or(EngineError::Order(OrderError::NoSuchPair))
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("get_order_book panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::AddTradingPair { pair, reply } => {
                tracing::info!(%pair, "add_trading_pair");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.add_trading_pair(pair);
                    Ok::<_, EngineError>(())
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("add_trading_pair panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::AddUser { user_id, reply } => {
                tracing::info!(%user_id, "add_user");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.add_user(user_id);
                    Ok::<_, EngineError>(())
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("add_user panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::RemoveUser { user_id, reply } => {
                tracing::info!(%user_id, "remove_user");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.remove_user(user_id).map_err(EngineError::User)
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("remove_user panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::DepositBalance {
                user_id,
                asset,
                quantity,
                reply,
            } => {
                tracing::info!(%user_id, ?asset, quantity, "deposit_balance");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine
                        .deposit_balance(user_id, asset, quantity)
                        .map_err(EngineError::User)
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("deposit_balance panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::WithdrawBalance {
                user_id,
                asset,
                quantity,
                reply,
            } => {
                tracing::info!(%user_id, ?asset, quantity, "withdraw_balance");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine
                        .withdraw_balance(user_id, asset, quantity)
                        .map_err(EngineError::User)
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("withdraw_balance panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::GetBalance {
                user_id,
                asset,
                reply,
            } => {
                tracing::info!(%user_id, ?asset, "get_balance");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine
                        .get_balance(user_id, asset)
                        .map_err(EngineError::User)
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("get_balance panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
            EngineCommand::GetTotalBalance {
                user_id,
                asset,
                reply,
            } => {
                tracing::info!(%user_id, ?asset, "get_total_balance");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine
                        .get_total_balance(user_id, asset)
                        .map_err(EngineError::User)
                }));
                match result {
                    Ok(res) => {
                        let _ = reply.send(res);
                    }
                    Err(e) => {
                        tracing::error!("get_total_balance panic: {:?}", e);
                        let _ = reply.send(Err(EngineError::Panic(e)));
                        let _ = fatal_tx.send(());
                        break;
                    }
                }
            }
        }
    }
}

fn proto_asset_to_engine(asset: proto::Asset) -> Result<Asset, Status> {
    match asset {
        proto::Asset::Eth => Ok(Asset::ETH),
        proto::Asset::Sol => Ok(Asset::SOL),
        proto::Asset::Btc => Ok(Asset::BTC),
        proto::Asset::Usdc => Ok(Asset::USDC),
        proto::Asset::Usdt => Ok(Asset::USDT),
    }
}

fn proto_side_to_engine(side: proto::Side) -> Result<Side, Status> {
    match side {
        proto::Side::Buy => Ok(Side::Buy),
        proto::Side::Sell => Ok(Side::Sell),
    }
}

fn proto_order_type_to_engine(order_type: proto::OrderType) -> Result<OrderType, Status> {
    match order_type {
        proto::OrderType::GoodTillCancel => Ok(OrderType::GoodTillCancel),
        proto::OrderType::GoodForDay => Ok(OrderType::GoodForDay),
        proto::OrderType::FillAndKill => Ok(OrderType::FillAndKill),
        proto::OrderType::FillOrKill => Ok(OrderType::FillOrKill),
    }
}

fn proto_pair_to_engine(pair: proto::TradingPair) -> Result<TradingPair, Status> {
    Ok(TradingPair::new(
        proto_asset_to_engine(pair.base())?,
        proto_asset_to_engine(pair.quote())?,
    ))
}

fn engine_trade_to_proto(trade: trade::Trade) -> proto::Trade {
    proto::Trade {
        trade_id: trade.get_trade_id(),
        timestamp: trade.get_timestamp(),
        bid_trade: Some(proto::TradeInfo {
            order_id: trade.get_bid_trade_info().get_order_id(),
            price: trade.get_bid_trade_info().get_price(),
            quantity: trade.get_bid_trade_info().get_quantity(),
            user_id: trade.get_bid_trade_info().get_user_id().to_string(),
        }),
        ask_trade: Some(proto::TradeInfo {
            order_id: trade.get_ask_trade_info().get_order_id(),
            price: trade.get_ask_trade_info().get_price(),
            quantity: trade.get_ask_trade_info().get_quantity(),
            user_id: trade.get_ask_trade_info().get_user_id().to_string(),
        }),
    }
}

#[tonic::async_trait]
impl UserServices for EngineService {
    async fn submit_order(
        &self,
        request: Request<proto::SubmitOrderRequest>,
    ) -> Result<Response<proto::SubmitOrderResponse>, Status> {
        let req = request.into_inner();
        let pair = proto_pair_to_engine(
            req.pair
                .ok_or_else(|| Status::invalid_argument("missing trading pair"))?,
        )?;
        let side = proto_side_to_engine(req.side())?;
        let order_type = proto_order_type_to_engine(req.order_type())?;
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("invalid user_id"))?;

        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::SubmitOrder {
                pair,
                order_type,
                side,
                price: req.price,
                quantity: req.quantity,
                user_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("engine unavailable"))?;

        let result = reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?;
        match result {
            Ok((order_id, trades)) => Ok(Response::new(proto::SubmitOrderResponse {
                order_id,
                trades: trades
                    .into_iter()
                    .flatten()
                    .map(engine_trade_to_proto)
                    .collect(),
            })),
            Err(e) => Err(e.to_status()),
        }
    }

    async fn cancel_order(
        &self,
        request: Request<proto::CancelOrderRequest>,
    ) -> Result<Response<proto::CancelOrderResponse>, Status> {
        let req = request.into_inner();
        let pair = proto_pair_to_engine(
            req.pair
                .ok_or_else(|| Status::invalid_argument("missing trading pair"))?,
        )?;
        let order_id = req.order_id;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::CancelOrder {
                pair,
                order_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?;
        match result {
            Ok(success) => Ok(Response::new(proto::CancelOrderResponse { success })),
            Err(e) => Err(e.to_status()),
        }
    }

    async fn modify_order(
        &self,
        request: Request<proto::ModifyOrderRequest>,
    ) -> Result<Response<proto::ModifyOrderResponse>, Status> {
        let req = request.into_inner();
        let pair = proto_pair_to_engine(
            req.pair
                .ok_or_else(|| Status::invalid_argument("missing trading pair"))?,
        )?;
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("invalid user_id"))?;
        let modify = OrderModify::new(
            req.order_id,
            req.price,
            proto_side_to_engine(req.side())?,
            req.quantity,
            OrderStatus::Empty,
            user_id,
        );
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::ModifyOrder {
                pair,
                modify,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?;
        match result {
            Ok(trades) => Ok(Response::new(proto::ModifyOrderResponse {
                trades: trades
                    .into_iter()
                    .flatten()
                    .map(engine_trade_to_proto)
                    .collect(),
            })),
            Err(e) => Err(e.to_status()),
        }
    }

    async fn get_order_book(
        &self,
        request: Request<proto::GetOrderBookRequest>,
    ) -> Result<Response<proto::GetOrderBookResponse>, Status> {
        let req = request.into_inner();
        let pair = proto_pair_to_engine(
            req.pair
                .ok_or_else(|| Status::invalid_argument("missing trading pair"))?,
        )?;

        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::GetOrderBook {
                pair,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("engine unavailable"))?;

        let info = reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?
            .map_err(|e| e.to_status())?;

        let bids = info
            .get_bids()
            .iter()
            .map(|level| proto::LevelInfo {
                price: level.price,
                quantity: level.quantity,
            })
            .collect();

        let asks = info
            .get_asks()
            .iter()
            .map(|level| proto::LevelInfo {
                price: level.price,
                quantity: level.quantity,
            })
            .collect();

        Ok(Response::new(proto::GetOrderBookResponse { bids, asks }))
    }
}

#[tonic::async_trait]
impl EngineServices for EngineService {
    async fn add_trading_pair(
        &self,
        request: tonic::Request<proto::AddTradingPairRequest>,
    ) -> Result<tonic::Response<proto::AddTradingPairResponse>, Status> {
        let req = request.into_inner();
        let pair = proto_pair_to_engine(
            req.pair
                .ok_or_else(|| Status::invalid_argument("missing trading pair"))?,
        )?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::AddTradingPair {
                pair,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("engine unavailable"))?;
        match reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?
        {
            Ok(()) => Ok(Response::new(proto::AddTradingPairResponse {
                success: true,
            })),
            Err(e) => Err(e.to_status()),
        }
    }

    async fn add_user(
        &self,
        request: tonic::Request<proto::AddUserRequest>,
    ) -> Result<tonic::Response<AddUserResponse>, Status> {
        let req = request.into_inner();
        // The client generates the user id (it doubles as the primary key in
        // the Postgres user table and keeps the operation idempotent for WAL
        // replay) — the engine just registers it.
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("invalid user_id"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::AddUser {
                user_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("engine unavailable"))?;
        match reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?
        {
            Ok(()) => Ok(Response::new(AddUserResponse {
                success: true,
                user_id: user_id.to_string(),
            })),
            Err(e) => Err(e.to_status()),
        }
    }
    async fn remove_user(
        &self,
        request: tonic::Request<proto::RemoveUserRequest>,
    ) -> Result<tonic::Response<proto::RemoveUserResponse>, Status> {
        let req = request.into_inner();
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("Invalid User Id"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::RemoveUser {
                user_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("Engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("Engine crashed"))?;
        match result {
            Ok(_map) => Ok(Response::new(RemoveUserResponse { success: true })),
            Err(e) => Err(e.to_status()),
        }
    }
    async fn deposit_balance(
        &self,
        request: tonic::Request<proto::DepositBalanceRequest>,
    ) -> Result<tonic::Response<proto::DepositBalanceResponse>, Status> {
        let req = request.into_inner();
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("Invalid User Id"))?;
        let asset = proto_asset_to_engine(req.asset())
            .map_err(|_| Status::invalid_argument("Invalid Asset"))?;
        let quantity = req.quantity;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::DepositBalance {
                user_id,
                asset,
                quantity,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("Engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("Engine crashed"))?;
        match result {
            Ok(()) => Ok(Response::new(DepositBalanceResponse { success: true })),
            Err(e) => Err(e.to_status()),
        }
    }
    async fn withdraw_balance(
        &self,
        request: tonic::Request<proto::WithdrawBalanceRequest>,
    ) -> Result<tonic::Response<proto::WithdrawBalanceResponse>, Status> {
        let req = request.into_inner();
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("Invalid User Id"))?;
        let asset = proto_asset_to_engine(req.asset())
            .map_err(|_| Status::invalid_argument("Invalid Asset"))?;
        let quantity = req.quantity;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::WithdrawBalance {
                user_id,
                asset,
                quantity,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("Engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("Engine crashed"))?;
        match result {
            Ok(()) => Ok(Response::new(WithdrawBalanceResponse { success: true })),
            Err(e) => Err(e.to_status()),
        }
    }
    async fn get_balance(
        &self,
        request: tonic::Request<proto::GetBalanceRequest>,
    ) -> Result<tonic::Response<proto::GetBalanceResponse>, Status> {
        let req = request.into_inner();
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("Invalid User Id"))?;
        let asset = proto_asset_to_engine(req.asset())
            .map_err(|_| Status::invalid_argument("Invalid Asset"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::GetBalance {
                user_id,
                asset,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("Engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("Engine crashed"))?;
        match result {
            Ok(quantity) => Ok(Response::new(GetBalanceResponse { quantity })),
            Err(e) => Err(e.to_status()),
        }
    }
    async fn get_total_balance(
        &self,
        request: tonic::Request<proto::GetTotalBalanceRequest>,
    ) -> Result<tonic::Response<proto::GetTotalBalanceResponse>, Status> {
        let req = request.into_inner();
        let user_id = Uuid::parse_str(&req.user_id)
            .map_err(|_| Status::invalid_argument("Invalid User Id"))?;
        let asset = proto_asset_to_engine(req.asset())
            .map_err(|_| Status::invalid_argument("Invalid Asset"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::GetTotalBalance {
                user_id,
                asset,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Status::internal("Engine unavailable"))?;
        let result = reply_rx
            .await
            .map_err(|_| Status::internal("Engine crashed"))?;
        match result {
            Ok(quantity) => Ok(Response::new(GetTotalBalanceResponse { quantity })),
            Err(e) => Err(e.to_status()),
        }
    }
}

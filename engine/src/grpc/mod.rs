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
        DepositBalanceResponse, RemoveUserResponse, WithdrawBalanceResponse,
        engine_services_server::EngineServices, user_serivces_server::UserSerivces,
    },
    level_info::OrderBookLevelInfo,
    order::Order,
    order_modify::OrderModify,
    trade::{self, Trades},
    trading_pair::TradingPair,
    types::{Asset, OrderId, OrderStatus, OrderType, Price, Quantity, Side},
};

pub enum EngineCommand {
    // Engine service commands
    SubmitOrder {
        pair: TradingPair,
        order_type: OrderType,
        side: Side,
        price: Price,
        quantity: Quantity,
        user_id: Uuid,
        reply: oneshot::Sender<Option<(OrderId, Trades)>>,
    },
    CancelOrder {
        pair: TradingPair,
        order_id: OrderId,
        reply: oneshot::Sender<bool>,
    },
    ModifyOrder {
        pair: TradingPair,
        modify: OrderModify,
        reply: oneshot::Sender<Option<Trades>>,
    },
    GetOrderBook {
        pair: TradingPair,
        reply: oneshot::Sender<Option<OrderBookLevelInfo>>,
    },
    AddTradingPair {
        pair: TradingPair,
        reply: oneshot::Sender<()>,
    },
    // user service commands
    AddUser {
        reply: oneshot::Sender<Uuid>,
    },
    RemoveUser {
        user_id: Uuid,
        reply: oneshot::Sender<Result<HashMap<Asset, Quantity>, String>>,
    },
    DepositBalance {
        user_id: Uuid,
        asset: Asset,
        quantity: Quantity,
        reply: oneshot::Sender<Result<(), String>>,
    },
    WithdrawBalance {
        user_id: Uuid,
        asset: Asset,
        quantity: Quantity,
        reply: oneshot::Sender<Result<(), String>>,
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

pub async fn run_engine(mut rx: mpsc::Receiver<EngineCommand>, mut engine: EngineWrapper) {
    while let Some(cmd) = rx.recv().await {
        // process commands sequentially
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
                let trades = engine.add_order(&pair, &order); // main engine execution
                let has_trades = trades.is_some();
                tracing::debug!(order_id, has_trades, "submit_order complete");
                let _ = reply.send(trades.map(|t| (order_id, t))); // sending back through rpc
            }
            EngineCommand::CancelOrder {
                pair,
                order_id,
                reply,
            } => {
                tracing::debug!(%pair, order_id, "cancel_order");
                let sucess = engine.cancel_order(&pair, &order_id);
                tracing::debug!(order_id, sucess, "cancel_order complete");
                let _ = reply.send(sucess);
            }
            EngineCommand::ModifyOrder {
                pair,
                modify,
                reply,
            } => {
                tracing::debug!(%pair, "modify_order");
                let trades = engine.modify_order(&pair, modify);
                let _ = reply.send(trades);
            }
            EngineCommand::GetOrderBook { pair, reply } => {
                tracing::debug!(%pair, "get_order_book");
                let info = engine.get_order_info(&pair);
                let _ = reply.send(info);
            }
            EngineCommand::AddTradingPair { pair, reply } => {
                tracing::info!(%pair, "add_trading_pair");
                engine.add_trading_pair(pair);
                let _ = reply.send(());
            }
            EngineCommand::AddUser { reply } => {
                let user_id = Uuid::new_v4();
                tracing::info!(%user_id, "add_user");
                engine.add_user(user_id);
                let _ = reply.send(user_id);
            }
            EngineCommand::RemoveUser { user_id, reply } => {
                tracing::info!(%user_id, "remove_user");
                let result = engine.remove_user(user_id);
                let _ = reply.send(result);
            }
            EngineCommand::DepositBalance {
                user_id,
                asset,
                quantity,
                reply,
            } => {
                tracing::info!(%user_id, ?asset, quantity, "deposit_balance");
                let result = engine.deposit_balance(user_id, asset, quantity);
                let _ = reply.send(result);
            }
            EngineCommand::WithdrawBalance {
                user_id,
                asset,
                quantity,
                reply,
            } => {
                tracing::info!(%user_id, ?asset, quantity, "withdraw_balance");
                let result = engine.withdraw_balance(user_id, asset, quantity);
                let _ = reply.send(result);
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

// #[warn(dead_code)]
// fn engine_asset_to_proto(asset: crate::types::Asset) -> proto::Asset {
//     match asset {
//         crate::types::Asset::ETH => proto::Asset::Eth,
//         crate::types::Asset::SOL => proto::Asset::Sol,
//         crate::types::Asset::BTC => proto::Asset::Btc,
//         crate::types::Asset::USDC => proto::Asset::Usdc,
//         crate::types::Asset::USDT => proto::Asset::Usdt,
//     }
// }

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
impl UserSerivces for EngineService {
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
            Some((order_id, trades)) => Ok(Response::new(proto::SubmitOrderResponse {
                order_id,
                trades: trades
                    .into_iter()
                    .map(|t| engine_trade_to_proto(t))
                    .collect(),
            })),
            None => Err(Status::failed_precondition("order could not be placed")),
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

        Ok(Response::new(proto::CancelOrderResponse {
            success: result,
        }))
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
            Some(trades) => Ok(Response::new(proto::ModifyOrderResponse {
                trades: trades
                    .into_iter()
                    .map(|trade| engine_trade_to_proto(trade))
                    .collect(),
            })),
            None => Err(Status::failed_precondition("order could not be modified")),
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
            .ok_or_else(|| Status::not_found("trading pair not found"))?;

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
        reply_rx
            .await
            .map_err(|_| Status::internal("engine task crashed"))?;
        Ok(Response::new(proto::AddTradingPairResponse {
            success: true,
        }))
    }

    async fn add_user(
        &self,
        _request: tonic::Request<proto::AddUserRequest>,
    ) -> Result<tonic::Response<proto::AddUserResponse>, Status> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::AddUser { reply: reply_tx })
            .await
            .map_err(|_| Status::internal("Engine Error"))?;
        let user_id = reply_rx
            .await
            .map_err(|_| Status::internal("Engine crashed"))?
            .to_string();
        Ok(Response::new(proto::AddUserResponse { user_id }))
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
            Err(e) => Err(Status::failed_precondition(e)),
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
            Err(e) => Err(Status::failed_precondition(e)),
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
            Err(e) => Err(Status::failed_precondition(e)),
        }
    }
}

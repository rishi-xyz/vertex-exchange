use tokio::net::TcpListener;

use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};
use vertex_engine::{
    engine::{CoreEngine, EngineWrapper},
    grpc::{
        self, EngineService,
        proto::{
            self, engine_services_client::EngineServicesClient,
            engine_services_server::EngineServicesServer, user_services_client::UserServicesClient,
            user_services_server::UserServicesServer,
        },
    },
    redis::FillPublisher,
};

async fn setup() -> (UserServicesClient<Channel>, EngineServicesClient<Channel>) {
    let engine = EngineWrapper::Core(CoreEngine::new(1, 1));
    let (tx, rx) = mpsc::channel(256);
    let publisher = FillPublisher::new().await;
    let (fatal_tx, _fatal_rx) = oneshot::channel();
    tokio::spawn(grpc::run_engine(rx, engine, publisher, fatal_tx));
    let service = EngineService::new(tx);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        Server::builder()
            .add_service(UserServicesServer::new(service.clone()))
            .add_service(EngineServicesServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    let user_client = UserServicesClient::connect(format!("http://127.0.0.1:{}", port))
        .await
        .unwrap();
    let engine_client = EngineServicesClient::connect(format!("http://127.0.0.1:{}", port))
        .await
        .unwrap();

    (user_client, engine_client)
}

fn eth_usdc_pair() -> proto::TradingPair {
    proto::TradingPair {
        base: proto::Asset::Eth as i32,
        quote: proto::Asset::Usdc as i32,
    }
}

#[allow(dead_code)]
fn btc_usdc_pair() -> proto::TradingPair {
    proto::TradingPair {
        base: proto::Asset::Btc as i32,
        quote: proto::Asset::Usdc as i32,
    }
}

async fn add_eth_usdc(engine_client: &mut EngineServicesClient<Channel>) {
    engine_client
        .add_trading_pair(proto::AddTradingPairRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap();
}

fn random_user_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn add_funded_user(engine_client: &mut EngineServicesClient<Channel>) -> String {
    let user_id = uuid::Uuid::new_v4().to_string();
    engine_client
        .add_user(proto::AddUserRequest {
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 1_000_000,
        })
        .await
        .unwrap();
    engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Eth as i32,
            quantity: 100,
        })
        .await
        .unwrap();
    user_id
}

#[tokio::test]
async fn test_add_trading_pair() {
    let (_, mut engine_client) = setup().await;

    let resp = engine_client
        .add_trading_pair(proto::AddTradingPairRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.success);
}

#[tokio::test]
async fn test_add_trading_pair_duplicate() {
    let (_, mut engine_client) = setup().await;

    engine_client
        .add_trading_pair(proto::AddTradingPairRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap();

    let resp = engine_client
        .add_trading_pair(proto::AddTradingPairRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.success);
}

#[tokio::test]
async fn test_submit_order_on_added_pair() {
    let (mut user_client, mut engine_client) = setup().await;
    add_eth_usdc(&mut engine_client).await;

    let user_id = add_funded_user(&mut engine_client).await;

    let resp = user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Buy as i32,
            price: 2000,
            quantity: 10,
            user_id,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.order_id > 0);
    assert!(resp.trades.is_empty());
}

#[tokio::test]
async fn test_submit_order_on_missing_pair() {
    let (mut user_client, _) = setup().await;

    let result = user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Buy as i32,
            price: 2000,
            quantity: 10,
            user_id: random_user_id(),
        })
        .await;

    assert!(result.is_err());
    let status = result.unwrap_err();
    assert_eq!(status.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn test_submit_order_invalid_price_fails() {
    let (mut user_client, mut engine_client) = setup().await;
    add_eth_usdc(&mut engine_client).await;

    let user_id = add_funded_user(&mut engine_client).await;

    let result = user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Buy as i32,
            price: 0,
            quantity: 10,
            user_id,
        })
        .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn test_submit_order_full_lifecycle() {
    let (mut user_client, mut engine_client) = setup().await;
    add_eth_usdc(&mut engine_client).await;

    let user_id = add_funded_user(&mut engine_client).await;

    let submit_resp = user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Buy as i32,
            price: 1950,
            quantity: 5,
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();

    let order_id = submit_resp.order_id;

    let book = user_client
        .get_order_book(proto::GetOrderBookRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(book.bids.len(), 1);
    assert_eq!(book.bids[0].price, 1950);
    assert_eq!(book.bids[0].quantity, 5);
    assert!(book.asks.is_empty());

    let cancel_resp = user_client
        .cancel_order(proto::CancelOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_id,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(cancel_resp.success);

    let book = user_client
        .get_order_book(proto::GetOrderBookRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap()
        .into_inner();

    assert!(book.bids.is_empty());
}

#[tokio::test]
async fn test_cancel_order() {
    let (mut user_client, mut engine_client) = setup().await;
    add_eth_usdc(&mut engine_client).await;

    let user_id = add_funded_user(&mut engine_client).await;

    let resp = user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Sell as i32,
            price: 2100,
            quantity: 3,
            user_id,
        })
        .await
        .unwrap()
        .into_inner();

    let cancel_resp = user_client
        .cancel_order(proto::CancelOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_id: resp.order_id,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(cancel_resp.success);
}

#[tokio::test]
async fn test_modify_order() {
    let (mut user_client, mut engine_client) = setup().await;
    add_eth_usdc(&mut engine_client).await;

    let user_id = add_funded_user(&mut engine_client).await;

    let submit_resp = user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Buy as i32,
            price: 1900,
            quantity: 10,
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();

    let modify_resp = user_client
        .modify_order(proto::ModifyOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_id: submit_resp.order_id,
            price: 1950,
            quantity: 8,
            side: proto::Side::Buy as i32,
            user_id,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(modify_resp.trades.is_empty());

    let book = user_client
        .get_order_book(proto::GetOrderBookRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(book.bids.len(), 1);
    assert_eq!(book.bids[0].price, 1950);
    assert_eq!(book.bids[0].quantity, 8);
}

#[tokio::test]
async fn test_get_order_book_empty_pair() {
    let (mut user_client, mut engine_client) = setup().await;
    add_eth_usdc(&mut engine_client).await;

    let book = user_client
        .get_order_book(proto::GetOrderBookRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap()
        .into_inner();

    assert!(book.bids.is_empty());
    assert!(book.asks.is_empty());
}

#[tokio::test]
async fn test_add_user_returns_valid_uuid() {
    let (_, mut engine_client) = setup().await;
    let user_id = uuid::Uuid::new_v4();
    let resp = engine_client
        .add_user(proto::AddUserRequest {
            user_id: user_id.to_string(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(resp.success);
    // The engine echoes the client-provided id back.
    assert_eq!(resp.user_id, user_id.to_string());
    let uid = uuid::Uuid::parse_str(&resp.user_id);
    assert!(uid.is_ok());
}

async fn add_user(engine_client: &mut EngineServicesClient<Channel>) -> String {
    let user_id = uuid::Uuid::new_v4().to_string();
    engine_client
        .add_user(proto::AddUserRequest {
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner()
        .user_id
}

#[tokio::test]
async fn test_deposit_balance() {
    let (_, mut engine_client) = setup().await;
    let user_id = add_user(&mut engine_client).await;
    let resp = engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 10000,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(resp.success);
}

#[tokio::test]
async fn test_withdraw_balance() {
    let (_, mut engine_client) = setup().await;
    let user_id = add_user(&mut engine_client).await;
    engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 5000,
        })
        .await
        .unwrap();
    let resp = engine_client
        .withdraw_balance(proto::WithdrawBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 2000,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(resp.success);
}

#[tokio::test]
async fn test_withdraw_insufficient_balance_fails() {
    let (_, mut engine_client) = setup().await;
    let user_id = add_user(&mut engine_client).await;
    engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 1000,
        })
        .await
        .unwrap();
    let result = engine_client
        .withdraw_balance(proto::WithdrawBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 2000,
        })
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn test_remove_user_returns_success() {
    let (_, mut engine_client) = setup().await;
    let user_id = add_user(&mut engine_client).await;
    let resp = engine_client
        .remove_user(proto::RemoveUserRequest {
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(resp.success);
}

#[tokio::test]
async fn test_add_user_then_deposit_withdraw_and_remove() {
    let (_, mut engine_client) = setup().await;
    let user_id = add_user(&mut engine_client).await;
    engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 10000,
        })
        .await
        .unwrap();
    engine_client
        .withdraw_balance(proto::WithdrawBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 3000,
        })
        .await
        .unwrap();
    let resp = engine_client
        .remove_user(proto::RemoveUserRequest {
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(resp.success);
}

#[tokio::test]
async fn test_get_balance_returns_available() {
    let (mut user_client, mut engine_client) = setup().await;
    let user_id = add_funded_user(&mut engine_client).await;
    engine_client
        .add_trading_pair(proto::AddTradingPairRequest {
            pair: Some(eth_usdc_pair()),
        })
        .await
        .unwrap();
    user_client
        .submit_order(proto::SubmitOrderRequest {
            pair: Some(eth_usdc_pair()),
            order_type: proto::OrderType::GoodTillCancel as i32,
            side: proto::Side::Buy as i32,
            price: 10,
            quantity: 10,
            user_id: user_id.clone(),
        })
        .await
        .unwrap();
    let available = engine_client
        .get_balance(proto::GetBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
        })
        .await
        .unwrap()
        .into_inner();
    let total = engine_client
        .get_total_balance(proto::GetTotalBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(available.quantity, 1_000_000 - 100);
    assert_eq!(total.quantity, 1_000_000);
}

#[tokio::test]
async fn test_get_balance_missing_user_fails() {
    let (_, mut engine_client) = setup().await;
    let result = engine_client
        .get_balance(proto::GetBalanceRequest {
            user_id: random_user_id(),
            asset: proto::Asset::Usdc as i32,
        })
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    let result = engine_client
        .get_total_balance(proto::GetTotalBalanceRequest {
            user_id: random_user_id(),
            asset: proto::Asset::Usdc as i32,
        })
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn test_deposit_balance_missing_user_fails() {
    let (_, mut engine_client) = setup().await;
    let result = engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: random_user_id(),
            asset: proto::Asset::Usdc as i32,
            quantity: 1000,
        })
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn test_add_user_idempotent() {
    let (_, mut engine_client) = setup().await;
    let user_id = uuid::Uuid::new_v4().to_string();

    let first = engine_client
        .add_user(proto::AddUserRequest {
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(first.success);
    assert_eq!(first.user_id, user_id);

    // Re-adding the same user is idempotent, not an error.
    let second = engine_client
        .add_user(proto::AddUserRequest {
            user_id: user_id.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(second.success);
    assert_eq!(second.user_id, user_id);

    // The user's state is usable after the idempotent re-add.
    engine_client
        .deposit_balance(proto::DepositBalanceRequest {
            user_id: user_id.clone(),
            asset: proto::Asset::Usdc as i32,
            quantity: 1000,
        })
        .await
        .unwrap();
}

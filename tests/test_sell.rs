mod common;

#[cfg(test)]
mod test {
    use log::error;
    use std::{fs, net::SocketAddr};

    use fatcrab_trading::{
        common::BlockchainInfo,
        maker::{FatCrabMakerNotif, FatCrabMakerState},
        order::{FatCrabOrder, FatCrabOrderType},
        taker::{FatCrabTakerNotif, FatCrabTakerState},
        trade_rsp::FatCrabTradeRspType,
        trader::FatCrabTrader,
    };
    use url::Url;
    use uuid::Uuid;

    use super::common::{logger::setup as logger_setup, node::Node, relay::Relay};

    #[tokio::test]
    async fn test_sell_order() {
        const TAKER_BALANCE: u64 = 2000000;
        const MAKER_BALANCE: u64 = 3000000;
        const PURCHASE_AMOUNT: f64 = 200.0;
        const PURCHASE_PRICE: f64 = 1000.0;

        // logger_setup();

        // Setup initial state
        if let Some(error) = fs::remove_dir_all("n3xb_data/").err() {
            error!("Failed to remove /n3xb_data/ directory: {}", error);
        }
        if let Some(error) = fs::remove_dir_all("fatcrab_data/").err() {
            error!("Failed to remove /fatcrab_data/ directory: {}", error);
        }

        // Initialize Regtest Bitcoin blockchain & mine 101 blocks
        let node = Node::new();

        // Initialize Relays
        let mut relays: Vec<Relay> = Vec::new();

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        // Maker - Create Fatcrab Trader for Maker
        let info = BlockchainInfo::Rpc {
            url: node.url(),
            auth: node.auth(),
            network: node.network(),
        };
        let trader_m = FatCrabTrader::new(info.clone(), "").await;

        // Taker - Create Fatcrab Trader for Taker
        let trader_t = FatCrabTrader::new(info, "").await;

        // Add Relays
        let mut relay_addrs: Vec<(Url, Option<SocketAddr>)> = Vec::new();

        for relay in relays.iter_mut() {
            let url = Url::parse(format!("{}:{}", "ws://localhost", relay.port).as_str()).unwrap();
            relay_addrs.push((url, None));
        }

        trader_m.add_relays(relay_addrs.clone()).await.unwrap();
        trader_t.add_relays(relay_addrs).await.unwrap();

        // Maker - Fund Maker Fatcrab Trader internal wallet from miner
        let address_m1 = trader_m.wallet_generate_receive_address().await.unwrap();
        let _txid_m1 = node.send_to_address(address_m1, MAKER_BALANCE);

        // Taker - Fund Taker Fatcrab Trader internal wallet from miner
        let address_t1 = trader_t.wallet_generate_receive_address().await.unwrap();
        let _txid_t1 = node.send_to_address(address_t1, TAKER_BALANCE);

        // Check wallet funding
        node.generate_blocks(1);

        trader_m.wallet_blockchain_sync().await.unwrap();
        let trader_m_balance = trader_m.wallet_spendable_balance().await.unwrap();
        let trader_m_spendable_balance = trader_m_balance.confirmed - trader_m_balance.allocated;
        assert_eq!(trader_m_spendable_balance, MAKER_BALANCE);

        trader_t.wallet_blockchain_sync().await.unwrap();
        let trader_t_balance = trader_t.wallet_spendable_balance().await.unwrap();
        let trader_t_spendable_balance = trader_t_balance.confirmed - trader_t_balance.allocated;
        assert_eq!(trader_t_spendable_balance, TAKER_BALANCE);

        // Maker - Create Sell Order
        let order = FatCrabOrder {
            order_type: FatCrabOrderType::Sell,
            trade_uuid: Uuid::new_v4(),
            amount: PURCHASE_AMOUNT,
            price: PURCHASE_PRICE,
        };

        // Maker - Create Fatcrab Maker
        let maker = trader_m.new_sell_maker(&order).await.unwrap();

        let maker_state = maker.get_state().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::New));

        // Maker Create channels & register Notif Tx
        let (maker_notif_tx, mut maker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
        maker.register_notif_tx(maker_notif_tx).await.unwrap();

        let maker_state = maker.post_new_order().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::WaitingForOffers));
        let maker_state = maker.get_state().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::WaitingForOffers));

        // Taker - Query Fatcrab Trade Order
        let orders = trader_t
            .query_orders(Some(FatCrabOrderType::Buy))
            .await
            .unwrap();
        assert_eq!(orders.len(), 0);

        let orders = trader_t
            .query_orders(Some(FatCrabOrderType::Sell))
            .await
            .unwrap();
        assert_eq!(orders.len(), 1);

        let order_envelope = &orders[0];
        let order = &order_envelope.order;
        assert_eq!(order.amount, PURCHASE_AMOUNT);
        assert_eq!(order.price, PURCHASE_PRICE);

        // Taker - Create Fatcrab Take Trader & Take Trade Order
        let taker_receive_fatcrab_addr = Uuid::new_v4().to_string();
        let taker = trader_t
            .new_sell_taker(&orders[0], taker_receive_fatcrab_addr)
            .await
            .unwrap();

        let taker_state = taker.get_state().await.unwrap();
        assert!(matches!(taker_state, FatCrabTakerState::New));

        let (taker_notif_tx, mut taker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
        taker.register_notif_tx(taker_notif_tx).await.unwrap();

        let taker_state = taker.take_order().await.unwrap();
        assert!(matches!(taker_state, FatCrabTakerState::SubmittedOffer));
        let taker_state = taker.get_state().await.unwrap();
        assert!(matches!(taker_state, FatCrabTakerState::SubmittedOffer));

        // Maker - Wait for Fatcrab Trader Order to be taken
        let maker_notif = maker_notif_rx.recv().await.unwrap();
        let offer_notif = match maker_notif {
            FatCrabMakerNotif::Offer(offer_notif) => offer_notif,
            _ => {
                panic!("Maker only expects Sell Offer Notif at this point");
            }
        };

        assert!(matches!(
            offer_notif.state,
            FatCrabMakerState::ReceivedOffer
        ));
        let maker_state = maker.get_state().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::ReceivedOffer));

        // Maker - Send Fatcrab Trade Response w/ BTC address
        let trade_rsp_type = FatCrabTradeRspType::Accept;
        let maker_state = maker
            .trade_response(trade_rsp_type, offer_notif.offer_envelope)
            .await
            .unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::AcceptedOffer));
        let maker_state = maker.get_state().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::AcceptedOffer));

        // Taker should auto remit BTC, auto peer notify with TxID and FatCrab address
        let taker_notif = taker_notif_rx.recv().await.unwrap();
        let trade_rsp_notif = match taker_notif {
            FatCrabTakerNotif::TradeRsp(trade_rsp_notif) => trade_rsp_notif,
            _ => {
                panic!("Taker only expects Trade Response Notif at this point");
            }
        };
        assert!(matches!(
            trade_rsp_notif.state,
            FatCrabTakerState::NotifiedOutbound
        ));
        let taker_state = taker.get_state().await.unwrap();
        assert!(matches!(taker_state, FatCrabTakerState::NotifiedOutbound));

        // Maker - Wait for Peer Message
        let maker_notif = maker_notif_rx.recv().await.unwrap();
        let peer_notif = match maker_notif {
            FatCrabMakerNotif::Peer(peer_notif) => peer_notif,
            _ => {
                panic!("Maker only expects Peer Message at this point");
            }
        };

        let _btc_txid = peer_notif.peer_envelope.message.txid;
        assert!(matches!(
            peer_notif.state,
            FatCrabMakerState::InboundBtcNotified
        ));

        // Mine several blocks to confirm Bitcoin Tx
        node.generate_blocks(10);
        trader_m.wallet_blockchain_sync().await.unwrap();
        trader_t.wallet_blockchain_sync().await.unwrap();

        // Maker - Confirm Bitcoin Tx
        let tx_conf = maker.check_btc_tx_confirmation().await.unwrap();
        assert_eq!(tx_conf, 10 - 1);

        // Confirm Bitcoin Balances
        let trader_m_balance = trader_m.wallet_spendable_balance().await.unwrap();
        let trader_m_spendable_balance = trader_m_balance.confirmed - trader_m_balance.allocated;
        assert_eq!(
            trader_m_spendable_balance,
            MAKER_BALANCE + (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64
        );

        let trader_t_balance = trader_t.wallet_spendable_balance().await.unwrap();
        let trader_t_spendable_balance = trader_t_balance.confirmed - trader_t_balance.allocated;
        assert!(
            trader_t_spendable_balance < TAKER_BALANCE - (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64
        );

        // Maker - *User remits Fatcrabs

        // Maker - User claims Fatcrab remittance, send Peer Message
        let maker_fatcrab_remittance_txid = Uuid::new_v4().to_string();
        let maker_state = maker
            .notify_peer(&maker_fatcrab_remittance_txid)
            .await
            .unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::NotifiedOutbound));

        let maker_state = maker.get_state().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::NotifiedOutbound));

        let maker_state = maker.trade_complete().await.unwrap();
        assert!(matches!(maker_state, FatCrabMakerState::TradeCompleted));

        // Taker - Wait for Fatcrab Peer Message
        let taker_notif = taker_notif_rx.recv().await.unwrap();
        let peer_notif = match taker_notif {
            FatCrabTakerNotif::Peer(peer_notif) => peer_notif,
            _ => {
                panic!("Taker only expects Peer Message at this point");
            }
        };

        assert_eq!(
            &peer_notif.peer_envelope.message.txid,
            &maker_fatcrab_remittance_txid
        );
        assert_eq!(
            matches!(peer_notif.state, FatCrabTakerState::InboundFcNotified),
            true
        );
        let taker_state = taker.get_state().await.unwrap();
        assert!(matches!(taker_state, FatCrabTakerState::InboundFcNotified));

        let taker_state = taker.trade_complete().await.unwrap();
        assert!(matches!(taker_state, FatCrabTakerState::TradeCompleted));

        // Trader Shutdown
        trader_m.shutdown().await.unwrap();
        trader_t.shutdown().await.unwrap();

        // Relays Shutdown
        relays.into_iter().for_each(|r| r.shutdown().unwrap());
    }
}

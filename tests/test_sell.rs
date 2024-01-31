mod common;

#[cfg(test)]
mod test {
    use std::net::SocketAddr;

    use fatcrab_trading::{
        common::BlockchainInfo,
        maker::FatCrabMakerNotif,
        order::{FatCrabOrder, FatCrabOrderType},
        taker::FatCrabTakerNotif,
        trade_rsp::FatCrabTradeRspType,
        trader::FatCrabTrader,
    };
    use url::Url;
    use uuid::Uuid;

    use super::common::{node::Node, relay::Relay};

    #[tokio::test]
    async fn test_sell_order() {
        const TAKER_BALANCE: u64 = 2000000;
        const MAKER_BALANCE: u64 = 3000000;
        const PURCHASE_AMOUNT: f64 = 200.0;
        const PURCHASE_PRICE: f64 = 1000.0;

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
        assert_eq!(
            trader_m.wallet_spendable_balance().await.unwrap(),
            MAKER_BALANCE
        );
        trader_t.wallet_blockchain_sync().await.unwrap();
        assert_eq!(
            trader_t.wallet_spendable_balance().await.unwrap(),
            TAKER_BALANCE
        );

        // Maker - Create Sell Order
        let order = FatCrabOrder {
            order_type: FatCrabOrderType::Sell,
            trade_uuid: Uuid::new_v4(),
            amount: PURCHASE_AMOUNT,
            price: PURCHASE_PRICE,
        };

        // Maker - Create Fatcrab Maker
        let maker = trader_m.new_sell_maker(&order).await.unwrap();

        // Maker Create channels & register Notif Tx
        let (maker_notif_tx, mut maker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
        maker.register_notif_tx(maker_notif_tx).await.unwrap();

        maker.post_new_order().await.unwrap();

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

        // Taker - Create Fatcrab Take Trader & Take Trade Order
        let taker_receive_fatcrab_addr = Uuid::new_v4().to_string();
        let taker = trader_t
            .new_sell_taker(&orders[0], taker_receive_fatcrab_addr)
            .await
            .unwrap();

        let (taker_notif_tx, mut taker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
        taker.register_notif_tx(taker_notif_tx).await.unwrap();

        taker.take_order().await.unwrap();

        // Maker - Wait for Fatcrab Trader Order to be taken
        let maker_notif = maker_notif_rx.recv().await.unwrap();
        let offer_envelope = match maker_notif {
            FatCrabMakerNotif::Offer(offer_envelope) => offer_envelope,
            _ => {
                panic!("Maker only expects Sell Offer Notif at this point");
            }
        };

        // Maker - Send Fatcrab Trade Response w/ BTC address
        let trade_rsp_type = FatCrabTradeRspType::Accept;
        maker
            .trade_response(trade_rsp_type, offer_envelope)
            .await
            .unwrap();

        // Taker should auto remit BTC, auto peer notify with TxID and FatCrab address

        // Maker - Wait for Peer Message
        let maker_notif = maker_notif_rx.recv().await.unwrap();
        let _btc_txid = match maker_notif {
            FatCrabMakerNotif::Peer(peer_msg_envelope) => peer_msg_envelope.message.txid,
            _ => {
                panic!("Maker only expects Peer Message at this point");
            }
        };

        // Mine several blocks to confirm Bitcoin Tx
        node.generate_blocks(10);
        trader_m.wallet_blockchain_sync().await.unwrap();
        trader_t.wallet_blockchain_sync().await.unwrap();

        // Maker - Confirm Bitcoin Tx
        let tx_conf = maker.check_btc_tx_confirmation().await.unwrap();
        assert_eq!(tx_conf, 10 - 1);

        // Confirm Bitcoin Balances
        let trader_m_balance = trader_m.wallet_spendable_balance().await.unwrap();
        assert_eq!(
            trader_m_balance,
            MAKER_BALANCE + (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64
        );

        let trader_t_balance = trader_t.wallet_spendable_balance().await.unwrap();
        assert!(trader_t_balance < TAKER_BALANCE - (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64);

        // Maker - *User remits Fatcrabs

        // Maker - User claims Fatcrab remittance, send Peer Message
        let maker_fatcrab_remittance_txid = Uuid::new_v4().to_string();
        maker
            .notify_peer(&maker_fatcrab_remittance_txid)
            .await
            .unwrap();

        maker.trade_complete().await.unwrap();

        // Taker - Wait for Fatcrab Peer Message
        let taker_notif = taker_notif_rx.recv().await.unwrap();
        match taker_notif {
            FatCrabTakerNotif::Peer(peer_msg_envelope) => {
                assert_eq!(
                    &peer_msg_envelope.message.txid,
                    &maker_fatcrab_remittance_txid
                );
            }
            _ => {
                panic!("Taker only expects Peer Message at this point");
            }
        }

        taker.trade_complete().await.unwrap();

        // Trader Shutdown
        trader_m.shutdown().await.unwrap();
        trader_t.shutdown().await.unwrap();

        // Relays Shutdown
        relays.into_iter().for_each(|r| r.shutdown().unwrap());
    }
}

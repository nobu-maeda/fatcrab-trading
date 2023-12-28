mod common;

#[cfg(test)]
mod test {
    use std::{net::SocketAddr, str::FromStr};

    use fatcrab_trading::{
        common::BlockchainInfo,
        maker::FatCrabMakerNotif,
        order::{FatCrabOrder, FatCrabOrderType},
        taker::FatCrabTakerNotif,
        trade_rsp::{FatCrabTradeRsp, FatCrabTradeRspType},
        trader::FatCrabTrader,
    };
    use secp256k1::SecretKey;
    use url::Url;
    use uuid::Uuid;

    use super::common::{node::Node, relay::Relay};

    #[tokio::test]
    async fn test_buy_order() {
        const MAKER_BALANCE: u64 = 1000000;
        const PURCHASE_AMOUNT: f64 = 100.0;
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
        let privkey_m =
            SecretKey::from_str("9709e361864037ef7b929c2b36dc36155568e9a066291dfadc79ed5d106e59f8")
                .unwrap();
        let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;

        // Maker - Fund Maker Fatcrab Trader internal wallet from miner
        let address_m1 = trader_m.wallet_generate_receive_address().await.unwrap();
        let _txid1 = node.send_to_address(address_m1, MAKER_BALANCE);
        node.generate_blocks(1);
        trader_m.wallet_blockchain_sync().await.unwrap();
        assert_eq!(
            trader_m.wallet_spendable_balance().await.unwrap(),
            MAKER_BALANCE
        );

        // Taker - Create Fatcrab Trader for Taker
        let privkey_t =
            SecretKey::from_str("80e6f8e839135232972dfc16f2acdaeee9c0bcb4793a8a8249b7e384a51377e1")
                .unwrap();
        let trader_t = FatCrabTrader::new_with_key(privkey_t, info, "").await;

        // Add Relays
        let mut relay_addrs: Vec<(Url, Option<SocketAddr>)> = Vec::new();

        for relay in relays.iter_mut() {
            let url = Url::parse(&format!("{}:{}", "ws://localhost", relay.port)).unwrap();
            relay_addrs.push((url, None));
        }

        trader_m.add_relays(relay_addrs.clone()).await.unwrap();
        trader_t.add_relays(relay_addrs).await.unwrap();

        // Maker - Create Fatcrab Trade Order
        let maker_receive_fatcrab_addr = Uuid::new_v4().to_string();
        let order = FatCrabOrder {
            order_type: FatCrabOrderType::Buy,
            trade_uuid: Uuid::new_v4(),
            amount: PURCHASE_AMOUNT,
            price: PURCHASE_PRICE,
        };

        // Maker - Create Fatcrab Maker
        let maker = trader_m
            .make_buy_order(&order, maker_receive_fatcrab_addr.clone())
            .await;

        // Maker - Create channels & register Notif Tx
        let (maker_notif_tx, mut maker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
        maker.register_notif_tx(maker_notif_tx).await.unwrap();

        // Taker - Query Fatcrab Trade Order
        let orders = trader_t
            .query_orders(Some(FatCrabOrderType::Sell))
            .await
            .unwrap();
        assert_eq!(orders.len(), 0);

        let orders = trader_t
            .query_orders(Some(FatCrabOrderType::Buy))
            .await
            .unwrap();
        assert_eq!(orders.len(), 1);

        // Taker - Create Fatcrab Take Trader & Take Trade Order
        let taker = trader_t.take_buy_order(&orders[0]).await;

        let (taker_notif_tx, mut taker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
        taker.register_notif_tx(taker_notif_tx).await.unwrap();

        // Maker - Wait for Fatcrab Trader Order to be taken
        let maker_notif = maker_notif_rx.recv().await.unwrap();
        let offer_envelope = match maker_notif {
            FatCrabMakerNotif::Offer(offer_envelope) => offer_envelope,
            _ => {
                panic!("Maker only expects Buy Offer Notif at this point");
            }
        };

        // Maker - Send Fatcrab Trade Response w/ Fatcrab address
        let trade_rsp_type = FatCrabTradeRspType::Accept;
        maker
            .trade_response(trade_rsp_type, offer_envelope)
            .await
            .unwrap();

        // Taker - Wait for Fatcrab Trade Response
        let taker_notif = taker_notif_rx.recv().await.unwrap();
        let maker_remitted_fatcrab_acct_id_string = match taker_notif {
            FatCrabTakerNotif::TradeRsp(trade_rsp_envelope) => match trade_rsp_envelope.trade_rsp {
                FatCrabTradeRsp::Accept { receive_address } => receive_address,
                _ => {
                    panic!("Taker only expects Accepted Trade Response at this point");
                }
            },
            _ => {
                panic!("Taker only expects Accepted Trade Response at this point");
            }
        };
        assert_eq!(
            maker_remitted_fatcrab_acct_id_string,
            maker_receive_fatcrab_addr
        );

        // Taker - *User remits Fatcrabs

        // Taker - User claims Fatcrab remittance, send Peer Message with Bitcoin address
        let taker_fatcrab_remittance_txid = Uuid::new_v4().to_string();
        taker
            .notify_peer(&taker_fatcrab_remittance_txid)
            .await
            .unwrap();

        // Maker - Wait for Fatcrab Peer Message
        let maker_notif = maker_notif_rx.recv().await.unwrap();
        match maker_notif {
            FatCrabMakerNotif::Peer(peer_msg_envelope) => {
                assert_eq!(
                    &peer_msg_envelope.message.txid,
                    &taker_fatcrab_remittance_txid
                );
            }
            _ => {
                panic!("Maker only expects Peer Message at this point");
            }
        }

        // Maker - *Confirms Fatcrab have been received

        // Maker - Release Bitcoin to Taker Bitcoin address
        maker.release_notify_peer().await.unwrap();

        // Maker - Trade Completion
        maker.trade_complete().await.unwrap();

        // Taker - Wait for Fatcrab Peer Message
        let taker_notif = taker_notif_rx.recv().await.unwrap();
        let _btc_txid = match taker_notif {
            FatCrabTakerNotif::Peer(peer_msg_envelope) => peer_msg_envelope.message.txid,
            _ => {
                panic!("Taker only expects Peer Message at this point");
            }
        };

        // Mine several blocks to confirm Bitcoin Tx
        node.generate_blocks(10);
        trader_m.wallet_blockchain_sync().await.unwrap();
        trader_t.wallet_blockchain_sync().await.unwrap();

        // Taker - Confirm Bitcoin Tx
        let tx_conf = taker.check_btc_tx_confirmation().await.unwrap();
        assert_eq!(tx_conf, 10 - 1);

        // Confirm Bitcoin Balances
        let trader_t_balance = trader_t.wallet_spendable_balance().await.unwrap();
        assert_eq!(trader_t_balance as f64, PURCHASE_AMOUNT * PURCHASE_PRICE);

        let trader_m_balance = trader_m.wallet_spendable_balance().await.unwrap();
        assert!(trader_m_balance < MAKER_BALANCE - (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64);

        // Taker - Trade Completion
        taker.trade_complete().await.unwrap();

        // Trader Shutdown
        trader_m.shutdown().await.unwrap();
        trader_t.shutdown().await.unwrap();

        // Relays Shutdown
        relays.into_iter().for_each(|r| r.shutdown().unwrap());
    }
}

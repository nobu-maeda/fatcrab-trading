mod common;

#[cfg(test)]

mod test {
    use crusty_n3xb::taker;
    use log::error;
    use secp256k1::SecretKey;
    use std::{fs, net::SocketAddr, str::FromStr, time::Duration};
    use uuid::Uuid;

    use tokio::time::sleep;
    use url::Url;

    use fatcrab_trading::{
        common::BlockchainInfo,
        maker::{FatCrabMakerAccessEnum, FatCrabMakerNotif},
        order::{FatCrabOrder, FatCrabOrderType},
        taker::{FatCrabTakerAccessEnum, FatCrabTakerNotif, FatCrabTakerState},
        trade_rsp::FatCrabTradeRspType,
        trader::FatCrabTrader,
    };

    use super::common::{logger::setup as logger_setup, node::Node, relay::Relay};

    #[tokio::test]
    async fn test_restore_sell() {
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

        let mut relays: Vec<Relay> = Vec::new();

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let mut relay_addrs: Vec<(Url, Option<SocketAddr>)> = Vec::new();

        for relay in relays.iter_mut() {
            let url = Url::parse(&format!("{}:{}", "ws://localhost", relay.port)).unwrap();
            relay_addrs.push((url, None));
        }

        let node = Node::new();

        let info = BlockchainInfo::Rpc {
            url: node.url(),
            auth: node.auth(),
            network: node.network(),
        };

        let privkey_m =
            SecretKey::from_str("9709e361864037ef7b929c2b36dc36155568e9a066291dfadc79ed5d106e59f8")
                .unwrap();

        let privkey_t =
            SecretKey::from_str("80e6f8e839135232972dfc16f2acdaeee9c0bcb4793a8a8249b7e384a51377e1")
                .unwrap();

        let trade_uuid = Uuid::new_v4();
        let taker_receive_fatcrab_addr = Uuid::new_v4().to_string();

        // Add Relays
        {
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;

            trader_m.add_relays(relay_addrs.clone()).await.unwrap();
            trader_t.add_relays(relay_addrs.clone()).await.unwrap();

            trader_m.shutdown().await.unwrap();
            trader_t.shutdown().await.unwrap();
        }

        // Fund Taker Trader
        {
            println!("Fund Taker Trader");
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;

            trader_m.reconnect().await.unwrap();
            trader_t.reconnect().await.unwrap();

            // Check relays as expected
            let relays_info = trader_m.get_relays().await;
            relays_info.iter().for_each(|relay_info| {
                assert_eq!(relay_info.url, relay_addrs[0].0);
                assert_eq!(relays_info.len(), relay_addrs.len());
            });

            let relays_info = trader_t.get_relays().await;
            relays_info.iter().for_each(|relay_info| {
                assert_eq!(relay_info.url, relay_addrs[0].0);
                assert_eq!(relays_info.len(), relay_addrs.len());
            });

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

            trader_m.shutdown().await.unwrap();
            trader_t.shutdown().await.unwrap();
        }

        // New Maker
        {
            println!("New Maker");
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            trader_m.reconnect().await.unwrap();
            trader_m.wallet_blockchain_sync().await.unwrap();

            // Check wallet balance as expected
            assert_eq!(
                trader_m.wallet_spendable_balance().await.unwrap(),
                MAKER_BALANCE
            );

            // Maker - Create Sell Order
            let order = FatCrabOrder {
                order_type: FatCrabOrderType::Sell,
                trade_uuid,
                amount: PURCHASE_AMOUNT,
                price: PURCHASE_PRICE,
            };

            // Maker - Create Fatcrab Maker
            let maker = trader_m.new_sell_maker(&order).await.unwrap();

            maker.shutdown().await.unwrap();
            trader_m.shutdown().await.unwrap();
        }

        // Post Order
        {
            println!("Post Order");

            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            trader_m.reconnect().await.unwrap();

            let makers = trader_m.get_makers().await;
            let maker_enum = makers.get(&trade_uuid).unwrap().to_owned();
            let maker = match maker_enum {
                FatCrabMakerAccessEnum::Sell(maker_access) => maker_access,
                _ => panic!("Maker is not a Sell Maker"),
            };

            maker.post_new_order().await.unwrap();
            maker.shutdown().await.unwrap();
            trader_m.shutdown().await.unwrap();

            // Create New Takers
            println!("Create New Takers");
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;
            trader_t.reconnect().await.unwrap();
            trader_t.wallet_blockchain_sync().await.unwrap();

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
            let taker = trader_t
                .new_sell_taker(&orders[0], taker_receive_fatcrab_addr)
                .await
                .unwrap();
            taker.shutdown().await.unwrap();
            trader_t.shutdown().await.unwrap();
        }

        // Taker Order
        {
            println!("Taker Order");
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;
            let takers = trader_t.get_takers().await;
            let taker_enum = takers.get(&trade_uuid).unwrap().to_owned();
            let taker = match taker_enum {
                FatCrabTakerAccessEnum::Sell(taker_access) => taker_access,
                _ => panic!("Taker is not a Sell Taker"),
            };
            let (taker_notif_tx, mut _taker_notif_rx) =
                tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
            taker.register_notif_tx(taker_notif_tx).await.unwrap();
            trader_t.reconnect().await.unwrap();

            taker.take_order().await.unwrap();
            taker.shutdown().await.unwrap();
            trader_t.shutdown().await.unwrap();

            sleep(Duration::from_secs(1)).await;

            // Wait for Offer
            println!("Wait for Offer");
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            let makers = trader_m.get_makers().await;
            let maker_enum = makers.get(&trade_uuid).unwrap().to_owned();
            let maker = match maker_enum {
                FatCrabMakerAccessEnum::Sell(maker_access) => maker_access,
                _ => panic!("Maker is not a Sell Maker"),
            };
            let (maker_notif_tx, mut maker_notif_rx) =
                tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
            maker.register_notif_tx(maker_notif_tx).await.unwrap();
            trader_m.reconnect().await.unwrap();

            let maker_notif = maker_notif_rx.recv().await.unwrap();
            let _ = match maker_notif {
                FatCrabMakerNotif::Offer(offer_envelope) => offer_envelope,
                _ => {
                    panic!("Maker only expects Sell Offer Notif at this point");
                }
            };
            maker.shutdown().await.unwrap();
            trader_m.shutdown().await.unwrap();
        }

        // Accept Offer
        {
            println!("Accept Offer - Restore Maker");
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            let makers = trader_m.get_makers().await;
            let maker_enum = makers.get(&trade_uuid).unwrap().to_owned();
            let maker = match maker_enum {
                FatCrabMakerAccessEnum::Sell(maker_access) => maker_access,
                _ => panic!("Maker is not a Sell Maker"),
            };

            let (maker_notif_tx, _) = tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
            maker.register_notif_tx(maker_notif_tx).await.unwrap();
            trader_m.reconnect().await.unwrap();

            let offer_envelopes = maker.query_offers().await.unwrap();
            assert!(offer_envelopes.len() >= 1);
            let offer_envelope = offer_envelopes.first().unwrap().to_owned();

            // Maker - Send Fatcrab Trade Response w/ BTC address
            println!("Accept Offer - Trade Response");
            let trade_rsp_type = FatCrabTradeRspType::Accept;
            maker
                .trade_response(trade_rsp_type, offer_envelope)
                .await
                .unwrap();

            maker.shutdown().await.unwrap();
            trader_m.shutdown().await.unwrap();

            sleep(Duration::from_secs(1)).await;

            // Taker should auto remit BTC, auto peer notify with TxID and FatCrab address
            println!("Taker BTC auto-remit");
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;
            trader_t.wallet_blockchain_sync().await.unwrap();

            let takers = trader_t.get_takers().await;
            let taker_enum = takers.get(&trade_uuid).unwrap().to_owned();
            let taker = match taker_enum {
                FatCrabTakerAccessEnum::Sell(taker_access) => taker_access,
                _ => panic!("Taker is not a Sell Taker"),
            };
            let (taker_notif_tx, mut taker_notif_rx) =
                tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
            taker.register_notif_tx(taker_notif_tx).await.unwrap();
            trader_t.reconnect().await.unwrap();

            let taker_notif = taker_notif_rx.recv().await.unwrap();
            let _ = match taker_notif {
                FatCrabTakerNotif::TradeRsp(trade_rsp_notif) => match trade_rsp_notif.state {
                    FatCrabTakerState::NotifiedOutbound => {}
                    _ => {
                        panic!("Taker only expects BTC to be auto-remitted and state jumps directly to Notified Outbound at this point");
                    }
                },
                _ => {
                    panic!("Taker only expects Trade Response at this point");
                }
            };

            taker.shutdown().await.unwrap();
            trader_t.shutdown().await.unwrap();

            sleep(Duration::from_secs(1)).await;

            // Maker receives Peer Message of Taker remitting BTC
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            let makers = trader_m.get_makers().await;
            let maker_enum = makers.get(&trade_uuid).unwrap().to_owned();
            let maker = match maker_enum {
                FatCrabMakerAccessEnum::Sell(maker_access) => maker_access,
                _ => panic!("Maker is not a Sell Maker"),
            };
            let (maker_notif_tx, mut maker_notif_rx) =
                tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
            maker.register_notif_tx(maker_notif_tx).await.unwrap();
            trader_m.reconnect().await.unwrap();

            let maker_notif = maker_notif_rx.recv().await.unwrap();
            let _ = match maker_notif {
                FatCrabMakerNotif::Peer(peer_notif) => peer_notif.peer_envelope.message.txid,
                _ => {
                    panic!("Maker only expects Peer Notif at this point");
                }
            };

            maker.shutdown().await.unwrap();
            trader_m.shutdown().await.unwrap();
        }

        // Mine several blocks to confirm Bitcoin Tx
        node.generate_blocks(10);

        let maker_fatcrab_remittance_txid = Uuid::new_v4().to_string();

        // Maker - User Remits Fatcrabs
        {
            println!("Maker - User Remits Fatcrabs");
            let trader_m = FatCrabTrader::new_with_key(privkey_m, info.clone(), "").await;
            trader_m.wallet_blockchain_sync().await.unwrap();

            let makers = trader_m.get_makers().await;
            let maker_enum = makers.get(&trade_uuid).unwrap().to_owned();
            let maker = match maker_enum {
                FatCrabMakerAccessEnum::Sell(maker_access) => maker_access,
                _ => panic!("Maker is not a Sell Maker"),
            };
            let (maker_notif_tx, _) = tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
            maker.register_notif_tx(maker_notif_tx).await.unwrap();
            trader_m.reconnect().await.unwrap();

            let _peer_msg_envelope = maker.query_peer_msg().await.unwrap().unwrap();

            // Maker - Confirm Bitcoin Tx
            let tx_conf = maker.check_btc_tx_confirmation().await.unwrap();
            assert_eq!(tx_conf, 10 - 1);

            // Confirm Bitcoin Balances
            let trader_m_balance = trader_m.wallet_spendable_balance().await.unwrap();
            assert_eq!(
                trader_m_balance,
                MAKER_BALANCE + (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64
            );

            // Maker - *User remits Fatcrabs

            maker
                .notify_peer(&maker_fatcrab_remittance_txid)
                .await
                .unwrap();

            maker.trade_complete().await.unwrap();
            trader_m.shutdown().await.unwrap();

            sleep(Duration::from_secs(1)).await;

            // Taker - Wait for Fatcrab Peer Message
            println!("Taker - Wait for Fatcrab Peer Message");
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;
            let takers = trader_t.get_takers().await;
            let taker_enum = takers.get(&trade_uuid).unwrap().to_owned();
            let taker = match taker_enum {
                FatCrabTakerAccessEnum::Sell(taker_access) => taker_access,
                _ => panic!("Taker is not a Sell Taker"),
            };
            let (taker_notif_tx, mut taker_notif_rx) =
                tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
            taker.register_notif_tx(taker_notif_tx).await.unwrap();
            trader_t.reconnect().await.unwrap();

            let taker_notif = taker_notif_rx.recv().await.unwrap();
            let _fatcrab_txid = match taker_notif {
                FatCrabTakerNotif::Peer(peer_notif) => peer_notif.peer_envelope.message.txid,
                _ => {
                    panic!("Taker only expects Peer Message at this point");
                }
            };

            taker.shutdown().await.unwrap();
            trader_t.shutdown().await.unwrap();
        }

        // Taker - Confirm Fatcrab Remittance
        {
            println!("Taker - Confirm Fatcrab Remittance");
            let trader_t = FatCrabTrader::new_with_key(privkey_t, info.clone(), "").await;
            trader_t.wallet_blockchain_sync().await.unwrap();

            let takers = trader_t.get_takers().await;
            let taker_enum = takers.get(&trade_uuid).unwrap().to_owned();
            let taker = match taker_enum {
                FatCrabTakerAccessEnum::Sell(taker_access) => taker_access,
                _ => panic!("Taker is not a Sell Taker"),
            };
            let (taker_notif_tx, _) = tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
            taker.register_notif_tx(taker_notif_tx).await.unwrap();
            trader_t.reconnect().await.unwrap();

            let trader_t_balance = trader_t.wallet_spendable_balance().await.unwrap();
            assert!(trader_t_balance < TAKER_BALANCE - (PURCHASE_AMOUNT * PURCHASE_PRICE) as u64);

            let peer_msg_envelope = taker.query_peer_msg().await.unwrap().unwrap();
            assert_eq!(
                &peer_msg_envelope.message.txid,
                &maker_fatcrab_remittance_txid
            );

            taker.trade_complete().await.unwrap();
            trader_t.shutdown().await.unwrap();
        }

        // Relays Shutdown
        relays.into_iter().for_each(|r| r.shutdown().unwrap());
    }
}

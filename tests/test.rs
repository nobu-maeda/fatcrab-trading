mod node;
mod relay;

#[cfg(test)]
mod test {
    use std::net::SocketAddr;

    use uuid::Uuid;

    use fatcrab_trading::{
        maker::FatCrabMakerNotif,
        order::{FatCrabOrder, FatCrabOrderType},
        taker::FatCrabTakerNotif,
        trade_rsp::FatCrabTradeRsp,
        trader::FatCrabTrader,
    };

    use crate::{node::Node, relay::Relay};

    #[tokio::test]
    async fn test() {
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
        let trader_m = FatCrabTrader::new(node.url(), node.auth(), node.network()).await;

        // TODO: Maker - Fund Maker Fatcrab Trader internal wallet from miner
        let address_m1 = trader_m.wallet_generate_receive_address().unwrap();
        let _txid1 = node.send_to_address(address_m1, 1000000);
        node.generate_blocks(1);
        trader_m.wallet_blockchain_sync().unwrap();
        assert_eq!(trader_m.wallet_spendable_balance().unwrap(), 1000000);

        // Taker - Create Fatcrab Trader for Taker
        let trader_t = FatCrabTrader::new(node.url(), node.auth(), node.network()).await;

        // Add Relays
        let mut relay_addrs: Vec<(String, Option<SocketAddr>)> = Vec::new();

        for relay in relays.iter_mut() {
            relay_addrs.push((format!("{}:{}", "ws://localhost", relay.port), None));
        }

        trader_m.add_relays(relay_addrs.clone()).await.unwrap();
        trader_t.add_relays(relay_addrs).await.unwrap();

        // Maker - Create Fatcrab Trade Order
        let maker_receive_fatcrab_acct_id = Uuid::new_v4().to_string();
        let order = FatCrabOrder {
            order_type: FatCrabOrderType::Buy,
            trade_uuid: Uuid::new_v4(),
            amount: 100.0,
            price: 1000.0,
        };

        // Maker - Create Fatcrab Maker
        let maker = trader_m
            .make_order(order, maker_receive_fatcrab_acct_id.clone())
            .await;

        // Maker - Create channels & register Notif Tx
        let (maker_notif_tx, mut maker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
        maker.register_notif_tx(maker_notif_tx).await.unwrap();

        // Taker - Query Fatcrab Trade Order
        let orders = trader_t.query_orders(FatCrabOrderType::Sell).await.unwrap();
        assert_eq!(orders.len(), 0);

        let orders = trader_t.query_orders(FatCrabOrderType::Buy).await.unwrap();
        assert_eq!(orders.len(), 1);

        // Taker - Create Fatcrab Take Trader & Take Trade Order
        let taker_receive_bitcoin_addr = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"; // TODO: Placeholder. Taker should generate with internal wallet
        let taker = trader_t
            .take_order(orders[0].clone(), taker_receive_bitcoin_addr)
            .await;

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
        let trade_rsp = FatCrabTradeRsp::Accept(maker_receive_fatcrab_acct_id.to_string());
        maker
            .trade_response(trade_rsp, offer_envelope)
            .await
            .unwrap();

        // Taker - Wait for Fatcrab Trade Response
        let taker_notif = taker_notif_rx.recv().await.unwrap();
        let maker_remitted_fatcrab_acct_id_string = match taker_notif {
            FatCrabTakerNotif::TradeRsp(trade_rsp_envelope) => match trade_rsp_envelope.trade_rsp {
                FatCrabTradeRsp::Accept(receive_address) => receive_address,
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
            maker_receive_fatcrab_acct_id
        );

        // Taker - User remitting Fatcrab

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
                assert_eq!(
                    &peer_msg_envelope.message.receive_address,
                    &taker_receive_bitcoin_addr.to_string()
                );
            }
            _ => {
                panic!("Maker only expects Peer Message at this point");
            }
        }

        // Maker - (Auto) Send Bitcoin to Taker Bitcoin address

        // Maker - (Auto) Send Fatcrab Peer Message with Bitcoin txid

        // Maker - (Auto) Trade Completion

        // Taker - Wait for Fatcrab Peer Message

        // Taker - Confirm Bitcoin txid

        // Taker - Trade Completion
    }
}

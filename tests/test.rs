mod relay;

#[cfg(test)]
mod test {
    use std::net::SocketAddr;

    use uuid::Uuid;

    use fatcrab_trading::{
        maker::FatCrabMakerNotif,
        offer::FatCrabOffer,
        order::{FatCrabOrder, FatCrabOrderType},
        taker::FatCrabTakerNotif,
        trade_rsp::FatCrabTradeRsp,
        trader::FatCrabTrader,
    };

    use crate::relay::Relay;

    #[tokio::test]
    async fn test() {
        // Initial Relays
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
        let trader_m = FatCrabTrader::new().await;

        // Taker - Create Fatcrab Trader fOR Taker
        let trader_t = FatCrabTrader::new().await;

        // Add Relays
        let mut relay_addrs: Vec<(String, Option<SocketAddr>)> = Vec::new();

        for relay in relays.iter_mut() {
            relay_addrs.push((format!("{}:{}", "ws://localhost", relay.port), None));
        }

        trader_m.add_relays(relay_addrs.clone()).await.unwrap();
        trader_t.add_relays(relay_addrs).await.unwrap();

        // Maker - Create Fatcrab Trade Order
        let maker_receive_fatcrab_acct_id = Uuid::new_v4();
        let order = FatCrabOrder::Buy {
            trade_uuid: Uuid::new_v4(),
            amount: 100.0,
            price: 1000.0,
            fatcrab_acct_id: maker_receive_fatcrab_acct_id,
        };

        // Maker - Create Fatcrab Maker
        let maker = trader_m.make_order(order).await;

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
        let taker_receive_bitcoin_addr = Uuid::new_v4().to_string();
        let offer = FatCrabOffer::Buy {
            bitcoin_addr: taker_receive_bitcoin_addr.clone(), // Use UUID as a placeholder of Bitcoin address
        };
        let taker = trader_t.take_order(orders[0].clone(), offer).await;

        let (taker_notif_tx, mut taker_notif_rx) =
            tokio::sync::mpsc::channel::<FatCrabTakerNotif>(5);
        taker.register_notif_tx(taker_notif_tx).await.unwrap();

        // Maker - Wait for Fatcrab Trader Order to be taken
        let maker_notif = maker_notif_rx.recv().await.unwrap();

        let offer_envelope = match maker_notif {
            FatCrabMakerNotif::Offer(offer_envelope) => match offer_envelope.offer.clone() {
                FatCrabOffer::Buy { bitcoin_addr } => {
                    assert_eq!(bitcoin_addr, taker_receive_bitcoin_addr);
                    offer_envelope
                }
                _ => {
                    panic!("Maker only expects Buy Offer Notif at this point");
                }
            },
            _ => {
                panic!("Maker only expects Buy Offer Notif at this point");
            }
        };

        // Maker - Send Fatcrab Trade Response w/ Fatcrab address
        let trade_rsp = FatCrabTradeRsp::Accept;
        maker
            .trade_response(trade_rsp, offer_envelope)
            .await
            .unwrap();

        // Taker - Wait for Fatcrab Trade Response
        let taker_notif = taker_notif_rx.recv().await.unwrap();

        match taker_notif {
            FatCrabTakerNotif::TradeResponse { trade_rsp_envelope } => {
                match trade_rsp_envelope.trade_rsp {
                    FatCrabTradeRsp::Accept => {}
                    _ => {
                        panic!("Taker only expects Accepted Trade Response at this point");
                    }
                }
            }
            _ => {
                panic!("Taker only expects Accepted Trade Response at this point");
            }
        }

        // Taker - Signal user to remit Fatcrab

        // Taker - User claims Fatcrab remittance, send Peer Message with Bitcoin address

        // Maker - Wait for Fatcrab Peer Message

        // Maker - Send Bitcoin to Taker Bitcoin address

        // Maker - Send Fatcrab Peer Message with Bitcoin txid

        // Maker - Trade Completion

        // Taker - Wait for Fatcrab Peer Message

        // Taker - Confirm Bitcoin txid

        // Taker - Trade Completion
    }
}
